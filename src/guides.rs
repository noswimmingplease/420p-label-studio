//! Persistent, non-printing guides in millimetres relative to the SDK's
//! printable layout bounds. Guide geometry never changes the SDK label model.
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke};
use serde_json::{json, Value};

const MAX_GUIDES_PER_AXIS: usize = 64;
const MAX_X_MM: f64 = 2000.0;
const MAX_Y_MM: f64 = 100.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Guides {
    visible: bool,
    snap: bool,
    vertical_mm: Vec<f64>,
    horizontal_mm: Vec<f64>,
}

impl Default for Guides {
    fn default() -> Self {
        Self { visible: true, snap: true, vertical_mm: Vec::new(), horizontal_mm: Vec::new() }
    }
}

impl Guides {
    pub fn is_visible(&self) -> bool { self.visible }
    pub fn snapping_enabled(&self) -> bool { self.snap }
    pub fn set_visible(&mut self, value: bool) { self.visible = value; }
    pub fn set_snapping(&mut self, value: bool) { self.snap = value; }

    pub fn save_value(&self) -> Value {
        json!({"version": 1, "visible": self.visible, "snap": self.snap,
            "verticalMm": self.vertical_mm, "horizontalMm": self.horizontal_mm})
    }

    /// Null supports projects saved before guides were introduced. Reject
    /// unknown fields so loading cannot silently discard future guide data.
    pub fn load_value(value: &Value) -> Result<Self, String> {
        if value.is_null() { return Ok(Self::default()); }
        let root = value.as_object().ok_or("Invalid saved guides.")?;
        if root.keys().any(|key| !["version", "visible", "snap", "verticalMm", "horizontalMm"].contains(&key.as_str())) {
            return Err("Saved guides contain an unsupported field.".into());
        }
        if root.get("version").and_then(Value::as_u64) != Some(1) { return Err("Unsupported guide version.".into()); }
        let visible = root.get("visible").and_then(Value::as_bool).ok_or("Guide visibility is missing.")?;
        let snap = root.get("snap").and_then(Value::as_bool).ok_or("Guide snapping setting is missing.")?;
        fn read_axis(value: Option<&Value>, max: f64) -> Result<Vec<f64>, String> {
            let rows = value.and_then(Value::as_array).ok_or("Guide positions are missing.")?;
            if rows.len() > MAX_GUIDES_PER_AXIS { return Err("Too many guides.".into()); }
            rows.iter().map(|value| value.as_f64().filter(|v| v.is_finite() && (0.0..=max).contains(v)).ok_or_else(|| "A guide position is invalid.".into())).collect()
        }
        Ok(Self { visible, snap, vertical_mm: read_axis(root.get("verticalMm"), MAX_X_MM)?, horizontal_mm: read_axis(root.get("horizontalMm"), MAX_Y_MM)? })
    }

    /// Draw over the actual printable-canvas rectangle, not the outer label or
    /// its ruler. The caller must clip this rect to its visible scroll area.
    pub fn draw(&self, painter: &Painter, printable_rect: Rect, width_mm: f64, height_mm: f64) {
        if !self.visible || !valid_canvas(printable_rect, width_mm, height_mm) { return; }
        let painter = painter.with_clip_rect(printable_rect);
        let stroke = Stroke::new(1.0, Color32::from_rgb(40, 210, 225));
        for &x in &self.vertical_mm {
            if (0.0..=width_mm).contains(&x) {
                let px = printable_rect.left() + printable_rect.width() * (x / width_mm) as f32;
                painter.line_segment([Pos2::new(px, printable_rect.top()), Pos2::new(px, printable_rect.bottom())], stroke);
            }
        }
        for &y in &self.horizontal_mm {
            if (0.0..=height_mm).contains(&y) {
                let py = printable_rect.top() + printable_rect.height() * (y / height_mm) as f32;
                painter.line_segment([Pos2::new(printable_rect.left(), py), Pos2::new(printable_rect.right(), py)], stroke);
            }
        }
    }

    /// Snap a movable object's left/centre/right and top/middle/bottom anchors
    /// to a guide. The caller converts its fixed pixel tolerance to millimetres
    /// independently for each axis, giving stable behaviour at every zoom.
    /// Coordinates must already be clamped to the current printable bounds.
    pub fn snap_position(&self, x: f64, y: f64, width: f64, height: f64,
        canvas_width: f64, canvas_height: f64, tolerance_x: f64, tolerance_y: f64) -> (f64, f64) {
        if !self.snap { return (x, y); }
        (snap_axis(x, width, canvas_width, tolerance_x, &self.vertical_mm),
         snap_axis(y, height, canvas_height, tolerance_y, &self.horizontal_mm))
    }

    /// Draw management controls. Existing guide positions are never silently
    /// truncated when tape or label dimensions change. Returns true on changes.
    pub fn ui(&mut self, ui: &mut egui::Ui, printable_dimensions: Option<(f64, f64)>) -> bool {
        let mut changed = false;
        ui.label("Guides are measured from the printable canvas and do not print.");
        changed |= ui.checkbox(&mut self.visible, "Show guides").changed();
        changed |= ui.checkbox(&mut self.snap, "Snap object edges and centres to guides").changed();
        if !self.visible && self.snap { ui.label("Snapping is still active while guides are hidden."); }
        changed |= axis_controls(ui, "Vertical guides (mm)", &mut self.vertical_mm,
            printable_dimensions.map(|size| size.0), MAX_X_MM, "vertical-guide");
        changed |= axis_controls(ui, "Horizontal guides (mm)", &mut self.horizontal_mm,
            printable_dimensions.map(|size| size.1), MAX_Y_MM, "horizontal-guide");
        changed
    }
}

fn axis_controls(ui: &mut egui::Ui, heading: &str, positions: &mut Vec<f64>, size: Option<f64>, max: f64, id: &str) -> bool {
    let mut changed = false;
    ui.separator();
    ui.label(heading);
    if let Some(size) = size.filter(|value| value.is_finite() && *value > 0.0) {
        ui.label(format!("Printable extent: {size:.2} mm"));
    } else { ui.label("Generate a preview to see the printable extent."); }
    let mut remove = None;
    egui::ScrollArea::vertical().id_salt(id).max_height(120.0).show(ui, |ui| {
        for (index, position) in positions.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{}", index + 1));
                changed |= ui.add(egui::DragValue::new(position).range(0.0..=max).speed(0.1).suffix(" mm")).changed();
                if size.is_some_and(|limit| *position > limit) { ui.label("Outside current canvas"); }
                if ui.button("Remove").clicked() { remove = Some(index); }
            });
        }
    });
    if let Some(index) = remove { positions.remove(index); changed = true; }
    if ui.add_enabled(positions.len() < MAX_GUIDES_PER_AXIS, egui::Button::new("Add guide")).clicked() {
        positions.push(size.filter(|v| v.is_finite() && *v > 0.0).map(|v| (v / 2.0).min(max)).unwrap_or(0.0));
        changed = true;
    }
    changed
}

fn valid_canvas(rect: Rect, width_mm: f64, height_mm: f64) -> bool {
    rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0 &&
        width_mm.is_finite() && width_mm > 0.0 && height_mm.is_finite() && height_mm > 0.0
}

fn snap_axis(position: f64, extent: f64, canvas: f64, tolerance: f64, guides: &[f64]) -> f64 {
    if !position.is_finite() || !extent.is_finite() || !canvas.is_finite() || !tolerance.is_finite() ||
        extent <= 0.0 || extent > canvas || tolerance <= 0.0 || position < 0.0 || position + extent > canvas + 0.000_001 { return position; }
    let mut nearest: Option<(f64, f64)> = None;
    for &guide in guides {
        if !guide.is_finite() || !(0.0..=canvas).contains(&guide) { continue; }
        for offset in [0.0, extent / 2.0, extent] {
            let candidate = guide - offset;
            if candidate < 0.0 || candidate + extent > canvas + 0.000_001 { continue; }
            let distance = (candidate - position).abs();
            if distance <= tolerance && nearest.is_none_or(|(best, _)| distance < best) { nearest = Some((distance, candidate)); }
        }
    }
    nearest.map_or(position, |(_, candidate)| candidate.clamp(0.0, (canvas - extent).max(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_guides_round_trip_and_legacy_projects_load() {
        let guides = Guides { visible: false, snap: true, vertical_mm: vec![1.5, 25.0], horizontal_mm: vec![3.0] };
        assert_eq!(Guides::load_value(&guides.save_value()).unwrap(), guides);
        assert_eq!(Guides::load_value(&Value::Null).unwrap(), Guides::default());
    }
    #[test]
    fn saved_guides_reject_invalid_or_unknown_values() {
        assert!(Guides::load_value(&json!({"version":1,"visible":true,"snap":true,"verticalMm":[-1],"horizontalMm":[]})).is_err());
        assert!(Guides::load_value(&json!({"version":1,"visible":true,"snap":true,"verticalMm":[],"horizontalMm":[101]})).is_err());
        assert!(Guides::load_value(&json!({"version":1,"visible":true,"snap":true,"verticalMm":[],"horizontalMm":[],"future":1})).is_err());
        assert!(Guides::load_value(&json!({"version":1,"visible":true,"snap":true,"verticalMm":vec![0; 65],"horizontalMm":[]})).is_err());
    }
    #[test]
    fn snap_uses_nearest_edge_or_centre_and_respects_bounds() {
        let guides = Guides { vertical_mm: vec![20.0, 30.0], horizontal_mm: vec![5.0], ..Default::default() };
        assert_eq!(guides.snap_position(15.2, 2.9, 10.0, 4.0, 50.0, 9.0, 0.5, 0.2), (15.0, 3.0));
        assert_eq!(guides.snap_position(29.8, 2.1, 10.0, 4.0, 50.0, 9.0, 0.3, 0.05), (30.0, 2.1));
        assert_eq!(guides.snap_position(0.0, 0.0, 10.0, 4.0, 15.0, 9.0, 1.0, 1.0), (0.0, 1.0));
    }
    #[test]
    fn snap_can_be_disabled_and_does_not_move_oversize_objects() {
        let mut guides = Guides { vertical_mm: vec![5.0], ..Default::default() };
        guides.set_snapping(false);
        assert_eq!(guides.snap_position(4.8, 0.0, 2.0, 2.0, 10.0, 10.0, 1.0, 1.0), (4.8, 0.0));
        guides.set_snapping(true);
        assert_eq!(guides.snap_position(4.8, 0.0, 20.0, 2.0, 10.0, 10.0, 1.0, 1.0), (4.8, 0.0));
    }
    #[test]
    fn invisible_guides_still_snap_and_empty_list_is_noop() {
        let mut guides = Guides::default();
        assert_eq!(guides.snap_position(4.8, 0.0, 2.0, 2.0, 10.0, 10.0, 1.0, 1.0), (4.8, 0.0));
        guides.vertical_mm.push(5.0); guides.set_visible(false);
        assert_eq!(guides.snap_position(4.8, 0.0, 2.0, 2.0, 10.0, 10.0, 1.0, 1.0), (5.0, 0.0));
    }
}
