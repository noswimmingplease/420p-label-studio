//! UI state for optional DYMO SDK label objects.
//!
//! Objects are arranged by DYMO's continuous tape layout. The positions sent
//! to the bridge are requested positions, not a promise of free positioning;
//! the bridge must render and report the resolved bounds before printing.
use eframe::egui::{self, RichText};
use serde_json::{Value, json};

const MAX_OBJECTS: usize = 50;
const MAX_LABEL_MM: f64 = 2000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureKind { Text, Address, Barcode, Qr, Shape, Line, Date, Counter }

impl FeatureKind {
    pub const ALL: [Self; 8] = [Self::Text, Self::Address, Self::Barcode, Self::Qr, Self::Shape, Self::Line, Self::Date, Self::Counter];
    pub fn label(self) -> &'static str { match self {
        Self::Text => "Text", Self::Address => "Address", Self::Barcode => "Barcode", Self::Qr => "QR code",
        Self::Shape => "Shape", Self::Line => "Line", Self::Date => "Date and time", Self::Counter => "Counter",
    }}
    pub fn wire(self) -> &'static str { match self {
        Self::Text => "text", Self::Address => "address", Self::Barcode => "barcode", Self::Qr => "qr",
        Self::Shape => "shape", Self::Line => "line", Self::Date => "date", Self::Counter => "counter",
    }}
    fn parse(value: &str) -> Option<Self> { Self::ALL.into_iter().find(|kind| kind.wire() == value) }
}

#[derive(Clone, Debug, PartialEq)]
struct Feature {
    kind: FeatureKind,
    name: String,
    value: String,
    variant: String,
    start_at: i32,
    increment: i32,
    text_width: i32,
    leading_zeros: bool,
    capture_at_preview: bool,
}

/// The caller should retain this with the label document and include
/// `request_value` in the preview/print request and saved project.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Features {
    objects: Vec<Feature>,
    selected: Option<usize>,
    next_id: u32,
}

fn printable_height(width_mm: u8) -> Option<f64> {
    // Conservatively rounded down from the bundled templates' DYMORect height.
    // Custom templates may differ and are checked by the SDK bridge as well.
    match width_mm { 6 => Some(4.20), 9 => Some(7.00), 12 => Some(8.10), 19 => Some(14.90), _ => None }
}

// Retail codes include their check digit, which the SDK itself does not validate.
fn barcode_format(variant: &str) -> Option<&'static str> {
    match variant {
        "" | "Code128" => Some("Code128"), "Code39" => Some("Code39"), "UpcE" => Some("UpcE"),
        "Ean13" => Some("Ean13"), "Ean8" => Some("Ean8"), "UpcA" => Some("UpcA"), _ => None,
    }
}

fn valid_retail_check_digit(bytes: &[u8]) -> bool {
    if bytes.len() < 2 || !bytes.iter().all(u8::is_ascii_digit) { return false; }
    let sum: u32 = bytes[..bytes.len() - 1].iter().rev().enumerate()
        .map(|(i, b)| u32::from(*b - b'0') * if i % 2 == 0 { 3 } else { 1 }).sum();
    (10 - sum % 10) % 10 == u32::from(bytes[bytes.len() - 1] - b'0')
}

fn valid_upce(bytes: &[u8]) -> bool {
    if bytes.len() != 8 || !bytes.iter().all(u8::is_ascii_digit) || !matches!(bytes[0], b'0' | b'1') { return false; }
    let d = &bytes[1..7];
    let mut expanded = Vec::with_capacity(12);
    expanded.push(bytes[0]);
    match d[5] {
        b'0'..=b'2' => expanded.extend_from_slice(&[d[0], d[1], d[5], b'0', b'0', b'0', b'0', d[2], d[3], d[4]]),
        b'3' => expanded.extend_from_slice(&[d[0], d[1], d[2], b'0', b'0', b'0', b'0', b'0', d[3], d[4]]),
        b'4' => expanded.extend_from_slice(&[d[0], d[1], d[2], d[3], b'0', b'0', b'0', b'0', b'0', d[4]]),
        _ => expanded.extend_from_slice(&[d[0], d[1], d[2], d[3], d[4], b'0', b'0', b'0', b'0', d[5]]),
    }
    expanded.push(bytes[7]);
    valid_retail_check_digit(&expanded)
}

fn valid_date(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' ||
        bytes.iter().enumerate().any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit()) { return false; }
    let Ok(y) = s[0..4].parse::<u32>() else { return false; };
    let Ok(m) = s[5..7].parse::<u32>() else { return false; };
    let Ok(d) = s[8..10].parse::<u32>() else { return false; };
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let last = match m { 1 | 3 | 5 | 7 | 8 | 10 | 12 => 31, 4 | 6 | 9 | 11 => 30, 2 if leap => 29, 2 => 28, _ => return false };
    y > 0 && d >= 1 && d <= last
}

fn valid_time(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 5 || bytes[2] != b':' || ![0, 1, 3, 4].into_iter().all(|i| bytes[i].is_ascii_digit()) { return false; }
    let hour = u32::from(bytes[0] - b'0') * 10 + u32::from(bytes[1] - b'0');
    let minute = u32::from(bytes[3] - b'0') * 10 + u32::from(bytes[4] - b'0');
    hour <= 23 && minute <= 59
}

fn valid_datetime(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 16 && bytes[10] == b'T' && s.is_ascii() && valid_date(&s[..10]) && valid_time(&s[11..])
}

fn date_format(variant: &str) -> Option<&'static str> {
    match variant { "" | "Date" => Some("Date"), "Time" => Some("Time"), "DateAndTime" => Some("DateAndTime"), _ => None }
}

#[cfg(target_os = "windows")]
fn local_datetime() -> String {
    #[repr(C)] struct SystemTime { year: u16, month: u16, weekday: u16, day: u16, hour: u16, minute: u16, second: u16, millis: u16 }
    #[link(name = "kernel32")] unsafe extern "system" { fn GetLocalTime(time: *mut SystemTime); }
    let mut time = SystemTime { year: 0, month: 0, weekday: 0, day: 0, hour: 0, minute: 0, second: 0, millis: 0 };
    // SAFETY: GetLocalTime writes exactly one SYSTEMTIME to this valid pointer.
    unsafe { GetLocalTime(&mut time); }
    format!("{:04}-{:02}-{:02}T{:02}:{:02}", time.year, time.month, time.day, time.hour, time.minute)
}
#[cfg(not(target_os = "windows"))]
fn local_datetime() -> String { String::new() }

impl Feature {
    fn new(kind: FeatureKind, id: u32) -> Self {
        Self {
            kind, name: format!("Extra{id}"),
            value: if kind == FeatureKind::Date { local_datetime().chars().take(10).collect() } else { String::new() },
            variant: match kind { FeatureKind::Shape => "Rectangle", FeatureKind::Line => "Horizontal", FeatureKind::Date => "Date", _ => "" }.into(),
            start_at: 1, increment: 1, text_width: 1, leading_zeros: false, capture_at_preview: kind == FeatureKind::Date,
        }
    }
    fn width_mm(&self, height: f64) -> f64 { match self.kind {
        FeatureKind::Qr | FeatureKind::Shape | FeatureKind::Line => height,
        FeatureKind::Barcode => 35.0,
        FeatureKind::Counter => 15.0,
        _ => 25.0,
    }}
    fn validate(&self) -> Result<(), String> {
        let bad = match self.kind {
            FeatureKind::Text | FeatureKind::Address => {
                if self.value.trim().is_empty() { Some("Enter text for the object.") }
                else if self.value.encode_utf16().count() > 500 { Some("Text must be 500 characters or fewer.") } else { None }
            }
            FeatureKind::Barcode => match barcode_format(&self.variant) {
                Some("Code128") if !self.value.is_empty() && self.value.len() <= 100 && self.value.bytes().all(|b| (32..=126).contains(&b)) => None,
                Some("Code128") => Some("Code 128 needs 1–100 printable ASCII characters."),
                Some("Code39") if !self.value.is_empty() && self.value.len() <= 100 && self.value.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b" -.$/+%".contains(&b)) => None,
                Some("Code39") => Some("Code 39 needs 1–100 uppercase letters, digits or - . $ / + % and space."),
                Some("UpcE") if valid_upce(self.value.as_bytes()) => None,
                Some("UpcE") => Some("UPC-E needs 8 digits including number system 0 or 1 and a valid check digit."),
                Some(format @ ("Ean13" | "Ean8" | "UpcA")) => {
                    let length = match format { "Ean13" => 13, "Ean8" => 8, _ => 12 };
                    if self.value.len() == length && valid_retail_check_digit(self.value.as_bytes()) { None }
                    else { Some("Enter the complete retail code with a valid check digit (EAN-13: 13, EAN-8: 8, UPC-A: 12 digits).") }
                }
                _ => Some("Choose a supported barcode format."),
            },
            FeatureKind::Qr => {
                if self.value.is_empty() || self.value.encode_utf16().count() > 512 || self.value.chars().any(char::is_control) {
                    Some("QR content needs 1–512 characters without control characters.")
                } else { None }
            }
            FeatureKind::Shape => if ["Rectangle", "RoundedRectangle", "Ellipse", "Triangle", "RightTriangle", "Diamond"].contains(&self.variant.as_str()) { None } else { Some("Choose a supported shape.") },
            FeatureKind::Line => if ["Horizontal", "Vertical", "ForwardDiagonal", "BackwardDiagonal"].contains(&self.variant.as_str()) { None } else { Some("Choose a supported line.") },
            FeatureKind::Date => match date_format(&self.variant) {
                None => Some("Choose date, time or date and time."),
                Some(_) if self.capture_at_preview => None,
                Some("Date") if valid_date(&self.value) => None,
                Some("Time") if valid_time(&self.value) => None,
                Some("DateAndTime") if valid_datetime(&self.value) => None,
                Some("Date") => Some("Enter a real date as YYYY-MM-DD."),
                Some("Time") => Some("Enter a real time as HH:MM (24-hour)."),
                _ => Some("Enter a real date and time as YYYY-MM-DDTHH:MM."),
            },
            FeatureKind::Counter => if (0..=999_999_999).contains(&self.start_at) && (1..=1_000_000).contains(&self.increment) && (1..=9).contains(&self.text_width) { None } else { Some("Counter values are outside the supported range.") },
        };
        bad.map_or(Ok(()), |message| Err(format!("{}: {message}", self.kind.label())))
    }
}

impl Features {
    pub fn is_empty(&self) -> bool { self.objects.is_empty() }
    pub fn names(&self) -> Vec<String> { self.objects.iter().map(|object| object.name.clone()).collect() }
    pub(crate) fn remove_by_name(&mut self, name: &str) -> bool {
        let Some(index) = self.objects.iter().position(|object| object.name.eq_ignore_ascii_case(name)) else { return false; };
        self.objects.remove(index);
        self.selected = if self.objects.is_empty() { None } else { Some(index.min(self.objects.len() - 1)) };
        true
    }
    pub fn select_by_name(&mut self, name: &str) -> bool {
        if let Some(index) = self.objects.iter().position(|object| object.name.eq_ignore_ascii_case(name)) {
            self.selected = Some(index);
            true
        } else { false }
    }
    /// Do not reuse an identity still present in a saved manual layout or a
    /// pending deletion, even if that feature has already been removed.
    pub fn reserve_layout_names(&mut self, names: impl IntoIterator<Item = String>) {
        for name in names {
            if let Some(id) = name.strip_prefix("Extra").and_then(|s| s.parse::<u32>().ok()).filter(|n| format!("Extra{n}") == name) {
                self.next_id = self.next_id.max(id);
            }
        }
    }
    pub fn has_counter(&self) -> bool { self.objects.iter().any(|o| o.kind == FeatureKind::Counter) }
    pub fn has_preview_captured_datetime(&self) -> bool { self.objects.iter().any(|o| o.kind == FeatureKind::Date && o.capture_at_preview) }
    pub fn add(&mut self, kind: FeatureKind, tape_width_mm: u8) -> Result<(), String> {
        if printable_height(tape_width_mm).is_none() { return Err("Select a supported tape width first.".into()); }
        if self.objects.len() >= MAX_OBJECTS { return Err("A label can have at most 50 additional objects.".into()); }
        self.next_id = self.next_id.checked_add(1).ok_or("No more object names are available.")?;
        self.objects.push(Feature::new(kind, self.next_id));
        self.selected = Some(self.objects.len() - 1);
        Ok(())
    }
    pub fn validate(&self, tape_width_mm: u8) -> Result<(), String> {
        let height = printable_height(tape_width_mm).ok_or("Select a supported tape width.")?;
        if self.objects.len() > MAX_OBJECTS { return Err("Too many objects.".into()); }
        let mut length = 0.0;
        let mut seen = std::collections::HashSet::new();
        for object in &self.objects {
            if !seen.insert(object.name.to_ascii_lowercase()) { return Err("Duplicate object names.".into()); }
            object.validate()?;
            length += object.width_mm(height);
            if length > MAX_LABEL_MM { return Err("The requested object layout is too long.".into()); }
        }
        Ok(())
    }
    /// JSON array accepted by the bridge's FeatureObject records. Actual bounds
    /// can change during SDK render and must be reported by the caller. Captured
    /// dates are refused here so printing cannot silently capture a new time.
    #[cfg(test)]
    pub fn request_value(&self, tape_width_mm: u8) -> Result<Value, String> {
        self.request_value_with_capture(tape_width_mm, None)
    }

    /// Capture all automatic date/time objects from one local-clock reading.
    /// The caller must retain the returned array after successful preview and
    /// reuse exactly that array for printing and batch previews/print jobs.
    pub fn capture_request(&self, tape_width_mm: u8) -> Result<Value, String> {
        let timestamp = if self.has_preview_captured_datetime() { Some(local_datetime()) } else { None };
        self.request_value_with_capture(tape_width_mm, timestamp.as_deref())
    }

    fn request_value_with_capture(&self, tape_width_mm: u8, captured: Option<&str>) -> Result<Value, String> {
        self.validate(tape_width_mm)?;
        if self.has_preview_captured_datetime() && !captured.is_some_and(valid_datetime) {
            return Err("Generate a new preview to capture the date and time before printing.".into());
        }
        let height = printable_height(tape_width_mm).unwrap();
        let mut x = 0.0;
        let mut items = Vec::with_capacity(self.objects.len());
        for object in &self.objects {
            let width = object.width_mm(height);
            let value = if object.kind == FeatureKind::Date && object.capture_at_preview {
                let timestamp = captured.expect("timestamp validated above");
                match date_format(&object.variant).expect("date format validated above") {
                    "Date" => &timestamp[..10],
                    "Time" => &timestamp[11..],
                    _ => timestamp,
                }
            } else { object.value.as_str() };
            items.push(json!({
                "kind": object.kind.wire(), "name": object.name,
                "value": if value.is_empty() { None } else { Some(value) },
                "xmm": x, "ymm": 0.0, "widthMm": width, "heightMm": height,
                "variant": if object.variant.is_empty() { None } else { Some(&object.variant) },
                "startAt": object.start_at, "increment": object.increment, "textWidth": object.text_width, "leadingZeros": object.leading_zeros,
            }));
            x += width;
        }
        Ok(Value::Array(items))
    }
    /// Store editable state separately from the SDK's resolved geometry.
    pub fn save_value(&self) -> Value {
        Value::Array(self.objects.iter().map(|o| json!({"kind":o.kind.wire(),"name":o.name,"value":o.value,"variant":o.variant,"startAt":o.start_at,"increment":o.increment,"textWidth":o.text_width,"leadingZeros":o.leading_zeros,"captureAtPreview":o.capture_at_preview})).collect())
    }
    pub fn load_value(value: &Value, tape_width_mm: u8) -> Result<Self, String> {
        let list = value.as_array().ok_or("Invalid objects list.")?;
        if list.len() > MAX_OBJECTS { return Err("Too many objects.".into()); }
        let mut result = Self::default();
        let mut reserved = std::collections::HashSet::new();
        for item in list {
            if let Some(value) = item.get("name") {
                let name = value.as_str().ok_or("Invalid object name.")?;
                let number = name.strip_prefix("Extra").and_then(|s| s.parse::<u32>().ok()).filter(|n| *n > 0).ok_or("Invalid object name.")?;
                if format!("Extra{number}") != name || !reserved.insert(number) { return Err("Object names must be unique and valid.".into()); }
            }
        }
        for item in list {
            let kind = FeatureKind::parse(item["kind"].as_str().ok_or("Object type missing.")?).ok_or("Unsupported object type.")?;
            let id = if let Some(name) = item.get("name").and_then(Value::as_str) {
                name.strip_prefix("Extra").and_then(|s| s.parse::<u32>().ok()).ok_or("Invalid object name.")?
            } else {
                let mut id = 1_u32;
                while reserved.contains(&id) { id = id.checked_add(1).ok_or("Too many objects.")?; }
                reserved.insert(id); id
            };
            result.next_id = result.next_id.max(id);
            let mut object = Feature::new(kind, id);
            object.value = match item.get("value") { None => String::new(), Some(v) => v.as_str().ok_or("Invalid object text.")?.to_owned() };
            if object.value.len() > 16_384 { return Err("Object text is too large.".into()); }
            object.variant = match item.get("variant") { None => object.variant, Some(v) => v.as_str().filter(|s| s.len() <= 64).ok_or("Invalid object variant.")?.to_owned() };
            let number = |key: &str, default: i32| -> Result<i32, String> {
                match item.get(key) { None => Ok(default), Some(v) => v.as_i64().and_then(|n| i32::try_from(n).ok()).ok_or_else(|| format!("Invalid {key}.")) }
            };
            object.start_at = number("startAt", 1)?; object.increment = number("increment", 1)?; object.text_width = number("textWidth", 1)?;
            object.leading_zeros = match item.get("leadingZeros") { None => false, Some(v) => v.as_bool().ok_or("Invalid counter setting.")? };
            // Before this option existed dates were always fixed; do not turn
            // old saved projects into moving timestamps when they are opened.
            object.capture_at_preview = match item.get("captureAtPreview") { None => false, Some(v) => v.as_bool().ok_or("Invalid date/time capture setting.")? };
            if kind != FeatureKind::Date && object.capture_at_preview { return Err("Date/time capture is only valid for date objects.".into()); }
            result.objects.push(object);
        }
        if printable_height(tape_width_mm).is_none() { return Err("Select a supported tape width.".into()); }
        Ok(result)
    }
    /// Adds an object list and an editor for the selected object. The returned
    /// flag indicates a document change and should invalidate the preview.
    pub fn ui(&mut self, ui: &mut egui::Ui, tape_width_mm: u8) -> bool {
        let mut changed = false;
        if self.objects.is_empty() { ui.label("No additional objects."); return false; }
        ui.label(RichText::new("Objects follow DYMO’s tape layout; positions and sizes may change.").small());
        for (index, object) in self.objects.iter().enumerate() {
            if ui.selectable_label(self.selected == Some(index), format!("{} · {}", index + 1, object.kind.label())).clicked() { self.selected = Some(index); }
        }
        let Some(index) = self.selected.filter(|index| *index < self.objects.len()) else { return false; };
        ui.separator();
        let obj = &mut self.objects[index];
        match obj.kind {
            FeatureKind::Text | FeatureKind::Address => { ui.label(if obj.kind == FeatureKind::Address { "Address text" } else { "Text" }); changed |= ui.text_edit_multiline(&mut obj.value).changed(); }
            FeatureKind::Barcode => {
                let formats = [("Code128", "Code 128"), ("Code39", "Code 39"), ("UpcE", "UPC-E"), ("Ean13", "EAN-13"), ("Ean8", "EAN-8"), ("UpcA", "UPC-A")];
                let current = barcode_format(&obj.variant).and_then(|v| formats.iter().find(|(value, _)| *value == v)).map(|(_, display)| *display).unwrap_or("Unsupported");
                ui.label("Barcode format");
                egui::ComboBox::from_id_salt("barcode-format").selected_text(current).show_ui(ui, |ui| {
                    for (value, display) in formats { changed |= ui.selectable_value(&mut obj.variant, value.to_owned(), display).changed(); }
                });
                ui.label("Barcode data");
                changed |= ui.text_edit_singleline(&mut obj.value).changed();
                ui.label(RichText::new("Retail codes must include the check digit. Check the printed barcode with a scanner.").small());
            }
            FeatureKind::Qr => { ui.label("QR content"); changed |= ui.text_edit_multiline(&mut obj.value).changed(); ui.label(RichText::new("A small tape may produce an unreadable QR code; check with a scanner.").small()); }
            FeatureKind::Date => {
                let current = match date_format(&obj.variant) { Some("Time") => "Time", Some("DateAndTime") => "Date and time", _ => "Date" };
                egui::ComboBox::from_id_salt("date-format").selected_text(current).show_ui(ui, |ui| {
                    for (value, display) in [("Date", "Date"), ("Time", "Time"), ("DateAndTime", "Date and time")] {
                        changed |= ui.selectable_value(&mut obj.variant, value.to_owned(), display).changed();
                    }
                });
                changed |= ui.checkbox(&mut obj.capture_at_preview, "Capture when preview is generated").changed();
                if obj.capture_at_preview {
                    ui.label(RichText::new("The local date/time is frozen at preview and reused when printing. Refresh the preview to update it.").small());
                } else {
                    let hint = match date_format(&obj.variant) { Some("Time") => "HH:MM (24-hour)", Some("DateAndTime") => "YYYY-MM-DDTHH:MM", _ => "YYYY-MM-DD" };
                    ui.label(format!("Fixed value ({hint})"));
                    changed |= ui.text_edit_singleline(&mut obj.value).changed();
                }
            }
            FeatureKind::Counter => {
                ui.horizontal(|ui| { ui.label("Start"); changed |= ui.add(egui::DragValue::new(&mut obj.start_at).range(0..=999_999_999)).changed(); });
                ui.horizontal(|ui| { ui.label("Increment"); changed |= ui.add(egui::DragValue::new(&mut obj.increment).range(1..=1_000_000)).changed(); });
                ui.horizontal(|ui| { ui.label("Digits"); changed |= ui.add(egui::DragValue::new(&mut obj.text_width).range(1..=9)).changed(); });
                changed |= ui.checkbox(&mut obj.leading_zeros, "Leading zeros").changed();
                ui.label(RichText::new("The copies setting determines how many numbers are printed.").small());
            }
            FeatureKind::Shape | FeatureKind::Line => {
                let variants: &[&str] = if obj.kind == FeatureKind::Shape { &["Rectangle", "RoundedRectangle", "Ellipse", "Triangle", "RightTriangle", "Diamond"] } else { &["Horizontal", "Vertical", "ForwardDiagonal", "BackwardDiagonal"] };
                egui::ComboBox::from_id_salt("feature-variant").selected_text(&obj.variant).show_ui(ui, |ui| { for variant in variants { changed |= ui.selectable_value(&mut obj.variant, (*variant).to_owned(), *variant).changed(); } });
            }
        }
        if let Err(error) = obj.validate() { ui.colored_label(egui::Color32::from_rgb(235, 160, 100), error); }
        if ui.button("Remove object").clicked() {
            self.objects.remove(index);
            self.selected = if self.objects.is_empty() { None } else { Some(index.min(self.objects.len() - 1)) };
            changed = true;
        }
        if printable_height(tape_width_mm).is_none() { ui.label("Select a supported tape width."); }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn saved_names_remain_stable_after_deletion() {
        let input = json!([{"kind":"shape","name":"Extra2","value":"","variant":"Rectangle"}]);
        let mut loaded = Features::load_value(&input, 9).unwrap();
        assert_eq!(loaded.save_value()[0]["name"], "Extra2");
        loaded.add(FeatureKind::Line, 9).unwrap();
        assert_eq!(loaded.save_value()[1]["name"], "Extra3");
        assert!(Features::load_value(&json!([{"kind":"line","name":"Extra2"},{"kind":"line","name":"Extra2"}]), 9).is_err());
    }
    #[test] fn validates_dates_including_leap_years() {
        assert!(valid_date("2024-02-29")); assert!(!valid_date("2025-02-29"));
        assert!(!valid_date("0000-01-01")); assert!(!valid_date("2026-13-01"));
        assert!(!valid_date("2026-01-1"));
        assert!(valid_time("00:00")); assert!(valid_time("23:59")); assert!(!valid_time("24:00")); assert!(!valid_time("12:60"));
        assert!(valid_datetime("2024-02-29T23:59")); assert!(!valid_datetime("2025-02-29T23:59")); assert!(!valid_datetime("2024-02-29T24:00"));
        assert!(!valid_datetime("2024-02-29 12:00")); assert!(!valid_datetime("2024-02-29T1:00"));
    }
    #[test] fn preserves_legacy_fixed_date_projects() {
        let old = json!([{"kind":"date", "value":"2020-02-29", "variant":""}]);
        let restored = Features::load_value(&old, 9).unwrap();
        assert!(!restored.has_preview_captured_datetime());
        assert_eq!(restored.request_value(9).unwrap()[0]["value"], "2020-02-29");
        let saved = restored.save_value();
        let reopened = Features::load_value(&saved, 9).unwrap();
        assert_eq!(reopened.request_value(9).unwrap()[0]["value"], "2020-02-29");
    }
    #[test] fn captures_one_timestamp_for_all_formats_and_never_resamples_for_print() {
        let input = json!([
            {"kind":"date", "value":"", "variant":"Date", "captureAtPreview":true},
            {"kind":"date", "value":"", "variant":"Time", "captureAtPreview":true},
            {"kind":"date", "value":"", "variant":"DateAndTime", "captureAtPreview":true},
            {"kind":"date", "value":"1999-12-31", "variant":"Date", "captureAtPreview":false}
        ]);
        let features = Features::load_value(&input, 9).unwrap();
        assert!(features.has_preview_captured_datetime());
        assert!(features.request_value(9).is_err());
        assert!(features.request_value_with_capture(9, Some("2026-02-30T12:34")).is_err());
        let captured = features.request_value_with_capture(9, Some("2026-10-29T12:34")).unwrap();
        assert_eq!(captured[0]["value"], "2026-10-29");
        assert_eq!(captured[1]["value"], "12:34");
        assert_eq!(captured[2]["value"], "2026-10-29T12:34");
        assert_eq!(captured[3]["value"], "1999-12-31");
        assert_eq!(features.request_value_with_capture(9, Some("2026-10-29T12:34")).unwrap(), captured);
        assert!(features.request_value(9).is_err(), "printing must use the stored preview array, not recapture");
        assert_eq!(Features::load_value(&features.save_value(), 9).unwrap(), features);
    }
    #[cfg(windows)]
    #[test] fn newly_added_date_can_capture_the_local_preview_clock() {
        let mut features = Features::default();
        features.add(FeatureKind::Date, 9).unwrap();
        assert!(features.has_preview_captured_datetime());
        let captured = features.capture_request(9).unwrap();
        assert!(valid_date(captured[0]["value"].as_str().unwrap()));
        assert!(features.request_value(9).is_err());
    }
    #[test] fn rejects_invalid_datetime_modes_and_capture_flags() {
        let time = Features::load_value(&json!([{"kind":"date","variant":"Time","value":"24:00","captureAtPreview":false}]),9).unwrap();
        assert!(time.validate(9).is_err());
        let bad_variant = Features::load_value(&json!([{"kind":"date","variant":"Unknown","value":"2026-10-29","captureAtPreview":true}]),9).unwrap();
        assert!(bad_variant.capture_request(9).is_err());
        assert!(Features::load_value(&json!([{"kind":"qr","value":"x","captureAtPreview":true}]),9).is_err());
        assert!(Features::load_value(&json!([{"kind":"date","captureAtPreview":"yes"}]),9).is_err());
    }
    #[test] fn rejects_invalid_barcode_and_qr_data() {
        let mut f=Feature::new(FeatureKind::Barcode, 1); f.value="£".into(); assert!(f.validate().is_err());
        f.value="ABC123".into(); assert!(f.validate().is_ok());
        let mut qr=Feature::new(FeatureKind::Qr, 2); qr.value="one\ntwo".into(); assert!(qr.validate().is_err());
    }
    #[test] fn barcode_formats_validate_and_preserve_legacy_projects() {
        let valid = [("Code128", "ABC123"), ("Code39", "ABC-123"), ("UpcE", "04252614"),
            ("Ean13", "4006381333931"), ("Ean8", "96385074"), ("UpcA", "036000291452")];
        for (variant, value) in valid {
            let loaded = Features::load_value(&json!([{"kind":"barcode", "variant":variant, "value":value}]), 9).unwrap();
            assert!(loaded.validate(9).is_ok(), "{variant}");
            let restored = Features::load_value(&loaded.save_value(), 9).unwrap();
            assert_eq!(restored.request_value(9).unwrap()[0]["variant"], variant);
        }
        let legacy = Features::load_value(&json!([{"kind":"barcode", "value":"ABC123"}]), 9).unwrap();
        assert!(legacy.validate(9).is_ok());
        assert!(legacy.request_value(9).unwrap()[0]["variant"].is_null());
        for (variant, value) in [("Code39", "abc"), ("Code39", "ABC*"), ("UpcE", "04252615"),
            ("UpcE", "24252614"), ("Ean13", "4006381333932"), ("Ean8", "96385075"),
            ("UpcA", "036000291453"), ("Ean13", "400638133393"), ("Bogus", "ABC")] {
            let loaded = Features::load_value(&json!([{"kind":"barcode", "variant":variant, "value":value}]), 9).unwrap();
            assert!(loaded.validate(9).is_err(), "{variant} {value}");
        }
    }
    #[test] fn serialises_editable_objects_with_safe_tape_geometry() {
        let mut fs=Features::default(); fs.add(FeatureKind::Qr,9).unwrap(); fs.objects[0].value="sample".into();
        fs.add(FeatureKind::Line,9).unwrap();
        let wire=fs.request_value(9).unwrap(); assert_eq!(wire[0]["kind"], "qr");
        assert!(wire[0]["heightMm"].as_f64().unwrap() <= 7.0);
        assert_eq!(wire[1]["xmm"].as_f64().unwrap(), wire[0]["widthMm"].as_f64().unwrap());
        let restored=Features::load_value(&fs.save_value(),9).unwrap();
        assert_eq!(restored.request_value(9).unwrap(), wire);
    }
    #[test] fn rejects_invalid_persisted_objects() {
        assert!(Features::load_value(&json!([{"kind":"barcode","value":"£"}]),9).unwrap().validate(9).is_err());
        assert!(Features::load_value(&json!([{"kind":"counter","startAt":-1}]),9).unwrap().validate(9).is_err());
        assert!(Features::load_value(&json!([{"kind":"unknown","value":"test"}]),9).is_err());
    }
}
