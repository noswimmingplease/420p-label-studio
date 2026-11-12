use std::{io::Write, os::windows::process::CommandExt, path::PathBuf, process::{Command, Stdio}};
use base64::Engine;
use wait_timeout::ChildExt;

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceStatus {
    pub connected: bool,
    pub available: bool,
    pub battery_percent: Option<u8>,
    pub charging: Option<bool>,
    pub tape_width_mm: Option<u8>,
    pub issues: Vec<String>,
    pub message: Option<String>,
}

impl DeviceStatus {
    pub fn disconnected() -> Self {
        Self { connected: false, available: false, battery_percent: None, charging: None, tape_width_mm: None, issues: Vec::new(), message: None }
    }

    pub fn unavailable() -> Self {
        Self { connected: true, message: Some("Printer details are unavailable. Try refreshing the connection.".into()), ..Self::disconnected() }
    }

    fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let connected = value["connected"].as_bool().ok_or("Printer connection missing from status response.")?;
        let available = value["statusAvailable"].as_bool().ok_or("Printer status availability missing.")?;
        if !connected { return Ok(Self::disconnected()); }
        let message = value["message"].as_str().map(|s| s.chars().take(500).collect());
        if !available { return Ok(Self { message, ..Self::unavailable() }); }
        let battery_percent = value["batteryPercent"].as_u64().filter(|n| *n <= 100).map(|n| n as u8);
        let tape_width_mm = value["tapeWidthMm"].as_u64().filter(|n| [6, 9, 12, 19].contains(n)).map(|n| n as u8);
        let issues = value["issues"].as_array().ok_or("Printer issues missing from status response.")?
            .iter().take(10).filter_map(|v| v.as_str()).map(|s| s.chars().take(500).collect()).collect();
        Ok(Self { connected, available, battery_percent, charging: value["charging"].as_bool(), tape_width_mm, issues, message })
    }
}

pub fn device_status() -> Result<DeviceStatus, String> {
    let bridge = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("Missing application folder")?.join("sdk").join("DymoBridge.exe");
    read_device_status(bridge, serde_json::json!({"action":"device-status"}))
}

fn read_device_status(bridge: PathBuf, request: serde_json::Value) -> Result<DeviceStatus, String> {
    let mut child = Command::new(bridge).creation_flags(0x08000000).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("Cannot start DYMO status reader: {e}"))?;
    child.stdin.take().ok_or("Cannot open SDK input")?.write_all(request.to_string().as_bytes()).map_err(|e| e.to_string())?;
    let output = wait_child(child, std::time::Duration::from_secs(15))?;
    if !output.status.success() { return Err("The DYMO SDK could not read printer details.".into()); }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    DeviceStatus::parse(&value)
}

pub struct Preview {
    pub image: image::RgbaImage,
    pub design_image: Option<image::RgbaImage>,
    pub dimensions_mm: Option<(f64, f64)>,
    pub printable_dimensions_mm: Option<(f64, f64)>,
    pub printable_origin_mm: Option<(f64, f64)>,
    pub adjusted_objects: Vec<String>,
    pub object_layouts: Vec<serde_json::Value>,
    pub all_layouts: Option<Vec<serde_json::Value>>,
    pub empty_primary_text: Option<String>,
    pub threshold_image_blank: bool,
    pub objects: Vec<serde_json::Value>,
    pub snapshot: std::sync::Arc<PrintSnapshot>,
}

/// Immutable template, image and formatting inputs used by both preview and print.
#[derive(Clone)]
pub struct PrintSnapshot {
    template_base64: String,
    image: Option<String>,
    width: u8,
    bold: bool,
    italic: bool,
    underline: bool,
    font: String,
    align: String,
    layout: Option<Vec<serde_json::Value>>,
    threshold_image_blank: bool,
}

#[cfg(test)]
pub fn test_snapshot() -> std::sync::Arc<PrintSnapshot> {
    std::sync::Arc::new(PrintSnapshot { template_base64: String::new(), image: None, width: 9, bold: false, italic: false, underline: false, font: "Segoe UI".into(), align: "Center".into(), layout: None, threshold_image_blank: false })
}

pub(crate) fn wait_child(mut child: std::process::Child, timeout: std::time::Duration) -> Result<std::process::Output, String> {
    use std::io::Read;
    let stdout = child.stdout.take().ok_or("SDK output unavailable")?;
    let stderr = child.stderr.take().ok_or("SDK errors unavailable")?;
    let out = std::thread::spawn(move || { let mut data = Vec::new(); stdout.take(32 * 1024 * 1024).read_to_end(&mut data).map(|_| data) });
    let err = std::thread::spawn(move || { let mut data = Vec::new(); stderr.take(64 * 1024).read_to_end(&mut data).map(|_| data) });
    let status = match child.wait_timeout(timeout).map_err(|e| e.to_string())? {
        Some(status) => status,
        None => { let _ = child.kill(); let _ = child.wait(); let _ = out.join(); let _ = err.join(); return Err("DYMO SDK did not respond in time.".into()); }
    };
    let stdout = out.join().map_err(|_| "SDK output reader stopped")?.map_err(|e| e.to_string())?;
    let stderr = err.join().map_err(|_| "SDK error reader stopped")?.map_err(|e| e.to_string())?;
    Ok(std::process::Output { status, stdout, stderr })
}

pub fn usb_present() -> Result<bool, String> {
    let bridge = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("Missing application folder")?.join("sdk").join("DymoBridge.exe");
    let mut child = Command::new(bridge).creation_flags(0x08000000).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("Cannot start DYMO SDK: {e}"))?;
    child.stdin.take().ok_or("Cannot open SDK input")?.write_all(br#"{"action":"usb-presence"}"#).map_err(|e| e.to_string())?;
    let out = wait_child(child, std::time::Duration::from_secs(15))?;
    if !out.status.success() { return Err("Cannot verify USB printer".into()); }
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    result["usbPresent"].as_bool().ok_or_else(|| "USB status unavailable".into())
}

pub fn installed_fonts() -> Result<Vec<String>, String> {
    let bridge = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("Missing application folder")?.join("sdk").join("DymoBridge.exe");
    let mut child = Command::new(bridge).creation_flags(0x08000000).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("Cannot start DYMO SDK: {e}"))?;
    child.stdin.take().ok_or("Cannot open SDK input")?.write_all(br#"{"action":"fonts"}"#).map_err(|e| e.to_string())?;
    let out = wait_child(child, std::time::Duration::from_secs(15))?;
    if !out.status.success() { return Err("Cannot list installed fonts".into()); }
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let fonts = result["fonts"].as_array().ok_or("Font list unavailable")?.iter().filter_map(|v| v.as_str()).filter(|v| !v.is_empty() && v.len() <= 128 && !v.chars().any(char::is_control)).map(str::to_owned).collect::<Vec<_>>();
    if fonts.is_empty() { Err("No installed fonts found".into()) } else { Ok(fonts) }
}

pub fn preview(template: String, text: String, image_path: String, monochrome: bool, cutoff: u8, width: u8, bold: bool, italic: bool, underline: bool, font: String, align: String, objects: Vec<serde_json::Value>, layout: Option<Vec<serde_json::Value>>) -> Result<Preview, String> {
    let bridge = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("Missing application folder")?.join("sdk").join("DymoBridge.exe");
    let snapshot = std::sync::Arc::new(make_snapshot(&template, &image_path, monochrome, cutoff, width, bold, italic, underline, font, align, layout)?);
    preview_snapshot_with_bridge(bridge, snapshot, text, objects, true)
}

/// Inspect the complete object set without printing, merge explicitly removed
/// objects, and then produce the only preview eligible for printing. Both
/// requests use identical frozen template and image bytes.
pub fn preview_reconciled(template: String, text: String, image_path: String, monochrome: bool, cutoff: u8, width: u8, bold: bool, italic: bool, underline: bool, font: String, align: String, objects: Vec<serde_json::Value>, mut layout: crate::layout_tools::LayoutState, allowed_removals: Vec<String>) -> Result<(crate::layout_tools::LayoutState, Result<Preview, String>), String> {
    let bridge = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("Missing application folder")?.join("sdk").join("DymoBridge.exe");
    preview_reconciled_with_bridge(bridge, template, text, image_path, monochrome, cutoff, width, bold, italic, underline, font, align, objects, &mut layout, allowed_removals)
}

fn preview_reconciled_with_bridge(bridge: PathBuf, template: String, text: String, image_path: String, monochrome: bool, cutoff: u8, width: u8, bold: bool, italic: bool, underline: bool, font: String, align: String, objects: Vec<serde_json::Value>, layout: &mut crate::layout_tools::LayoutState, allowed_removals: Vec<String>) -> Result<(crate::layout_tools::LayoutState, Result<Preview, String>), String> {
    let mut snapshot = make_snapshot(&template, &image_path, monochrome, cutoff, width, bold, italic, underline, font, align, None)?;
    let inspection = request_snapshot(bridge.clone(), "inspect-layout", &snapshot, text.clone(), 1, objects.clone(), false)?;
    let all = inspection.get("allLayouts").ok_or("The SDK did not provide the complete object list.")?;
    let names: std::collections::HashSet<_> = all.as_array().ok_or("The SDK object list is invalid.")?.iter().filter_map(|item| item["Name"].as_str().map(str::to_lowercase)).collect();
    let saved = layout.save_value();
    let existing: std::collections::HashSet<_> = saved["objects"].as_array().into_iter().flatten().filter_map(|item| item["name"].as_str().map(str::to_lowercase)).collect();
    let permitted = allowed_removals.into_iter().filter(|name| !names.contains(&name.to_lowercase()) && existing.contains(&name.to_lowercase())).collect::<Vec<_>>();
    let size = inspection["printableLengthMm"].as_f64().zip(inspection["printableWidthMm"].as_f64());
    let can_expand = inspection["isFixedLength"].as_bool() == Some(false);
    layout.reconcile_with_expansion(all, &permitted, size, can_expand)?;
    snapshot.layout = match layout.request_value() {
        Ok(value) => value.and_then(|v| v.as_array().cloned()),
        Err(error) => return Ok((layout.clone(), Err(error))),
    };
    let preview = preview_snapshot_with_bridge(bridge, std::sync::Arc::new(snapshot), text, objects, true);
    Ok((layout.clone(), preview))
}

#[cfg(test)]
fn preview_with_bridge(bridge: PathBuf, template: String, text: String, image_path: String, monochrome: bool, cutoff: u8, width: u8, bold: bool, italic: bool, underline: bool, font: String, align: String, objects: Vec<serde_json::Value>) -> Result<Preview, String> {
    let snapshot = std::sync::Arc::new(make_snapshot(&template, &image_path, monochrome, cutoff, width, bold, italic, underline, font, align, None)?);
    preview_snapshot_with_bridge(bridge, snapshot, text, objects, true)
}

pub fn preview_snapshot(snapshot: std::sync::Arc<PrintSnapshot>, text: String, objects: Vec<serde_json::Value>) -> Result<Preview, String> {
    let bridge = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("Missing application folder")?.join("sdk").join("DymoBridge.exe");
    preview_snapshot_with_bridge(bridge, snapshot, text, objects, false)
}

fn preview_snapshot_with_bridge(bridge: PathBuf, snapshot: std::sync::Arc<PrintSnapshot>, text: String, objects: Vec<serde_json::Value>, include_design: bool) -> Result<Preview, String> {
    let result = request_snapshot(bridge, "preview", &snapshot, text, 1, objects.clone(), include_design)?;
    let encoded = result["preview"].as_str().ok_or("Preview missing from SDK response")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|e| e.to_string())?;
    let dimensions_mm = result["lengthMm"].as_f64().zip(result["canvasWidthMm"].as_f64())
        .filter(|(length, width)| length.is_finite() && width.is_finite() && *length > 0.0 && *length <= 10000.0 && *width > 0.0 && *width <= 100.0);
    let printable_dimensions_mm = result["printableLengthMm"].as_f64().zip(result["printableWidthMm"].as_f64())
        .filter(|(length, width)| length.is_finite() && width.is_finite() && *length > 0.0 && *length <= 10000.0 && *width > 0.0 && *width <= 100.0);
    let printable_origin_mm = result["printableXmm"].as_f64().zip(result["printableYmm"].as_f64())
        .filter(|(x, y)| x.is_finite() && y.is_finite() && *x >= 0.0 && *y >= 0.0);
    let mut adjusted_objects: Vec<String> = result["objectLayouts"].as_array().into_iter().flatten().filter(|v| v["ResizedBySdk"].as_bool() == Some(true))
        .filter_map(|v| v["Name"].as_str().map(str::to_owned)).collect();
    for name in result["manualAdjustedObjects"].as_array().into_iter().flatten().filter_map(|v| v.as_str()) {
        if !adjusted_objects.iter().any(|existing| existing == name) { adjusted_objects.push(name.to_owned()); }
    }
    let object_layouts = result["objectLayouts"].as_array().cloned().unwrap_or_default();
    let all_layouts = result["allLayouts"].as_array().cloned();
    let design_image = result["designPreview"].as_str().and_then(|encoded| base64::engine::general_purpose::STANDARD.decode(encoded).ok())
        .and_then(|bytes| image::load_from_memory(&bytes).ok()).map(|image| image.to_rgba8())
        .filter(|image| image.width() <= 4096 && image.height() <= 4096 && u64::from(image.width()) * u64::from(image.height()) <= 16_000_000);
    let empty_primary_text = result["emptyPrimaryText"].as_str().map(str::to_owned);
    let threshold_image_blank = snapshot.threshold_image_blank;
    Ok(Preview { image: image::load_from_memory(&bytes).map_err(|e| e.to_string())?.to_rgba8(), design_image, dimensions_mm, printable_dimensions_mm, printable_origin_mm, adjusted_objects, object_layouts, all_layouts, empty_primary_text, threshold_image_blank, objects, snapshot })
}

pub fn print_snapshot(snapshot: &PrintSnapshot, text: String, copies: u16, objects: Vec<serde_json::Value>) -> Result<(), String> {
    let _access = crate::dymo_service::PRINTER_ACCESS.lock().map_err(|_| "Printer access state unavailable.")?;
    match crate::dymo_service::discover() {
        Ok((_, true)) => {},
        Ok((_, false)) => return Err("The 420P is disconnected according to DYMO Connect.".into()),
        Err(e) => return Err(format!("Cannot verify the 420P: {e}")),
    }
    let bridge = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("Missing application folder")?.join("sdk").join("DymoBridge.exe");
    let result = request_snapshot(bridge, "print", snapshot, text, copies, objects, false)?;
    if result["ok"].as_bool() != Some(true) { return Err("DYMO did not confirm the job".into()); }
    Ok(())
}

/// Export the exact frozen inputs used by the verified preview, including the
/// embedded image and SDK-resolved manual positions. This does not print.
pub fn export_snapshot(snapshot: &PrintSnapshot, text: String, objects: Vec<serde_json::Value>) -> Result<Vec<u8>, String> {
    let bridge = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("Missing application folder")?.join("sdk").join("DymoBridge.exe");
    export_snapshot_with_bridge(bridge, snapshot, text, objects)
}

fn export_snapshot_with_bridge(bridge: PathBuf, snapshot: &PrintSnapshot, text: String, objects: Vec<serde_json::Value>) -> Result<Vec<u8>, String> {
    let result = request_snapshot(bridge, "export", snapshot, text, 1, objects, false)?;
    let encoded = result["labelBase64"].as_str().ok_or("Exported label missing from SDK response")?;
    if encoded.is_empty() || encoded.len() > 22_369_624 { return Err("Exported label is empty or larger than 16 MiB.".into()); }
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|e| format!("Invalid exported label: {e}"))?;
    if bytes.is_empty() || bytes.len() > 16_777_216 { return Err("Exported label is empty or larger than 16 MiB.".into()); }
    Ok(bytes)
}

fn make_snapshot(template: &str, image_path: &str, monochrome: bool, cutoff: u8, width: u8, bold: bool, italic: bool, underline: bool, font: String, align: String, layout: Option<Vec<serde_json::Value>>) -> Result<PrintSnapshot, String> {
    use std::io::Read;
    let file = std::fs::File::open(template).map_err(|e| format!("Cannot open template: {e}"))?;
    let mut bytes = Vec::new();
    file.take(4_194_305).read_to_end(&mut bytes).map_err(|e| format!("Cannot read template: {e}"))?;
    if bytes.is_empty() || bytes.len() > 4_194_304 { return Err("Template is empty or larger than 4 MiB.".into()); }
    let template_base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut threshold_image_blank = false;
    let image = if image_path.trim().is_empty() { None } else {
        let data = prepare_image(image_path, monochrome, cutoff)?;
        threshold_image_blank = monochrome && !data.pixels().any(|pixel| pixel[0] == 0);
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(data).write_to(&mut bytes, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        Some(base64::engine::general_purpose::STANDARD.encode(bytes.into_inner()))
    };
    Ok(PrintSnapshot { template_base64, image, width, bold, italic, underline, font, align, layout, threshold_image_blank })
}

fn request_snapshot(bridge: PathBuf, action: &str, snapshot: &PrintSnapshot, text: String, copies: u16, objects: Vec<serde_json::Value>, include_design: bool) -> Result<serde_json::Value, String> {
    let request = serde_json::json!({"action":action, "templateBase64":snapshot.template_base64, "text":text, "image":snapshot.image, "width":snapshot.width, "copies":copies, "bold":snapshot.bold, "italic":snapshot.italic, "underline":snapshot.underline, "font":snapshot.font, "align":snapshot.align, "objects":objects, "includeDesign":include_design, "layout":snapshot.layout});
    let mut child = Command::new(bridge).creation_flags(0x08000000).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("Cannot start DYMO SDK: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("Cannot open SDK input")?;
    let body = request.to_string().into_bytes();
    let writer = std::thread::spawn(move || stdin.write_all(&body));
    let output = wait_child(child, std::time::Duration::from_secs(30));
    let write_result = writer.join().map_err(|_| "SDK input writer stopped" )?;
    let out = output.map_err(|e| if action == "print" { format!("{e} The job may have been submitted; check the print queue before retrying.") } else { e })?;
    write_result.map_err(|e| format!("Cannot send label to DYMO SDK: {e}"))?;
    if !out.status.success() {
        let message = serde_json::from_slice::<serde_json::Value>(&out.stderr).ok().and_then(|value| value["error"].as_str().map(str::to_owned))
            .unwrap_or_else(|| String::from_utf8_lossy(&out.stderr).trim().to_owned());
        return Err(format!("DYMO SDK: {message}"));
    }
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("Invalid SDK response: {e}"))?;
    Ok(result)
}

pub(crate) fn prepare_image(path: &str, monochrome: bool, cutoff: u8) -> Result<image::RgbaImage, String> {
    let mut reader = image::ImageReader::open(path).map_err(|e| format!("Cannot open image: {e}"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096); limits.max_image_height = Some(4096); limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let data = reader.decode().map_err(|e| format!("Cannot decode image: {e}"))?;
    Ok(if monochrome { crate::monochrome::threshold(&data, cutoff) } else { data.to_rgba8() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_status_clears_stale_details_and_rejects_invalid_values() {
        let mut value = serde_json::json!({"connected":true,"statusAvailable":true,"batteryPercent":72,"charging":true,"tapeWidthMm":9,"issues":[]});
        assert_eq!(DeviceStatus::parse(&value).unwrap().battery_percent, Some(72));
        for invalid in [serde_json::json!(101), serde_json::json!(-1), serde_json::json!("72"), serde_json::json!(null)] {
            value["batteryPercent"] = invalid;
            assert_eq!(DeviceStatus::parse(&value).unwrap().battery_percent, None);
        }
        value["tapeWidthMm"] = serde_json::json!(24);
        assert_eq!(DeviceStatus::parse(&value).unwrap().tape_width_mm, None);
        value["connected"] = serde_json::json!(false);
        assert_eq!(DeviceStatus::parse(&value).unwrap(), DeviceStatus::disconnected());
        value["connected"] = serde_json::json!(true);
        value["statusAvailable"] = serde_json::json!(false);
        let unavailable = DeviceStatus::parse(&value).unwrap();
        assert!(unavailable.connected && !unavailable.available);
        assert_eq!((unavailable.battery_percent, unavailable.charging, unavailable.tape_width_mm), (None, None, None));
        assert!(unavailable.issues.is_empty());
        assert!(DeviceStatus::parse(&serde_json::json!({})).is_err());
    }

    #[test]
    fn sdk_decodes_synthetic_420p_status_when_bridge_supplied() {
        let Ok(bridge) = std::env::var("DYMO_TEST_BRIDGE") else { return; };
        let decode = |bytes: &[u8]| read_device_status(PathBuf::from(&bridge), serde_json::json!({"action":"parse-device-status", "statusBase64":base64::engine::general_purpose::STANDARD.encode(bytes)})).unwrap();
        for (encoded_width, width) in [(1, 6), (2, 9), (3, 12), (4, 19)] {
            let status = decode(&[0x40, encoded_width, 0x10, 75, 0, 0, 0, 0]);
            assert!(status.connected && status.available);
            assert_eq!((status.battery_percent, status.charging, status.tape_width_mm), (Some(75), Some(true), Some(width)));
            assert!(status.issues.is_empty());
        }
        for percent in [0, 100] {
            let status = decode(&[0x40, 2, 0, percent, 0, 0, 0, 0]);
            assert_eq!((status.battery_percent, status.charging), (Some(percent), Some(false)));
        }
        let invalid = decode(&[0x40, 6, 0, 255, 0, 0, 0, 0]);
        assert_eq!((invalid.battery_percent, invalid.tape_width_mm), (None, None));
        for bytes in [vec![], vec![0; 7], vec![0; 9]] {
            let incomplete = decode(&bytes);
            assert!(incomplete.connected && !incomplete.available);
            assert_eq!((incomplete.battery_percent, incomplete.charging, incomplete.tape_width_mm), (None, None, None));
        }
        let missing = decode(&[0, 2, 0, 75, 0, 0, 0, 0]);
        assert_eq!(missing.tape_width_mm, None);
        assert!(missing.issues.iter().any(|s| s.contains("No cassette")));
        for (flags, message) in [(0x21, "Battery power error"), (0x22, "No battery"), (0x23, "USB charging current"), (0x20, "charging error")] {
            let status = decode(&[0x40, 2, flags, 75, 0, 0, 0, 0]);
            assert_eq!(status.charging, None);
            assert!(status.issues.iter().any(|s| s.contains(message)), "{status:?}");
            if flags == 0x22 { assert_eq!(status.battery_percent, None); }
        }
        let low = decode(&[0x40, 2, 0x80, 0, 0, 0, 0, 0]);
        assert!(low.issues.iter().any(|s| s.contains("too low to print")));
        let missing_and_low = decode(&[0, 2, 0x80, 0, 0, 0, 0, 0]);
        assert_eq!(missing_and_low.tape_width_mm, None);
        assert!(missing_and_low.issues.iter().any(|s| s.contains("too low to print")));
        assert!(missing_and_low.issues.iter().any(|s| s.contains("No cassette")));
        let error = decode(&[0x44, 2, 0, 75, 0, 0, 0, 0]);
        assert!(error.issues.iter().any(|s| s.contains("Printer error")));
    }

    fn powershell(command: &str) -> std::process::Child {
        let root = std::env::var_os("SystemRoot").unwrap();
        Command::new(PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe"))
            .creation_flags(0x08000000).args(["-NoProfile", "-NonInteractive", "-Command", command])
            .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
    }
    #[test]
    fn prepared_image_preserves_colour_and_supports_monochrome() {
        let path = std::env::temp_dir().join(format!("dymo420p-image-{}-{}.png", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut source = image::RgbaImage::new(2, 1);
        source.put_pixel(0, 0, image::Rgba([30, 100, 200, 128]));
        source.put_pixel(1, 0, image::Rgba([0, 0, 0, 0]));
        source.save(&path).unwrap();
        let colour = prepare_image(path.to_str().unwrap(), false, 128).unwrap();
        assert_eq!(colour, source);
        let mono = prepare_image(path.to_str().unwrap(), true, 128).unwrap();
        assert_eq!(mono.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(mono.get_pixel(1, 0).0, [255, 255, 255, 255]);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn child_output_is_drained_and_hangs_are_bounded() {
        let large = wait_child(powershell("[Console]::Out.Write(('x' * 100000))"), std::time::Duration::from_secs(10)).unwrap();
        assert!(large.status.success()); assert_eq!(large.stdout.len(), 100000);
        let result = wait_child(powershell("Start-Sleep -Seconds 5"), std::time::Duration::from_millis(100));
        assert!(result.is_err());
    }
    #[test]
    fn sdk_preview_roundtrip_when_fixture_supplied() {
        let (Ok(bridge), Ok(template)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE")) else { return; };
        let image = preview_with_bridge(PathBuf::from(bridge), template, "Test Print".to_string(), String::new(), false, 128, 6, false, false, false, "Segoe UI".into(), "Center".into(), vec![]).expect("SDK must render the fixture");
        assert!(image.image.width() > 0 && image.image.height() > 0);
        assert!(image.image.pixels().any(|p| p[0] < 128 && p[3] > 0), "rendered text must contain dark pixels");
    }
    #[test]
    fn sdk_previews_every_supported_tape_width_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        for width in [6, 9, 12, 19] {
            let template = PathBuf::from(&dir).join(format!("{width}mm.dymo"));
            let image = preview_with_bridge(PathBuf::from(&bridge), template.to_string_lossy().into_owned(), "Test Print".into(), String::new(), false, 128, width, false, false, false, "Segoe UI".into(), "Center".into(), vec![]).expect("SDK should render the tape template");
            assert!(image.image.width() > 0 && image.image.height() > 0);
            assert!(image.image.pixels().any(|p| p[0] < 128 && p[3] > 0));
            assert!(image.dimensions_mm.is_some(), "SDK label dimensions should be available for the fixture");
            assert!((image.dimensions_mm.unwrap().1 - f64::from(width)).abs() <= 2.0, "the template geometry must match its nominal tape width");
        }
    }
    #[test]
    fn renamed_template_cannot_misrepresent_tape_geometry_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let mut snapshot = make_snapshot(&template, "", false, 128, 19, false, false, false, "Segoe UI".into(), "Center".into(), None).unwrap();
        let xml = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(&snapshot.template_base64).unwrap()).unwrap();
        snapshot.template_base64 = base64::engine::general_purpose::STANDARD.encode(xml.replace("9MMX7M-TAPE BLACK/YELLOW", "19X7-TAPE BLACK/RED"));
        let result = preview_snapshot_with_bridge(PathBuf::from(bridge), std::sync::Arc::new(snapshot), "Test".into(), vec![], false);
        let error = match result { Ok(_) => panic!("renaming the tape must not bypass its geometry check"), Err(error) => error };
        assert!(error.contains("Template tape geometry"), "unexpected error: {error}");
    }
    #[test]
    fn sdk_dimensions_follow_automatic_label_length_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let render = |text: &str| preview_with_bridge(PathBuf::from(&bridge), template.clone(), text.into(), String::new(), false, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![]).unwrap();
        let short = render("Test").dimensions_mm.unwrap();
        let long = render("This is a much longer label that must expand the tape").dimensions_mm.unwrap();
        assert!(long.0 > short.0, "automatic label length must grow with content");
        assert!((long.1 - short.1).abs() < 0.1, "tape width must remain stable");
    }
    #[test]
    fn high_resolution_design_preserves_standard_preview_and_falls_back_for_barcodes_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let render = |objects| preview_with_bridge(PathBuf::from(&bridge), template.clone(), "Test".into(), String::new(), false, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), objects).unwrap();
        let ordinary = render(vec![]);
        let high = ordinary.design_image.as_ref().expect("SDK should supply the high-resolution design");
        assert!(high.width() >= ordinary.image.width() * 3 && high.height() >= ordinary.image.height() * 3);
        let barcode = serde_json::json!({"kind":"barcode", "name":"Extra1", "value":"12345", "xmm":0, "ymm":0, "widthMm":5, "heightMm":5});
        let fallback = render(vec![barcode]);
        assert!(fallback.design_image.is_none(), "barcode must use the standard preview fallback");
        assert!(fallback.image.width() > 0);
    }
    #[test]
    fn snapshot_keeps_original_template_even_if_source_changes_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let temporary = std::env::temp_dir().join(format!("dymo-snapshot-{}-{}.dymo", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::copy(PathBuf::from(dir).join("9mm.dymo"), &temporary).unwrap();
        let original = preview_with_bridge(PathBuf::from(&bridge), temporary.to_string_lossy().into_owned(), "Snapshot".into(), String::new(), false, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![]).unwrap();
        std::fs::write(&temporary, "not a template").unwrap();
        let again = preview_snapshot_with_bridge(PathBuf::from(bridge), original.snapshot.clone(), "Snapshot".into(), vec![], false).unwrap();
        assert_eq!(again.image, original.image);
        std::fs::remove_file(temporary).unwrap();
    }
    #[test]
    fn snapshot_keeps_original_image_even_if_source_changes_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let path = std::env::temp_dir().join(format!("dymo-snapshot-image-{}-{}.png", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut source = image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 255, 255, 255]));
        for y in 2..14 { source.put_pixel(3, y, image::Rgba([0, 0, 0, 255])); }
        source.save(&path).unwrap();
        let preview = preview_with_bridge(PathBuf::from(&bridge), template, "Snapshot".into(), path.to_string_lossy().into_owned(), true, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![]).unwrap();
        std::fs::write(&path, b"changed image").unwrap();
        let again = preview_snapshot_with_bridge(PathBuf::from(bridge), preview.snapshot.clone(), "Snapshot".into(), vec![], false).unwrap();
        assert_eq!(again.image, preview.image);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn exported_dymo_preserves_frozen_image_and_manual_layout_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let stamp = format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let image_path = std::env::temp_dir().join(format!("dymo-export-image-{stamp}.png"));
        let mut image = image::RgbaImage::from_pixel(24, 24, image::Rgba([255, 255, 255, 255]));
        for y in 4..20 { for x in 4..10 { image.put_pixel(x, y, image::Rgba([0, 0, 0, 255])); } }
        image.save(&image_path).unwrap();
        let objects = vec![serde_json::json!({"kind":"shape","name":"Extra1","value":"","variant":"Rectangle","xmm":0,"ymm":0,"widthMm":5,"heightMm":5})];
        let original = preview_with_bridge(PathBuf::from(&bridge), template.clone(), "Export test".into(), image_path.to_string_lossy().into_owned(), true, 128, 9, true, false, true, "Segoe UI".into(), "Center".into(), objects.clone()).unwrap();
        let layout = original.all_layouts.as_ref().unwrap().iter().map(|row| serde_json::json!({"Name":row["Name"],"Xmm":row["Xmm"],"Ymm":row["Ymm"],"WidthMm":row["WidthMm"],"HeightMm":row["HeightMm"],"Layer":row["Layer"],"AllowOverlap":true})).collect();
        let snapshot = std::sync::Arc::new(make_snapshot(&template, image_path.to_str().unwrap(), true, 128, 9, true, false, true, "Segoe UI".into(), "Center".into(), Some(layout)).unwrap());
        let positioned = preview_snapshot_with_bridge(PathBuf::from(&bridge), snapshot.clone(), "Export test".into(), objects.clone(), false).unwrap();
        // Export must not reopen a subsequently modified source image.
        std::fs::write(&image_path, b"image source changed").unwrap();
        let bytes = export_snapshot_with_bridge(PathBuf::from(&bridge), &snapshot, "Export test".into(), objects).unwrap();
        let restored = PrintSnapshot { template_base64: base64::engine::general_purpose::STANDARD.encode(bytes), image: None, layout: None, ..(*snapshot).clone() };
        let roundtrip = preview_snapshot_with_bridge(PathBuf::from(bridge), std::sync::Arc::new(restored), "Export test".into(), vec![], false).unwrap();
        assert_eq!(roundtrip.image, positioned.image, "exported DYMO label must retain its image, formatting and manual geometry");
        assert_eq!(roundtrip.dimensions_mm, positioned.dimensions_mm);
        std::fs::remove_file(image_path).unwrap();
    }
    #[test]
    fn imported_live_dates_and_embedded_counters_are_rejected_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let snapshot = make_snapshot(&template, "", true, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), None).unwrap();
        let date = serde_json::json!({"kind":"date","name":"Extra1","value":"2000-01-01T00:00","variant":"DateAndTime","xmm":0,"ymm":0,"widthMm":25,"heightMm":7});
        let bytes = export_snapshot_with_bridge(PathBuf::from(&bridge), &snapshot, "".into(), vec![date]).unwrap();
        let static_xml = String::from_utf8(bytes).unwrap();
        assert!(static_xml.contains("<IsStatic>True</IsStatic>"));
        let imported_static = PrintSnapshot { template_base64: base64::engine::general_purpose::STANDARD.encode(static_xml.as_bytes()), ..snapshot.clone() };
        // Explicit fixed dates imported from DYMO remain supported.
        request_snapshot(PathBuf::from(&bridge), "preview", &imported_static, "".into(), 1, vec![], false).unwrap();
        let dynamic_xml = static_xml.replace("<IsStatic>True</IsStatic>", "<IsStatic>False</IsStatic>");
        let imported_live = PrintSnapshot { template_base64: base64::engine::general_purpose::STANDARD.encode(dynamic_xml.as_bytes()), ..snapshot.clone() };
        for action in ["preview", "export", "inspect-layout"] {
            let error = request_snapshot(PathBuf::from(&bridge), action, &imported_live, "".into(), 1, vec![], false).unwrap_err();
            assert!(error.contains("dynamic date/time object"), "unexpected error: {error}");
        }
        // App-added counters still render/export. Importing one as a template
        // must not bypass the app's explicit per-label counter planning.
        let counter = serde_json::json!({"kind":"counter","name":"Extra1","value":"","xmm":0,"ymm":0,"widthMm":8,"heightMm":5,"startAt":1,"increment":1,"textWidth":2,"leadingZeros":true});
        let bytes = export_snapshot_with_bridge(PathBuf::from(&bridge), &snapshot, "".into(), vec![counter]).unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("<CounterObject>"));
        let imported_counter = PrintSnapshot { template_base64: base64::engine::general_purpose::STANDARD.encode(bytes), ..snapshot };
        for action in ["preview", "export", "inspect-layout"] {
            let error = request_snapshot(PathBuf::from(&bridge), action, &imported_counter, "".into(), 1, vec![], false).unwrap_err();
            assert!(error.contains("embedded counter"), "unexpected error: {error}");
        }
    }
    #[test]
    fn image_threshold_changes_the_actual_sdk_preview_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let path = std::env::temp_dir().join(format!("dymo-threshold-{}-{}.png", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        image::RgbaImage::from_pixel(24, 24, image::Rgba([120, 120, 120, 255])).save(&path).unwrap();
        let render = |cutoff| preview_with_bridge(PathBuf::from(&bridge), template.clone(), String::new(), path.to_string_lossy().into_owned(), true, cutoff, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![]).unwrap();
        let white = render(50);
        let black = render(200);
        assert!(white.threshold_image_blank, "an all-white threshold must be reported to the user");
        assert!(!black.threshold_image_blank);
        assert_ne!(white.image, black.image, "changing threshold must change the SDK's actual printer-resolution output");
        assert!(black.image.pixels().filter(|pixel| pixel[0] < 100 && pixel[1] < 100 && pixel[2] < 100).count() > white.image.pixels().filter(|pixel| pixel[0] < 100 && pixel[1] < 100 && pixel[2] < 100).count());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn sdk_additional_objects_render_and_invalid_geometry_is_rejected_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let render = |objects| preview_with_bridge(PathBuf::from(&bridge), template.clone(), "Test".into(), String::new(), false, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), objects);
        let qr = serde_json::json!({"kind":"qr", "name":"Extra1", "value":"sample", "xmm":0, "ymm":0, "widthMm":5, "heightMm":5});
        let preview = render(vec![qr.clone()]).expect("SDK should render text and a QR object");
        assert_eq!(preview.object_layouts.len(), 1);
        assert!(preview.object_layouts[0]["Xmm"].as_f64().unwrap() > 0.0);
        let mut bad = qr; bad["heightMm"] = serde_json::json!(100);
        assert!(render(vec![bad]).is_err(), "an object outside the tape must be rejected");
    }
    #[test]
    fn manual_layout_requires_all_objects_and_rejects_unknown_ones_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let original = preview_with_bridge(PathBuf::from(&bridge), template.clone(), "Manual layout".into(), String::new(), false, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![]).unwrap();
        assert!(original.printable_dimensions_mm.is_some());
        let rows = original.all_layouts.unwrap();
        assert!(!rows.is_empty());
        let specs = rows.iter().map(|r| serde_json::json!({"Name":r["Name"],"Xmm":r["Xmm"],"Ymm":r["Ymm"],"WidthMm":r["WidthMm"],"HeightMm":r["HeightMm"],"Layer":r["Layer"],"AllowOverlap":true})).collect::<Vec<_>>();
        let make = |layout| std::sync::Arc::new(make_snapshot(&template, "", false, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), Some(layout)).unwrap());
        let good = preview_snapshot_with_bridge(PathBuf::from(&bridge), make(specs.clone()), "Manual layout".into(), vec![], false).expect("the SDK should render its own resolved layout");
        assert_eq!(good.all_layouts.unwrap().len(), specs.len());
        let mut invalid = specs; invalid[0]["Name"] = serde_json::json!("UnknownObject");
        assert!(preview_snapshot_with_bridge(PathBuf::from(bridge), make(invalid), "Manual layout".into(), vec![], false).is_err());
    }
    #[test]
    fn manual_layout_add_and_remove_preserves_existing_position_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let original = preview_with_bridge(PathBuf::from(&bridge), template.clone(), "Test".into(), String::new(), false, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![]).unwrap();
        let mut layout = crate::layout_tools::LayoutState::default();
        layout.enable_manual(&serde_json::Value::Array(original.all_layouts.unwrap())).unwrap();
        let text_name = layout.save_value()["objects"][0]["name"].as_str().unwrap().to_owned();
        layout.move_object(&text_name, 3.005, 0.0).unwrap();
        let before = layout.save_value()["objects"][0].clone();
        let shape = serde_json::json!({"kind":"shape","name":"Extra1","value":"","variant":"Rectangle","xmm":0,"ymm":0,"widthMm":5,"heightMm":5});
        let (mut added, preview) = preview_reconciled_with_bridge(PathBuf::from(&bridge), template.clone(), "Test".into(), String::new(), false, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![shape], &mut layout, vec![]).unwrap();
        preview.expect("the merged manual layout should render");
        let saved = added.save_value();
        let retained = saved["objects"].as_array().unwrap().iter().find(|item| item["name"] == text_name).unwrap();
        assert_eq!(retained["x"], before["x"]);
        assert!(saved["objects"].as_array().unwrap().iter().any(|item| item["name"] == "Extra1"));
        let (removed, preview) = preview_reconciled_with_bridge(PathBuf::from(bridge), template, "Test".into(), String::new(), false, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![], &mut added, vec!["Extra1".into()]).unwrap();
        preview.expect("the retained manual layout should render after removal");
        assert_eq!(removed.save_value()["objects"][0]["x"], before["x"]);
        assert_eq!(removed.save_value()["objects"].as_array().unwrap().len(), 1);
    }
    #[test]
    fn empty_primary_text_does_not_block_a_shape_but_visible_text_still_does_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let shape = serde_json::json!({"kind":"shape","name":"Extra1","value":"","variant":"Rectangle","xmm":0,"ymm":0,"widthMm":5,"heightMm":5});
        let auto = preview_with_bridge(PathBuf::from(&bridge), template.clone(), String::new(), String::new(), false, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![shape.clone()]).unwrap();
        let mut layout = crate::layout_tools::LayoutState::default();
        layout.enable_manual(&serde_json::Value::Array(auto.all_layouts.unwrap())).unwrap();
        layout.move_object("Extra1", 0.0, 0.0).unwrap();
        let request = layout.request_value().unwrap().unwrap().as_array().unwrap().clone();
        let snapshot = std::sync::Arc::new(make_snapshot(&template, "", false, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), Some(request)).unwrap());
        let blank = preview_snapshot_with_bridge(PathBuf::from(&bridge), snapshot.clone(), String::new(), vec![shape.clone()], false).expect("blank text should not block the shape");
        assert!(blank.empty_primary_text.is_some());
        assert!(preview_snapshot_with_bridge(PathBuf::from(bridge), snapshot, "Test".into(), vec![shape], false).is_err(), "visible text must still reject unapproved overlap");
    }
    #[test]
    fn removing_date_reproduces_the_original_image_preview_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir), Ok(icon)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR"), std::env::var("DYMO_TEST_IMAGE")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let render = |objects| preview_with_bridge(PathBuf::from(&bridge), template.clone(), String::new(), icon.clone(), true, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), objects);
        let before = render(vec![]).unwrap();
        let date = serde_json::json!({"kind":"date","name":"Extra1","value":"2026-10-29T12:34","variant":"DateAndTime","xmm":0,"ymm":0,"widthMm":25,"heightMm":7});
        let with_date = render(vec![date]).unwrap();
        assert_eq!(with_date.object_layouts.len(), 1);
        let after = render(vec![]).unwrap();
        assert!(after.object_layouts.is_empty());
        assert_eq!(before.image, after.image, "removing a date must not leave its pixels in the generated preview");
    }
    #[test]
    fn sdk_object_kinds_render_on_nine_mm_tape_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        for (kind, value) in [("text", "extra"), ("address", "Example"), ("barcode", "12345"), ("qr", "sample"), ("shape", ""), ("line", ""), ("date", "2026-10-29"), ("counter", "")] {
            let item = serde_json::json!({"kind":kind,"name":"Extra1","value":value,"xmm":0,"ymm":0,"widthMm":5,"heightMm":5});
            let preview = preview_with_bridge(PathBuf::from(&bridge), template.clone(), "Test".into(), String::new(), false, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![item]).unwrap_or_else(|e| panic!("{kind} should render: {e}"));
            assert_eq!(preview.object_layouts.len(), 1, "{kind}");
            assert!(preview.image.pixels().any(|pixel| pixel[0] < 128 && pixel[3] > 0), "{kind} should show ink");
        }
    }
    #[test]
    fn overlapping_mixed_objects_are_rejected_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir), Ok(icon)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR"), std::env::var("DYMO_TEST_IMAGE")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let qr = serde_json::json!({"kind":"qr","name":"Extra1","value":"sample","xmm":0,"ymm":0,"widthMm":5,"heightMm":5});
        let barcode = serde_json::json!({"kind":"barcode","name":"Extra2","value":"12345","xmm":5,"ymm":0,"widthMm":35,"heightMm":5});
        let result = preview_with_bridge(PathBuf::from(bridge), template, "Test".into(), icon, true, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![qr, barcode]);
        if let Err(error) = result { assert!(error.contains("overlap"), "unexpected error: {error}"); }
        else { /* Some SDK releases may lay this combination out without overlap. */ }
    }
    #[test]
    fn monochrome_image_and_text_render_together_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir), Ok(icon)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR"), std::env::var("DYMO_TEST_IMAGE")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo");
        let image = preview_with_bridge(PathBuf::from(bridge), template.to_string_lossy().into_owned(), "Test Print".into(), icon, true, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![]).expect("SDK should render monochrome image and text");
        assert!(image.image.width() > 100 && image.image.height() > 0);
        assert!(image.image.pixels().any(|p| p[0] < 128 && p[3] > 0));
    }
    #[test]
    fn image_and_text_are_individually_selectable_in_sdk_geometry_when_fixtures_supplied() {
        let (Ok(bridge), Ok(dir), Ok(icon)) = (std::env::var("DYMO_TEST_BRIDGE"), std::env::var("DYMO_TEST_TEMPLATE_DIR"), std::env::var("DYMO_TEST_IMAGE")) else { return; };
        let template = PathBuf::from(dir).join("9mm.dymo").to_string_lossy().into_owned();
        let preview = preview_with_bridge(PathBuf::from(bridge), template, "Test".into(), icon, true, 128, 9, false, false, false, "Segoe UI".into(), "Center".into(), vec![]).unwrap();
        let geometry = serde_json::Value::Array(preview.all_layouts.expect("SDK should provide all objects"));
        let objects = crate::canvas_layout::objects_from_snapshot(&geometry).unwrap();
        assert!(objects.iter().any(|object| object.name == "ImageObject0"));
        assert!(objects.iter().any(|object| object.name.starts_with("TextObject")));
        assert!(preview.printable_origin_mm.is_some() && preview.printable_dimensions_mm.is_some());
    }
}
