//! Access the DYMO Connect service on loopback only.
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Mutex, atomic::{AtomicU16, Ordering}};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const PRINTER: &str = "DYMO LabelManager 420P";
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(12);
const LAUNCH_COOLDOWN: Duration = Duration::from_secs(60);
static LAST_PORT: AtomicU16 = AtomicU16::new(0);
static LAST_LAUNCH_ATTEMPT: Mutex<Option<Instant>> = Mutex::new(None);

fn call(port: u16, endpoint: &str, fields: &[(&str, &str)], timeout: Duration) -> Result<String, String> {
    if !(41951..=41960).contains(&port) || !matches!(endpoint, "GetPrinters" | "RenderLabel") {
        return Err("Invalid local service request".into());
    }
    let system = std::env::var_os("SystemRoot").ok_or("Windows directory unavailable")?;
    let curl = std::path::PathBuf::from(system).join("System32").join("curl.exe");
    let mut cmd = Command::new(curl);
    let seconds = format!("{:.3}", timeout.as_secs_f64().max(0.001));
    cmd.creation_flags(CREATE_NO_WINDOW).args(["--silent", "--show-error", "--fail", "--ssl-no-revoke", "--connect-timeout", &seconds, "--max-time", &seconds, "--noproxy", "*", "--proto", "=https"]);
    if !fields.is_empty() {
        cmd.args(["--request", "POST", "--header", "Content-Type: application/x-www-form-urlencoded", "--data-binary", "@-"]);
    }
    cmd.arg(format!("https://127.0.0.1:{port}/DYMO/DLS/Printing/{endpoint}"));
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    if !fields.is_empty() {
        let body = fields.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&");
        child.stdin.take().ok_or("Could not open request")?.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    }
    let out = crate::sdk::wait_child(child, timeout + Duration::from_secs(1))?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned()); }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { out.push(b as char); }
        else { out.push_str(&format!("%{b:02X}")); }
    }
    out
}

pub fn discover() -> Result<(u16, bool), String> {
    if !crate::sdk::usb_present()? { return Ok((0, false)); }
    // Reserve time for service recovery instead of spending an independent
    // eight-second timeout on every occupied loopback port in both scans.
    let deadline = Instant::now() + DISCOVERY_TIMEOUT;
    if let Some(found) = scan((Instant::now() + Duration::from_secs(4)).min(deadline)) { return Ok(found); }
    let start_host = claim_launch(&mut *LAST_LAUNCH_ATTEMPT.lock().map_err(|_| "DYMO service recovery state unavailable")?, Instant::now());
    if start_host {
        let root = std::env::var_os("ProgramFiles(x86)").ok_or("DYMO Connect is not installed")?;
        let host = std::path::PathBuf::from(root).join("DYMO").join("DYMO Connect").join("DYMO.WebApi.Win.Host.exe");
        Command::new(host).creation_flags(CREATE_NO_WINDOW).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("Cannot start DYMO Connect service: {e}"))?;
        std::thread::sleep(Duration::from_secs(3).min(deadline.saturating_duration_since(Instant::now())));
    }
    scan(deadline).ok_or_else(|| "The DYMO 420P was not found by DYMO Connect.".into())
}

fn claim_launch(last_attempt: &mut Option<Instant>, now: Instant) -> bool {
    if last_attempt.is_some_and(|last| now.saturating_duration_since(last) < LAUNCH_COOLDOWN) { return false; }
    // Failed launches also back off, but can be retried without restarting the app.
    *last_attempt = Some(now);
    true
}

fn port_order(cached: u16) -> Vec<u16> {
    let mut ports = Vec::with_capacity(10);
    if (41951..=41960).contains(&cached) { ports.push(cached); }
    ports.extend((41951..=41960).filter(|port| *port != cached));
    ports
}

fn scan(deadline: Instant) -> Option<(u16, bool)> {
    for port in port_order(LAST_PORT.load(Ordering::Relaxed)) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { break; }
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(150).min(remaining)).is_err() { continue; }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { break; }
        if let Ok(xml) = call(port, "GetPrinters", &[], Duration::from_secs(2).min(remaining)) {
            for tape in xml.split("<TapePrinter>").skip(1).filter_map(|p| p.split("</TapePrinter>").next()) {
                if tape.contains(&format!("<ModelName>{PRINTER}</ModelName>")) && tape.contains(&format!("<Name>{PRINTER}</Name>")) {
                    LAST_PORT.store(port, Ordering::Relaxed);
                    return Some((port, tape.contains("<IsConnected>True</IsConnected>")));
                }
            }
        }
        let _ = LAST_PORT.compare_exchange(port, 0, Ordering::Relaxed, Ordering::Relaxed);
    }
    None
}

#[allow(dead_code)]
pub fn render(port: u16, label_xml: &str) -> Result<String, String> {
    call(port, "RenderLabel", &[("labelXml", label_xml), ("renderParamsXml", ""), ("printerName", PRINTER)], Duration::from_secs(8))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn encodes_xml_and_unicode_as_form_data() {
        assert_eq!(encode("<a>£ &</a>"), "%3Ca%3E%C2%A3%20%26%3C%2Fa%3E");
    }
    #[test] fn service_launch_retries_after_cooldown_even_after_failed_attempt() {
        let start = Instant::now();
        let mut last_attempt = None;
        assert!(claim_launch(&mut last_attempt, start));
        assert!(!claim_launch(&mut last_attempt, start + Duration::from_secs(15)));
        assert!(!claim_launch(&mut last_attempt, start + Duration::from_secs(59)));
        assert!(claim_launch(&mut last_attempt, start + Duration::from_secs(60)));
        assert!(!claim_launch(&mut last_attempt, start + Duration::from_secs(61)));
    }
    #[test] fn cached_port_is_first_without_losing_fallback_ports() {
        let ports = port_order(41957);
        assert_eq!(ports[0], 41957);
        assert_eq!(ports.len(), 10);
        let mut sorted = ports; sorted.sort_unstable();
        assert_eq!(sorted, (41951..=41960).collect::<Vec<_>>());
        assert_eq!(port_order(0), (41951..=41960).collect::<Vec<_>>());
        assert_eq!(port_order(65535), port_order(0));
    }
}
