use std::{io::Write, os::windows::process::CommandExt, process::{Command, Stdio}};

const NOTES: &str = "https://mediaserver.newellrubbermaid.com/dls/DCDesktop/win/en/Content/WhatsNew.htm";
pub fn check() -> Result<String, String> {
    let bridge = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("Application folder missing")?.join("sdk").join("DymoBridge.exe");
    let mut child = Command::new(bridge).creation_flags(0x08000000).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    child.stdin.take().ok_or("Cannot check installed version")?.write_all(br#"{"action":"versions"}"#).map_err(|e| e.to_string())?;
    let result = crate::sdk::wait_child(child, std::time::Duration::from_secs(15))?;
    if !result.status.success() { return Err("Could not check installed DYMO Connect".into()); }
    let json: serde_json::Value = serde_json::from_slice(&result.stdout).map_err(|e| e.to_string())?;
    let installed = json["dymoConnect"].as_str().unwrap_or("not installed");
    let sdk = json["sdk"].as_str().unwrap_or("unknown");
    let root = std::env::var_os("SystemRoot").ok_or("Windows directory unavailable")?;
    let output = Command::new(std::path::PathBuf::from(root).join("System32").join("curl.exe"))
        .creation_flags(0x08000000).args(["--silent", "--show-error", "--fail", "--connect-timeout", "4", "--max-time", "12", "--proto", "=https", NOTES]).output().map_err(|e| e.to_string())?;
    if !output.status.success() { return Err(format!("Installed DYMO Connect: {installed}. Could not check DYMO release notes.")); }
    let page = String::from_utf8_lossy(&output.stdout);
    let latest = parse_version(&page).ok_or("DYMO release notes did not contain a recognisable version")?;
    let status = match compare_release(installed, latest) {
        Some(std::cmp::Ordering::Less) => "A newer DYMO Connect version is listed.",
        Some(std::cmp::Ordering::Equal) => "DYMO Connect is on the listed release.",
        Some(_) => "The installed version is newer than the listed release notes.",
        None => "Compare the versions before updating DYMO Connect.",
    };
    Ok(format!("Installed DYMO Connect: {installed}\nLatest version in DYMO release notes: {latest}\n{status}\nBundled DYMO SDK: {sdk}\nFirmware: no verified 420P update source.\nThis app: updates are installed separately."))
}
fn parse_version(page: &str) -> Option<&str> {
    page.split("Version ").skip(1).filter_map(|rest| {
        let end = rest.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(rest.len());
        let version = rest[..end].trim_end_matches('.');
        version_parts(version).map(|parts| (version, parts))
    }).max_by_key(|(_, parts)| *parts).map(|(version, _)| version)
}
fn version_parts(version: &str) -> Option<[u32; 4]> {
    let parts: Vec<_> = version.split('.').collect();
    if !(2..=4).contains(&parts.len()) { return None; }
    let mut result = [0; 4];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) { return None; }
        result[i] = part.parse().ok()?;
    }
    Some(result)
}
fn compare_release(installed: &str, listed: &str) -> Option<std::cmp::Ordering> {
    let mut installed = version_parts(installed)?;
    let latest = version_parts(listed)?;
    // Release notes often omit the installed Windows build component.
    for part in installed.iter_mut().skip(listed.split('.').count()) { *part = 0; }
    Some(installed.cmp(&latest))
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn parses_release_notes() {
        assert_eq!(parse_version("<h2>Version 1.6.4</h2>"), Some("1.6.4"));
        assert_eq!(parse_version("no version"), None);
    }
    #[test] fn selects_highest_numeric_release_and_rejects_malformed_versions() {
        assert_eq!(parse_version("Version ... Version 1.9 Version 1.10.0 Version 1.6.4.11"), Some("1.10.0"));
        assert_eq!(version_parts("1.6.4"), version_parts("1.6.4.0"));
        assert_eq!(compare_release("1.6.4.6", "1.6.4"), Some(std::cmp::Ordering::Equal));
        assert_eq!(compare_release("1.6.4.6", "1.6.4.7"), Some(std::cmp::Ordering::Less));
        for invalid in ["1..6", "...", "1", "1.6.beta", "1.6.4.0.1", "999999999999.6"] { assert_eq!(version_parts(invalid), None); }
    }
}
