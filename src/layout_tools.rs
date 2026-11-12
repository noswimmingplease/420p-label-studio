//! Explicit, opt-in layout requests for the DYMO SDK's complete object list.
//! Positions are in millimetres relative to the printable canvas. The SDK may
//! resize or reposition objects; only its subsequently rendered result is a
//! print preview. The SDK bridge performs the final clipping/overlap checks.
use eframe::egui::{self, RichText};
use serde_json::{Value, json};

const MAX_OBJECTS: usize = 64;
const MAX_LENGTH_MM: f64 = 2000.0;
const MAX_HEIGHT_MM: f64 = 100.0;

#[derive(Clone, Debug, PartialEq)]
struct Item {
    name: String,
    kind: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    allow_overlap: bool,
    // A selection belongs to the UI, not the saved document.
    selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order { Front, Forward, Backward, Back }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment { Left, Centre, Right, Top, Middle, Bottom }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reconciliation {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// New objects which could not be placed without collision in the supplied
    /// printable canvas. Their SDK positions are retained for manual repair.
    pub needs_adjustment: Vec<String>,
}

/// Automatic mode does not submit layout requests. Manual mode always submits
/// the complete object list; it never guesses missing objects or silently drops
/// an invalid request. Selection and reset confirmation are session-only.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutState {
    manual: bool,
    locked: bool,
    items: Vec<Item>, // back to front; array position is the SDK layer
    confirm_reset: bool,
    status: String,
}

impl LayoutState {
    pub fn is_manual(&self) -> bool { self.manual }
    pub fn is_locked(&self) -> bool { self.locked }
    pub fn selected_count(&self) -> usize { self.items.iter().filter(|item| item.selected).count() }
    pub fn can_edit(&self) -> bool { self.manual && !self.locked }
    pub fn can_order(&self) -> bool { self.can_edit() && self.selected_count() == 1 }
    pub fn can_align_objects(&self) -> bool { self.can_edit() && self.selected_count() >= 2 }
    pub fn can_align_to_label(&self) -> bool { self.can_edit() && self.selected_count() >= 1 }

    /// Select exactly one known object without modifying its saved geometry.
    /// Selection is permitted while locked so the user can inspect an object.
    pub fn select_only(&mut self, name: &str) -> bool {
        if !self.manual || !self.items.iter().any(|item| item.name.eq_ignore_ascii_case(name)) { return false; }
        let mut changed = false;
        for item in &mut self.items {
            let selected = item.name.eq_ignore_ascii_case(name);
            changed |= item.selected != selected;
            item.selected = selected;
        }
        changed
    }
    pub fn selected_names(&self) -> Vec<String> {
        self.items.iter().filter(|item| item.selected).map(|item| item.name.clone()).collect()
    }
    pub fn allows_overlap(&self, first: &str, second: &str) -> bool {
        [first, second].iter().all(|name| self.items.iter().find(|item| item.name.eq_ignore_ascii_case(name)).is_some_and(|item| item.allow_overlap))
    }

    /// Restore session-only selection after an asynchronous SDK result. Names
    /// no longer present are ignored; geometry and persisted state are untouched.
    pub fn restore_selection(&mut self, names: &[String]) -> bool {
        let selected: std::collections::HashSet<String> = names.iter().map(|name| name.to_lowercase()).collect();
        let mut changed = false;
        for item in &mut self.items {
            let next = selected.contains(&item.name.to_lowercase());
            changed |= item.selected != next;
            item.selected = next;
        }
        changed
    }

    /// Commit a drag request; the SDK still validates the printable canvas,
    /// overlap policy and actual rendered positions before printing.
    pub fn move_object(&mut self, name: &str, x: f64, y: f64) -> Result<bool, String> {
        if !self.can_edit() { return Err("Unlock the manual layout before moving an object.".into()); }
        let item = self.items.iter_mut().find(|item| item.name.eq_ignore_ascii_case(name)).ok_or("The selected object is no longer in the layout.")?;
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 || x + item.width > MAX_LENGTH_MM || y + item.height > MAX_HEIGHT_MM {
            return Err(format!("{} has invalid requested bounds.", item.name));
        }
        let changed = item.x != x || item.y != y;
        item.x = x; item.y = y;
        Ok(changed)
    }

    /// Record explicitly deleted objects immediately, including before an SDK
    /// preview finishes, so a saved project cannot retain deleted placements.
    /// Refuse unknown or duplicate names and leave all geometry untouched.
    pub fn remove_intentionally(&mut self, names: &[String]) -> Result<bool, String> {
        if names.is_empty() { return Ok(false); }
        if !self.manual { return Err("Manual layout is not enabled.".into()); }
        let requested: std::collections::HashSet<String> = names.iter().map(|name| name.to_lowercase()).collect();
        if requested.len() != names.len() || requested.iter().any(|name| !self.items.iter().any(|item| item.name.to_lowercase() == *name)) {
            return Err("The requested objects could not be matched to the manual layout; no positions were removed.".into());
        }
        if requested.len() == self.items.len() { return Err("A manual layout must retain at least one object.".into()); }
        let mut items: Vec<Item> = self.items.iter().filter(|item| !requested.contains(&item.name.to_lowercase())).cloned().collect();
        if !items.iter().any(|item| item.selected) { if let Some(first) = items.first_mut() { first.selected = true; } }
        self.items = items;
        self.status = format!("Removed {} object{}; other positions were preserved.", names.len(), if names.len() == 1 { "" } else { "s" });
        Ok(true)
    }

    #[cfg(test)]
    pub fn set_locked(&mut self, value: bool) { self.locked = value; }
    pub fn toggle_locked(&mut self) { self.locked = !self.locked; }

    /// Switch to manual mode using the SDK's most recent *complete* snapshot.
    /// An invalid snapshot leaves the previous state untouched. An object
    /// outside the current canvas can be loaded so that it can be repaired;
    /// request_value refuses invalid coordinates before they reach the SDK.
    pub fn enable_manual(&mut self, snapshot: &Value) -> Result<(), String> {
        if self.manual { return Err("Manual layout is already enabled; reset it before replacing its objects.".into()); }
        if self.locked { return Err("Unlock the layout before enabling manual placement.".into()); }
        let mut items = parse_snapshot(snapshot)?;
        if let Some(first) = items.first_mut() { first.selected = true; }
        self.items = items; self.manual = true; self.confirm_reset = false;
        self.status = "Manual layout enabled. DYMO may adjust the requested positions and sizes.".into();
        Ok(())
    }

    /// Reconcile an explicitly requested object-set change against a complete
    /// SDK snapshot. Existing objects keep their exact coordinates, layer
    /// order, overlap permission and selection. New objects use SDK geometry
    /// and are placed above the retained layers in SDK order. Removed objects
    /// must exactly match the caller's intentionally removed object names.
    /// Any mismatch or malformed snapshot leaves the entire state unchanged.
    #[cfg(test)]
    pub fn reconcile(&mut self, snapshot: &Value, allowed_removed: &[String], printable_canvas: Option<(f64, f64)>) -> Result<Reconciliation, String> {
        self.reconcile_with_expansion(snapshot, allowed_removed, printable_canvas, false)
    }

    pub fn reconcile_with_expansion(&mut self, snapshot: &Value, allowed_removed: &[String], printable_canvas: Option<(f64, f64)>, allow_expansion: bool) -> Result<Reconciliation, String> {
        if !self.manual { return Err("Manual layout must be enabled before reconciling objects.".into()); }
        let incoming = parse_snapshot(snapshot)?;
        let incoming_names: std::collections::HashSet<String> = incoming.iter().map(|item| item.name.to_lowercase()).collect();
        let existing_names: std::collections::HashSet<String> = self.items.iter().map(|item| item.name.to_lowercase()).collect();
        let removed: Vec<String> = self.items.iter().filter(|item| !incoming_names.contains(&item.name.to_lowercase())).map(|item| item.name.clone()).collect();
        let actual: std::collections::HashSet<String> = removed.iter().map(|name| name.to_lowercase()).collect();
        let expected: std::collections::HashSet<String> = allowed_removed.iter().map(|name| name.to_lowercase()).collect();
        if expected.len() != allowed_removed.len() || actual != expected {
            return Err("The SDK object list does not match the requested removals; existing positions were preserved.".into());
        }
        for old in &self.items {
            if let Some(new) = incoming.iter().find(|item| item.name.eq_ignore_ascii_case(&old.name)) {
                if !new.kind.eq_ignore_ascii_case(&old.kind) {
                    return Err(format!("The object '{}' changed type; its layout was not changed.", old.name));
                }
            }
        }
        let added: Vec<String> = incoming.iter().filter(|item| !existing_names.contains(&item.name.to_lowercase())).map(|item| item.name.clone()).collect();
        let mut merged: Vec<Item> = self.items.iter().filter(|item| incoming_names.contains(&item.name.to_lowercase())).cloned().collect();
        let mut needs_adjustment = Vec::new();
        for mut item in incoming.into_iter().filter(|item| !existing_names.contains(&item.name.to_lowercase())) {
            if !place_new_object(&mut item, &merged, printable_canvas, allow_expansion) { needs_adjustment.push(item.name.clone()); }
            merged.push(item);
        }
        validate_names(&merged)?;
        if !merged.iter().any(|item| item.selected) { if let Some(first) = merged.first_mut() { first.selected = true; } }
        self.items = merged;
        self.status = if !needs_adjustment.is_empty() {
            format!("Preserved existing positions. Move {} before printing; no clear space was found.", needs_adjustment.join(", "))
        } else if added.is_empty() && removed.is_empty() {
            "Existing object positions were preserved.".into()
        } else { format!("Preserved existing positions; {} added, {} removed. Check the DYMO print preview.", added.len(), removed.len()) };
        Ok(Reconciliation { added, removed, needs_adjustment })
    }

    /// Return to the SDK's automatic placement. Call only in response to an
    /// explicit user choice; this discards the editable manual positions.
    pub fn reset_to_automatic(&mut self) -> bool {
        if self.locked || !self.manual { return false; }
        self.manual = false; self.items.clear(); self.confirm_reset = false;
        self.status = "Automatic layout restored.".into();
        true
    }

    pub fn request_value(&self) -> Result<Option<Value>, String> {
        if !self.manual { return Ok(None); }
        validate_request(&self.items)?;
        Ok(Some(Value::Array(self.items.iter().enumerate().map(|(layer, item)| json!({
            "Name": item.name, "Xmm": item.x, "Ymm": item.y,
            "WidthMm": item.width, "HeightMm": item.height,
            "Layer": layer, "AllowOverlap": item.allow_overlap,
        })).collect())))
    }

    /// Full editable state; this is not an SDK request and includes metadata.
    pub fn save_value(&self) -> Value {
        json!({"version": 1, "manual": self.manual, "locked": self.locked,
            "objects": self.items.iter().enumerate().map(|(layer, item)| json!({
                "name": item.name, "kind": item.kind, "x": item.x, "y": item.y,
                "width": item.width, "height": item.height, "layer": layer,
                "allowOverlap": item.allow_overlap,
            })).collect::<Vec<_>>()})
    }

    /// Null is the backwards-compatible value for projects predating layouts.
    /// Unknown fields are rejected rather than silently discarding user data.
    pub fn load_value(value: &Value) -> Result<Self, String> {
        if value.is_null() { return Ok(Self::default()); }
        let root = value.as_object().ok_or("Invalid saved layout.")?;
        reject_unknown(root, &["version", "manual", "locked", "objects"])?;
        if root.get("version").and_then(Value::as_u64) != Some(1) { return Err("Unsupported saved layout version.".into()); }
        let manual = root.get("manual").and_then(Value::as_bool).ok_or("Saved layout mode is missing.")?;
        let locked = root.get("locked").and_then(Value::as_bool).ok_or("Saved layout lock is missing.")?;
        let rows = root.get("objects").and_then(Value::as_array).ok_or("Saved layout objects are missing.")?;
        if rows.len() > MAX_OBJECTS || (manual && rows.is_empty()) || (!manual && !rows.is_empty()) { return Err("Saved layout has an invalid number of objects.".into()); }
        let mut parsed = Vec::with_capacity(rows.len());
        for row in rows {
            let obj = row.as_object().ok_or("Invalid saved layout object.")?;
            reject_unknown(obj, &["name", "kind", "x", "y", "width", "height", "layer", "allowOverlap"])?;
            let item = Item { name: string_field(obj, "name")?, kind: string_field(obj, "kind")?,
                x: number_field(obj, "x")?, y: number_field(obj, "y")?, width: number_field(obj, "width")?, height: number_field(obj, "height")?,
                allow_overlap: obj.get("allowOverlap").and_then(Value::as_bool).ok_or("Object overlap setting is missing.")?, selected: false };
            validate_snapshot_item(&item)?;
            let layer = obj.get("layer").and_then(Value::as_u64).ok_or("Invalid saved object layer.")?;
            parsed.push((layer, item));
        }
        parsed.sort_by_key(|(layer, _)| *layer);
        if parsed.iter().enumerate().any(|(index, (layer, _))| *layer != index as u64) { return Err("Saved object layers must be unique and contiguous.".into()); }
        let mut items = parsed.into_iter().map(|(_, item)| item).collect::<Vec<_>>();
        validate_names(&items)?;
        if let Some(first) = items.first_mut() { first.selected = true; }
        Ok(Self { manual, locked, items, confirm_reset: false, status: String::new() })
    }

    /// Change a selected object's layer. Only single-selection ordering is
    /// supported, so no object is moved unexpectedly as a side effect.
    pub fn move_selected(&mut self, order: Order) -> bool {
        if !self.can_order() { return false; }
        let from = self.items.iter().position(|item| item.selected).unwrap();
        let to = match order { Order::Front => self.items.len() - 1, Order::Forward => (from + 1).min(self.items.len() - 1), Order::Backward => from.saturating_sub(1), Order::Back => 0 };
        if from == to { return false; }
        let object = self.items.remove(from); self.items.insert(to, object); true
    }

    /// Align selected objects within their joint bounding box.
    pub fn align_selected(&mut self, alignment: Alignment) -> Result<bool, String> {
        if !self.can_align_objects() { return Err("Select at least two objects and unlock the layout.".into()); }
        validate_request(&self.items)?;
        let selected = self.items.iter().filter(|item| item.selected).collect::<Vec<_>>();
        let left = selected.iter().map(|item| item.x).fold(f64::INFINITY, f64::min);
        let right = selected.iter().map(|item| item.x + item.width).fold(f64::NEG_INFINITY, f64::max);
        let top = selected.iter().map(|item| item.y).fold(f64::INFINITY, f64::min);
        let bottom = selected.iter().map(|item| item.y + item.height).fold(f64::NEG_INFINITY, f64::max);
        let mut changed = false;
        for item in self.items.iter_mut().filter(|item| item.selected) {
            let (x, y) = aligned_position(item, alignment, left, right, top, bottom);
            changed |= item.x != x || item.y != y; item.x = x; item.y = y;
        }
        Ok(changed)
    }

    /// Align selected objects to the SDK's printable canvas, not the cut tape.
    pub fn align_to_canvas(&mut self, alignment: Alignment, width_mm: f64, height_mm: f64) -> Result<bool, String> {
        if !self.can_align_to_label() { return Err("Select at least one object and unlock the layout.".into()); }
        if !width_mm.is_finite() || !height_mm.is_finite() || width_mm <= 0.0 || width_mm > MAX_LENGTH_MM || height_mm <= 0.0 || height_mm > MAX_HEIGHT_MM { return Err("Valid SDK canvas dimensions are required.".into()); }
        validate_request(&self.items)?;
        if self.items.iter().any(|item| item.selected && (item.width > width_mm || item.height > height_mm)) { return Err("An object is larger than the printable canvas.".into()); }
        let mut changed = false;
        for item in self.items.iter_mut().filter(|item| item.selected) {
            let (x, y) = aligned_position(item, alignment, 0.0, width_mm, 0.0, height_mm);
            changed |= item.x != x || item.y != y; item.x = x; item.y = y;
        }
        Ok(changed)
    }

    /// Draw the layout panel. The `snapshot` is the latest SDK snapshot; it is
    /// consulted only when the user explicitly enables manual layout.
    pub fn ui(&mut self, ui: &mut egui::Ui, snapshot: Option<&Value>) -> bool {
        let mut changed = false;
        ui.label(RichText::new("The SDK may adjust placement. Check its print preview before printing.").small());
        changed |= ui.checkbox(&mut self.locked, "Lock layout").changed();
        if !self.manual {
            if ui.add_enabled(!self.locked && snapshot.is_some(), egui::Button::new("Enable manual layout")).clicked() {
                match self.enable_manual(snapshot.unwrap()) { Ok(()) => changed = true, Err(error) => self.status = error }
            }
            if snapshot.is_none() { ui.label("Generate a DYMO preview to load the complete object list."); }
        } else {
            ui.label("Select one or more objects; lower entries are in front.");
            for item in &mut self.items {
                ui.checkbox(&mut item.selected, format!("{} · {}", item.name, item.kind));
            }
            let editable = self.can_edit();
            if self.selected_count() == 1 {
                let index = self.items.iter().position(|item| item.selected).unwrap();
                let item = &mut self.items[index];
                ui.add_enabled_ui(editable, |ui| {
                    ui.horizontal(|ui| { ui.label("X (mm)"); changed |= ui.add(egui::DragValue::new(&mut item.x).range(0.0..=MAX_LENGTH_MM).speed(0.1)).changed(); });
                    ui.horizontal(|ui| { ui.label("Y (mm)"); changed |= ui.add(egui::DragValue::new(&mut item.y).range(0.0..=MAX_HEIGHT_MM).speed(0.1)).changed(); });
                    ui.horizontal(|ui| { ui.label("Width (mm)"); changed |= ui.add(egui::DragValue::new(&mut item.width).range(0.01..=MAX_LENGTH_MM).speed(0.1)).changed(); });
                    ui.horizontal(|ui| { ui.label("Height (mm)"); changed |= ui.add(egui::DragValue::new(&mut item.height).range(0.01..=MAX_HEIGHT_MM).speed(0.1)).changed(); });
                    changed |= ui.checkbox(&mut item.allow_overlap, "Allow intentional overlap").on_hover_text("Both overlapping objects must allow overlap.").changed();
                });
            } else { ui.label("Select one object to edit its position and size."); }
            ui.horizontal_wrapped(|ui| {
                for (caption, action) in [("To front", Order::Front), ("Forward", Order::Forward), ("Backward", Order::Backward), ("To back", Order::Back)] {
                    if ui.add_enabled(self.can_order(), egui::Button::new(caption)).clicked() { changed |= self.move_selected(action); }
                }
            });
            if let Err(error) = self.request_value() { ui.colored_label(egui::Color32::from_rgb(235, 160, 100), error); }
            if !self.confirm_reset {
                if ui.add_enabled(editable, egui::Button::new("Reset to automatic layout…")).clicked() { self.confirm_reset = true; }
            } else {
                ui.label("Discard the manual positions and restore automatic layout?");
                ui.horizontal(|ui| {
                    if ui.add_enabled(!self.locked, egui::Button::new("Discard manual layout")).clicked() { changed |= self.reset_to_automatic(); }
                    if ui.button("Keep manual layout").clicked() { self.confirm_reset = false; }
                });
            }
        }
        if !self.status.is_empty() { ui.label(&self.status); }
        changed
    }
}

fn aligned_position(item: &Item, alignment: Alignment, left: f64, right: f64, top: f64, bottom: f64) -> (f64, f64) {
    match alignment {
        Alignment::Left => (left, item.y), Alignment::Centre => ((left + right - item.width) / 2.0, item.y), Alignment::Right => (right - item.width, item.y),
        Alignment::Top => (item.x, top), Alignment::Middle => (item.x, (top + bottom - item.height) / 2.0), Alignment::Bottom => (item.x, bottom - item.height),
    }
}

/// Try SDK placement first, then positions touching each existing edge. Only
/// the new item moves; retained layouts, including intentional overlaps, stay
/// exactly as the user set them. The SDK will perform the final validation.
fn place_new_object(item: &mut Item, existing: &[Item], printable_canvas: Option<(f64, f64)>, allow_expansion: bool) -> bool {
    const TOLERANCE: f64 = 0.15;
    let Some((canvas_width, canvas_height)) = printable_canvas.filter(|(width, height)|
        width.is_finite() && height.is_finite() && *width > 0.0 && *width <= MAX_LENGTH_MM && *height > 0.0 && *height <= MAX_HEIGHT_MM) else { return false; };
    let max_width = if allow_expansion { MAX_LENGTH_MM } else { canvas_width };
    let fits = |x: f64, y: f64| {
        x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0 &&
        x + item.width <= max_width && y + item.height <= canvas_height + TOLERANCE &&
        existing.iter().all(|other| {
            let overlap_x = (x + item.width).min(other.x + other.width) - x.max(other.x);
            let overlap_y = (y + item.height).min(other.y + other.height) - y.max(other.y);
            overlap_x <= TOLERANCE || overlap_y <= TOLERANCE
        })
    };
    if fits(item.x, item.y) { return true; }
    let mut xs = vec![0.0]; let mut ys = vec![0.0];
    if item.x >= 0.0 { xs.push(item.x); } if item.y >= 0.0 { ys.push(item.y); }
    for other in existing { xs.push(other.x + other.width); ys.push(other.y + other.height); }
    xs.sort_by(f64::total_cmp); xs.dedup_by(|a, b| (*a - *b).abs() < 0.001);
    ys.sort_by(f64::total_cmp); ys.dedup_by(|a, b| (*a - *b).abs() < 0.001);
    for y in ys { for &x in &xs { if fits(x, y) { item.x = x; item.y = y; return true; } } }
    false
}

fn string_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<String, String> {
    let value = obj.get(key).and_then(Value::as_str).ok_or_else(|| format!("{key} is missing."))?;
    if value.trim().is_empty() || value.chars().count() > 128 || value.chars().any(char::is_control) { return Err(format!("{key} is invalid.")); }
    Ok(value.to_owned())
}
fn number_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<f64, String> {
    obj.get(key).and_then(Value::as_f64).filter(|value| value.is_finite()).ok_or_else(|| format!("{key} is missing or invalid."))
}
fn parse_snapshot(snapshot: &Value) -> Result<Vec<Item>, String> {
    let entries = snapshot.as_array().ok_or("The SDK did not provide an object list.")?;
    if entries.is_empty() || entries.len() > MAX_OBJECTS { return Err("The SDK object list must contain 1–64 objects.".into()); }
    let mut parsed = Vec::with_capacity(entries.len());
    for entry in entries {
        let obj = entry.as_object().ok_or("Invalid SDK object.")?;
        let name = string_field(obj, "Name")?;
        let kind = string_field(obj, "Kind")?;
        let layer = obj.get("Layer").and_then(Value::as_u64).ok_or("Object layer is missing or invalid.")?;
        if layer >= entries.len() as u64 { return Err("The SDK object layer is outside the object list.".into()); }
        let item = Item {
            name, kind,
            x: number_field(obj, "Xmm")?, y: number_field(obj, "Ymm")?,
            width: number_field(obj, "WidthMm")?, height: number_field(obj, "HeightMm")?,
            allow_overlap: false, selected: false,
        };
        validate_snapshot_item(&item)?;
        // Allow out-of-canvas snapshots to be loaded for explicit repair.
        obj.get("IsValid").and_then(Value::as_bool).ok_or("Object validity is missing.")?;
        obj.get("InCanvas").and_then(Value::as_bool).ok_or("Object canvas status is missing.")?;
        parsed.push((layer, item));
    }
    parsed.sort_by_key(|(layer, _)| *layer);
    if parsed.iter().enumerate().any(|(index, (layer, _))| *layer != index as u64) { return Err("The SDK object layers must be unique and contiguous.".into()); }
    let items = parsed.into_iter().map(|(_, item)| item).collect::<Vec<_>>();
    validate_names(&items)?;
    Ok(items)
}
fn reject_unknown(obj: &serde_json::Map<String, Value>, accepted: &[&str]) -> Result<(), String> {
    if obj.keys().any(|key| !accepted.contains(&key.as_str())) { return Err("The saved layout contains unsupported fields and was not changed.".into()); }
    Ok(())
}
fn validate_snapshot_item(item: &Item) -> Result<(), String> {
    if !item.x.is_finite() || !item.y.is_finite() || !item.width.is_finite() || !item.height.is_finite() ||
        item.x.abs() > 10000.0 || item.y.abs() > 100.0 || item.width <= 0.0 || item.width > 10000.0 || item.height <= 0.0 || item.height > 100.0 {
        return Err(format!("{} has invalid SDK geometry.", item.name));
    }
    Ok(())
}
fn validate_names(items: &[Item]) -> Result<(), String> {
    let mut names = std::collections::HashSet::new();
    if items.iter().any(|item| !names.insert(item.name.to_lowercase())) { return Err("Object names must be unique.".into()); }
    Ok(())
}
fn validate_request(items: &[Item]) -> Result<(), String> {
    if items.is_empty() || items.len() > MAX_OBJECTS { return Err("Manual layout requires 1–64 objects.".into()); }
    validate_names(items)?;
    for item in items {
        validate_snapshot_item(item)?;
        if item.x < 0.0 || item.y < 0.0 || item.x + item.width > MAX_LENGTH_MM || item.y + item.height > MAX_HEIGHT_MM {
            return Err(format!("{} has invalid requested bounds.", item.name));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Value { json!([
        {"Name":"Extra1","Kind":"ShapeObject","Xmm":12.0,"Ymm":0.0,"WidthMm":4.0,"HeightMm":4.0,"Layer":1,"IsValid":true,"InCanvas":true},
        {"Name":"TextObject0","Kind":"TextObject","Xmm":0.0,"Ymm":0.0,"WidthMm":10.0,"HeightMm":4.0,"Layer":0,"IsValid":true,"InCanvas":true}
    ]) }

    #[test]
    fn automatic_and_manual_requests_round_trip() {
        let mut state = LayoutState::default();
        assert_eq!(state.request_value().unwrap(), None);
        state.enable_manual(&snapshot()).unwrap();
        assert_eq!(state.selected_count(), 1);
        let wire = state.request_value().unwrap().unwrap();
        assert_eq!(wire[0]["Name"], "TextObject0");
        assert_eq!(wire[1]["Name"], "Extra1");
        assert_eq!(wire[1]["Layer"], 1);
        assert_eq!(wire[1]["AllowOverlap"], false);
        state.items[1].allow_overlap = true;
        state.locked = true;
        let saved = state.save_value();
        let restored = LayoutState::load_value(&saved).unwrap();
        assert!(restored.is_manual() && restored.is_locked());
        assert_eq!(restored.request_value().unwrap(), state.request_value().unwrap());
        assert_eq!(LayoutState::load_value(&Value::Null).unwrap().request_value().unwrap(), None);
    }

    #[test]
    fn rejects_invalid_sdk_snapshots_without_changing_previous_state() {
        let mut state = LayoutState::default();
        let mut bad = snapshot(); bad[0]["Name"] = json!("textobject0");
        assert!(state.enable_manual(&bad).unwrap_err().contains("unique"));
        assert!(!state.is_manual());
        let mut bad = snapshot(); bad[0]["Layer"] = json!(0);
        assert!(state.enable_manual(&bad).unwrap_err().contains("layers"));
        let mut bad = snapshot(); bad[0]["WidthMm"] = json!(-1);
        assert!(state.enable_manual(&bad).is_err());
        assert_eq!(state.request_value().unwrap(), None);
    }

    #[test]
    fn saved_unknown_fields_are_not_silently_discarded() {
        let mut state = LayoutState::default(); state.enable_manual(&snapshot()).unwrap();
        let mut saved = state.save_value(); saved["objects"][0]["futureOption"] = json!(42);
        assert!(LayoutState::load_value(&saved).unwrap_err().contains("unsupported fields"));
        let mut saved = state.save_value(); saved["objects"][1]["layer"] = json!(0);
        assert!(LayoutState::load_value(&saved).is_err());
    }

    #[test]
    fn locked_layout_does_not_move_or_align_objects() {
        let mut state = LayoutState::default(); state.enable_manual(&snapshot()).unwrap();
        state.set_locked(true);
        let before = state.request_value().unwrap();
        assert!(!state.move_selected(Order::Front));
        assert!(state.align_to_canvas(Alignment::Right, 50.0, 8.0).is_err());
        assert!(!state.reset_to_automatic());
        assert_eq!(state.request_value().unwrap(), before);
    }

    #[test]
    fn layers_and_alignment_are_deterministic() {
        let mut state = LayoutState::default(); state.enable_manual(&snapshot()).unwrap();
        assert!(state.move_selected(Order::Front));
        let wire = state.request_value().unwrap().unwrap();
        assert_eq!(wire[0]["Name"], "Extra1"); assert_eq!(wire[1]["Name"], "TextObject0");
        assert!(state.align_to_canvas(Alignment::Right, 50.0, 8.0).unwrap());
        assert_eq!(state.request_value().unwrap().unwrap()[1]["Xmm"], 40.0);
        state.items[0].selected = true;
        assert!(state.align_selected(Alignment::Left).unwrap());
        let wire = state.request_value().unwrap().unwrap();
        assert_eq!(wire[0]["Xmm"], wire[1]["Xmm"]);
        assert!(state.align_to_canvas(Alignment::Right, f64::NAN, 8.0).is_err());
    }

    #[test]
    fn invalid_requested_bounds_fail_closed_until_repaired() {
        let mut input = snapshot(); input[0]["Xmm"] = json!(-0.5); input[0]["InCanvas"] = json!(false);
        let mut state = LayoutState::default(); state.enable_manual(&input).unwrap();
        assert!(state.request_value().unwrap_err().contains("bounds"));
        state.items[1].x = 0.0;
        assert!(state.request_value().unwrap().is_some());
    }

    #[test]
    fn reconciliation_preserves_existing_positions_flags_and_order() {
        let mut state = LayoutState::default(); state.enable_manual(&snapshot()).unwrap();
        state.items[0].x = 7.25; state.items[0].allow_overlap = true;
        state.items[1].x = 31.0; state.items[1].selected = true;
        assert!(state.move_selected(Order::Front) == false); // Both are selected.
        let before = state.request_value().unwrap().unwrap();
        let mut incoming = snapshot();
        incoming[0]["Xmm"] = json!(70.0); incoming[1]["Xmm"] = json!(60.0);
        incoming.as_array_mut().unwrap().push(json!({"Name":"Extra2","Kind":"LineObject","Xmm":42.0,"Ymm":0.0,"WidthMm":4.0,"HeightMm":4.0,"Layer":2,"IsValid":true,"InCanvas":true}));
        let summary = state.reconcile(&incoming, &[], Some((100.0, 9.0))).unwrap();
        assert_eq!(summary.added, vec!["Extra2"]); assert!(summary.removed.is_empty());
        assert!(summary.needs_adjustment.is_empty());
        let after = state.request_value().unwrap().unwrap();
        assert_eq!(after[0], before[0]); assert_eq!(after[1], before[1]);
        assert_eq!(after[2]["Name"], "Extra2"); assert_eq!(after[2]["Xmm"], 42.0);
        assert_eq!(after[2]["AllowOverlap"], false); assert_eq!(state.selected_count(), 2);
    }

    #[test]
    fn reconciliation_never_drops_unapproved_or_malformed_objects() {
        let mut state = LayoutState::default(); state.enable_manual(&snapshot()).unwrap();
        state.items[1].x = 30.0;
        let before = state.clone();
        let mut fewer = snapshot(); fewer.as_array_mut().unwrap().retain(|item| item["Name"] == "TextObject0");
        assert!(state.reconcile(&fewer, &[], Some((50.0, 9.0))).unwrap_err().contains("requested removals"));
        assert_eq!(state, before);
        let mut wrong_kind = snapshot(); wrong_kind[0]["Kind"] = json!("BarcodeObject");
        assert!(state.reconcile(&wrong_kind, &[], Some((50.0, 9.0))).unwrap_err().contains("changed type"));
        assert_eq!(state, before);
        let mut duplicate = snapshot(); duplicate[0]["Name"] = json!("textobject0");
        assert!(state.reconcile(&duplicate, &[], Some((50.0, 9.0))).is_err());
        assert_eq!(state, before);
        assert!(state.reconcile(&fewer, &["TextObject0".into()], Some((50.0, 9.0))).is_err());
        assert_eq!(state, before);
        let result = state.reconcile(&fewer, &["Extra1".into()], Some((50.0, 9.0))).unwrap();
        assert_eq!(result.removed, vec!["Extra1"]);
        let wire = state.request_value().unwrap().unwrap();
        assert_eq!(wire.as_array().unwrap().len(), 1); assert_eq!(wire[0]["Name"], "TextObject0");
    }

    #[test]
    fn reconciliation_places_only_new_objects_in_clear_space_or_reports_failure() {
        let mut state = LayoutState::default(); state.enable_manual(&snapshot()).unwrap();
        state.items[1].x = 14.0;
        let previous = state.request_value().unwrap().unwrap();
        let mut incoming = snapshot();
        incoming.as_array_mut().unwrap().push(json!({"Name":"Extra2","Kind":"LineObject","Xmm":2.0,"Ymm":0.0,"WidthMm":4.0,"HeightMm":4.0,"Layer":2,"IsValid":true,"InCanvas":true}));
        let mut too_small = state.clone();
        too_small.items[1].x = 10.0;
        let small = too_small.reconcile(&incoming, &[], Some((17.0, 4.0))).unwrap();
        assert_eq!(small.needs_adjustment, vec!["Extra2"]);
        assert_eq!(too_small.request_value().unwrap().unwrap()[2]["Xmm"], 2.0, "failure keeps the SDK-provided position");
        let summary = state.reconcile(&incoming, &[], Some((24.0, 4.0))).unwrap();
        assert!(summary.needs_adjustment.is_empty());
        let wire = state.request_value().unwrap().unwrap();
        assert_eq!(wire[0], previous[0]); assert_eq!(wire[1], previous[1]);
        assert_eq!(wire[2]["Xmm"], 10.0);
    }

    #[test]
    fn canvas_selection_and_move_only_change_the_requested_item() {
        let mut state = LayoutState::default(); state.enable_manual(&snapshot()).unwrap();
        let before = state.request_value().unwrap().unwrap();
        assert!(state.select_only("extra1")); assert_eq!(state.selected_names(), vec!["Extra1"]);
        assert!(!state.select_only("missing")); assert_eq!(state.selected_names(), vec!["Extra1"]);
        assert!(state.move_object("Extra1", 25.0, 1.0).unwrap());
        let after = state.request_value().unwrap().unwrap();
        assert_eq!(after[0], before[0]);
        assert_eq!(after[1]["Xmm"], 25.0); assert_eq!(after[1]["Ymm"], 1.0);
        assert_eq!(after[1]["WidthMm"], before[1]["WidthMm"]);
        assert!(state.move_object("Extra1", f64::NAN, 0.0).is_err());
        assert!(state.move_object("Extra1", 1999.0, 0.0).is_err());
        state.toggle_locked();
        assert!(state.move_object("Extra1", 5.0, 0.0).is_err());
        assert_eq!(state.request_value().unwrap().unwrap(), after);
        assert!(state.select_only("TextObject0"));
    }

    #[test]
    fn explicitly_removed_objects_are_persisted_without_changing_survivors() {
        let mut state = LayoutState::default(); state.enable_manual(&snapshot()).unwrap();
        state.items[0].x = 4.5; state.items[0].allow_overlap = true;
        let before = state.request_value().unwrap().unwrap()[0].clone();
        let old = state.clone();
        assert!(state.remove_intentionally(&["missing".into()]).is_err()); assert_eq!(state, old);
        assert!(state.remove_intentionally(&["Extra1".into(), "extra1".into()]).is_err()); assert_eq!(state, old);
        assert!(state.remove_intentionally(&["Extra1".into(), "TextObject0".into()]).is_err()); assert_eq!(state, old);
        assert!(state.remove_intentionally(&["EXTRA1".into()]).unwrap());
        let saved = state.save_value();
        let restored = LayoutState::load_value(&saved).unwrap();
        assert_eq!(restored.request_value().unwrap().unwrap()[0], before);
        assert_eq!(restored.request_value().unwrap().unwrap().as_array().unwrap().len(), 1);
    }

    #[test]
    fn restored_selection_ignores_removed_names_and_keeps_geometry() {
        let mut state = LayoutState::default(); state.enable_manual(&snapshot()).unwrap();
        let geometry = state.request_value().unwrap().unwrap();
        assert!(state.restore_selection(&["textobject0".into(), "EXTRA1".into(), "removed".into()]));
        assert_eq!(state.selected_names(), vec!["TextObject0", "Extra1"]);
        assert_eq!(state.request_value().unwrap().unwrap(), geometry);
        assert!(!state.restore_selection(&["TextObject0".into(), "extra1".into()]));
        assert!(state.restore_selection(&["removed".into()]));
        assert!(state.selected_names().is_empty());
    }
}
