//! Interaction overlay for SDK-resolved objects on the label preview.
//!
//! The caller provides the *printable* rectangle in screen coordinates, after
//! accounting for label margins, scrolling and zoom. No assumptions about the
//! printer DPI or cut-tape dimensions are made here. Dragging is provisional:
//! only the final position is returned, and the caller must render a fresh SDK
//! preview before permitting printing.
use eframe::egui::{self, Color32, Pos2, Rect, Response, Stroke, StrokeKind, Vec2};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub layer: u64,
}

#[derive(Clone, Debug)]
pub struct MoveVisual { pub object: Object, pub x: f64, pub y: f64 }

/// Check a proposed move against SDK-resolved visible objects. The bridge is
/// still authoritative because the SDK can adjust the resulting geometry.
pub fn collision_at<'a>(objects: &'a [Object], moving: &Object, x: f64, y: f64, allow: impl Fn(&str, &str) -> bool) -> Option<&'a str> {
    objects.iter().filter(|other| !other.name.eq_ignore_ascii_case(&moving.name)).find(|other| {
        let overlap_x = (x + moving.width).min(other.x + other.width) - x.max(other.x);
        let overlap_y = (y + moving.height).min(other.y + other.height) - y.max(other.y);
        overlap_x > 0.15 && overlap_y > 0.15 && !allow(&moving.name, &other.name)
    }).map(|other| other.name.as_str())
}

/// A fresh SDK object list, in back-to-front order. Invalid data disables the
/// overlay instead of allowing a drag against guessed geometry.
pub fn objects_from_snapshot(snapshot: &Value) -> Result<Vec<Object>, String> {
    let rows = snapshot.as_array().ok_or("The SDK object list is unavailable.")?;
    if rows.len() > 64 { return Err("The SDK returned too many objects.".into()); }
    let mut objects = Vec::with_capacity(rows.len());
    for row in rows {
        let name = row["Name"].as_str().filter(|s| !s.trim().is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)).ok_or("The SDK returned an invalid object name.")?;
        let num = |key: &str| row[key].as_f64().filter(|n| n.is_finite()).ok_or("The SDK returned invalid object geometry.");
        let obj = Object { name: name.to_owned(), x: num("Xmm")?, y: num("Ymm")?, width: num("WidthMm")?, height: num("HeightMm")?, layer: row["Layer"].as_u64().ok_or("The SDK returned an invalid layer.")? };
        if obj.x.abs() > 10000.0 || obj.y.abs() > 100.0 || obj.width <= 0.0 || obj.width > 10000.0 || obj.height <= 0.0 || obj.height > 100.0 { return Err("The SDK returned invalid object geometry.".into()); }
        if objects.iter().any(|old: &Object| old.name.eq_ignore_ascii_case(&obj.name) || old.layer == obj.layer) { return Err("The SDK returned duplicate objects or layers.".into()); }
        objects.push(obj);
    }
    objects.sort_by_key(|item| item.layer);
    if objects.iter().enumerate().any(|(i, o)| i as u64 != o.layer) { return Err("The SDK object layers are incomplete.".into()); }
    Ok(objects)
}

#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub rect: Rect,
    pub width_mm: f64,
    pub height_mm: f64,
}

impl Surface {
    pub fn valid(&self) -> bool {
        self.rect.is_positive() && self.rect.is_finite() && self.width_mm.is_finite() && self.height_mm.is_finite() && self.width_mm > 0.0 && self.height_mm > 0.0 && self.width_mm <= 10000.0 && self.height_mm <= 100.0
    }
    fn x(&self, x: f64) -> f32 { self.rect.left() + (x / self.width_mm) as f32 * self.rect.width() }
    fn y(&self, y: f64) -> f32 { self.rect.top() + (y / self.height_mm) as f32 * self.rect.height() }
    fn object_rect(&self, obj: &Object) -> Rect { Rect::from_min_max(egui::pos2(self.x(obj.x), self.y(obj.y)), egui::pos2(self.x(obj.x + obj.width), self.y(obj.y + obj.height))) }
    fn delta_mm(&self, delta: Vec2) -> (f64, f64) { (f64::from(delta.x / self.rect.width()) * self.width_mm, f64::from(delta.y / self.rect.height()) * self.height_mm) }
}

/// Move pixels from the last SDK preview while the real preview is being
/// regenerated. The destination is provisional and is never used for print.
pub fn paint_move(ui: &egui::Ui, surface: Surface, label_rect: Rect, texture: egui::TextureId, paper: Color32, visual: &MoveVisual, objects: &[Object]) {
    if !surface.valid() || !label_rect.is_positive() || collision_at(objects, &visual.object, visual.object.x, visual.object.y, |_, _| false).is_some() || collision_at(objects, &visual.object, visual.x, visual.y, |_, _| false).is_some() { return; }
    // A rendered border can extend beyond the SDK's geometric rectangle.
    // Include a small screen-space margin when it will not erase a neighbour.
    let exact = surface.object_rect(&visual.object).intersect(label_rect);
    let expanded = exact.expand(1.5).intersect(label_rect);
    let near_neighbour = objects.iter().filter(|other| !other.name.eq_ignore_ascii_case(&visual.object.name)).any(|other| surface.object_rect(other).intersects(expanded));
    let source = if near_neighbour { exact } else { expanded };
    if !source.is_positive() { return; }
    let delta = Vec2::new(((visual.x - visual.object.x) / surface.width_mm) as f32 * surface.rect.width(), ((visual.y - visual.object.y) / surface.height_mm) as f32 * surface.rect.height());
    let uv = Rect::from_min_max(egui::pos2((source.left() - label_rect.left()) / label_rect.width(), (source.top() - label_rect.top()) / label_rect.height()), egui::pos2((source.right() - label_rect.left()) / label_rect.width(), (source.bottom() - label_rect.top()) / label_rect.height()));
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(label_rect));
    painter.rect_filled(source, 0.0, paper);
    painter.image(texture, source.translate(delta), uv, Color32::WHITE);
}

#[derive(Clone, Debug)]
struct Drag {
    object: Object,
    surface: Surface,
    pointer_origin: Pos2,
    current: (f64, f64),
    guide_x: Option<f64>,
    guide_y: Option<f64>,
}

#[derive(Default)]
pub struct CanvasInteraction { drag: Option<Drag> }

#[derive(Default, Debug, PartialEq)]
pub struct CanvasAction {
    /// Single selection; the caller updates its layout selection.
    pub select: Option<String>,
    /// A drag is committed only on pointer release. Invalid results must never
    /// be used to print until the SDK has successfully rendered them.
    pub move_to: Option<(String, f64, f64)>,
}

pub(crate) fn topmost_at(objects: &[Object], surface: Surface, point: Pos2) -> Option<&Object> {
    if !surface.rect.contains(point) { return None; }
    objects.iter().rev().find(|o| surface.object_rect(o).intersect(surface.rect).contains(point))
}

/// Snap an object's edges/centre to other edges/centres, or to the canvas.
/// The threshold is specified in screen pixels so zoom doesn't make snapping
/// too sensitive. The nearest alignment wins deterministically.
fn snap_axis(position: f64, extent: f64, limit: f64, others: impl Iterator<Item=(f64, f64)>, threshold: f64) -> (f64, Option<f64>) {
    let maximum = (limit - extent).max(0.0);
    let position = position.clamp(0.0, maximum);
    let offsets = [0.0, extent / 2.0, extent];
    let mut target = vec![0.0, limit / 2.0, limit];
    for (start, size) in others { target.extend([start, start + size / 2.0, start + size]); }
    let mut best: Option<(f64, f64, f64)> = None;
    for anchor in target {
        if !anchor.is_finite() { continue; }
        for offset in offsets {
            let candidate = anchor - offset;
            if candidate < 0.0 || candidate > maximum { continue; }
            let distance = (candidate - position).abs();
            if distance <= threshold && best.as_ref().is_none_or(|old| distance < old.0) { best = Some((distance, candidate, anchor)); }
        }
    }
    match best { Some((_, candidate, anchor)) => (candidate, Some(anchor)), None => (position, None) }
}

impl CanvasInteraction {
    pub fn cancel(&mut self) { self.drag = None; }
    pub fn is_dragging(&self) -> bool { self.drag.is_some() }

    /// Draw selection outlines and alignment guides over the already-painted
    /// preview. `response` must use `Sense::click_and_drag()` over the label;
    /// clipping is taken from the current scrolling UI and printable bounds.
    pub fn show(&mut self, ui: &egui::Ui, response: &Response, surface: Surface, objects: &[Object], selected: &[String], locked: bool, guides: &crate::guides::Guides, image: Option<(Rect, egui::TextureId, Color32)>) -> CanvasAction {
        let mut action = CanvasAction::default();
        if !surface.valid() { self.cancel(); return action; }
        if locked { self.cancel(); }
        if self.drag.as_ref().is_some_and(|drag| drag.surface.rect != surface.rect || drag.surface.width_mm != surface.width_mm || drag.surface.height_mm != surface.height_mm) { self.cancel(); }
        if response.drag_started_by(egui::PointerButton::Primary) && !locked {
            if let Some(origin) = ui.input(|input| input.pointer.press_origin()) {
                if let Some(object) = topmost_at(objects, surface, origin) {
                    action.select = Some(object.name.clone());
                    self.drag = Some(Drag { object: object.clone(), surface, pointer_origin: origin, current: (object.x, object.y), guide_x: None, guide_y: None });
                }
            }
        }
        if response.clicked_by(egui::PointerButton::Primary) {
            if let Some(point) = response.interact_pointer_pos() { if let Some(object) = topmost_at(objects, surface, point) { action.select = Some(object.name.clone()); } }
        }
        if let Some(drag) = &mut self.drag {
            if let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) {
                let (dx, dy) = surface.delta_mm(pointer - drag.pointer_origin);
                let tol_x = surface.width_mm * 5.0 / f64::from(surface.rect.width());
                let tol_y = surface.height_mm * 5.0 / f64::from(surface.rect.height());
                let (x, gx) = if guides.snapping_enabled() { snap_axis(drag.object.x + dx, drag.object.width, surface.width_mm, objects.iter().filter(|o| o.name != drag.object.name).map(|o| (o.x, o.width)), tol_x) } else { ((drag.object.x + dx).clamp(0.0, (surface.width_mm - drag.object.width).max(0.0)), None) };
                let (y, gy) = if guides.snapping_enabled() { snap_axis(drag.object.y + dy, drag.object.height, surface.height_mm, objects.iter().filter(|o| o.name != drag.object.name).map(|o| (o.y, o.height)), tol_y) } else { ((drag.object.y + dy).clamp(0.0, (surface.height_mm - drag.object.height).max(0.0)), None) };
                let (final_x, final_y) = guides.snap_position(x, y, drag.object.width, drag.object.height, surface.width_mm, surface.height_mm, tol_x, tol_y);
                drag.current = (final_x, final_y);
                drag.guide_x = if (final_x - x).abs() < 1e-8 { gx } else { None };
                drag.guide_y = if (final_y - y).abs() < 1e-8 { gy } else { None };
            }
        }
        if let (Some(drag), Some((rect, texture, paper))) = (&self.drag, image) {
            paint_move(ui, surface, rect, texture, paper, &MoveVisual { object: drag.object.clone(), x: drag.current.0, y: drag.current.1 }, objects);
        }
        let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(surface.rect));
        if guides.is_visible() {
            let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(20, 150, 210, 120));
            painter.line_segment([egui::pos2(surface.rect.center().x, surface.rect.top()), egui::pos2(surface.rect.center().x, surface.rect.bottom())], stroke);
            painter.line_segment([egui::pos2(surface.rect.left(), surface.rect.center().y), egui::pos2(surface.rect.right(), surface.rect.center().y)], stroke);
        }
        for object in objects {
            let is_dragged = self.drag.as_ref().is_some_and(|drag| drag.object.name == object.name);
            let chosen = selected.iter().any(|name| name == &object.name) || action.select.as_ref() == Some(&object.name) || is_dragged;
            let colour = if chosen { Color32::from_rgb(30, 115, 235) } else { Color32::from_rgba_unmultiplied(45, 80, 110, 100) };
            let rect = surface.object_rect(object);
            painter.rect_stroke(rect, 0.0, Stroke::new(if chosen { 1.5 } else { 1.0 }, colour), StrokeKind::Inside);
        }
        if let Some(drag) = &self.drag {
            let mut moving = drag.object.clone(); moving.x = drag.current.0; moving.y = drag.current.1;
            let rect = surface.object_rect(&moving);
            painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(36, 125, 225, 38));
            painter.rect_stroke(rect, 0.0, Stroke::new(2.0, Color32::from_rgb(20, 115, 235)), StrokeKind::Inside);
            let stroke = Stroke::new(1.5, Color32::from_rgb(245, 65, 110));
            if let Some(x) = drag.guide_x { let x = surface.x(x); painter.line_segment([egui::pos2(x, surface.rect.top()), egui::pos2(x, surface.rect.bottom())], stroke); }
            if let Some(y) = drag.guide_y { let y = surface.y(y); painter.line_segment([egui::pos2(surface.rect.left(), y), egui::pos2(surface.rect.right(), y)], stroke); }
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) {
            if let Some(drag) = self.drag.take() {
                if (drag.current.0 - drag.object.x).abs() > 1e-5 || (drag.current.1 - drag.object.y).abs() > 1e-5 { action.move_to = Some((drag.object.name, drag.current.0, drag.current.1)); }
            }
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn object(name: &str, x: f64, y: f64, layer: u64) -> Object { Object { name: name.into(), x, y, width: 4.0, height: 2.0, layer } }
    fn surface() -> Surface { Surface { rect: Rect::from_min_size(egui::pos2(150.0, 50.0), egui::vec2(400.0, 100.0)), width_mm: 40.0, height_mm: 10.0 } }
    #[test] fn screen_mapping_accounts_for_scrolling_and_zoom() {
        let s = surface(); let rect = s.object_rect(&object("one", 10.0, 2.0, 0));
        assert_eq!(rect.min, egui::pos2(250.0, 70.0));
        assert_eq!(rect.size(), egui::vec2(40.0, 20.0));
        let (dx, dy) = s.delta_mm(egui::vec2(20.0, -10.0));
        assert!((dx - 2.0).abs() < 1e-6 && (dy + 1.0).abs() < 1e-6);
    }
    #[test] fn hit_test_uses_topmost_layer_and_clips_to_canvas() {
        let s = surface(); let objects = vec![object("back", 1.0, 1.0, 0), object("front", 1.0, 1.0, 1)];
        assert_eq!(topmost_at(&objects, s, egui::pos2(165.0, 65.0)).unwrap().name, "front");
        assert!(topmost_at(&objects, s, egui::pos2(149.0, 65.0)).is_none());
    }
    #[test] fn drop_collision_preserves_touching_and_explicitly_allowed_positions() {
        let objects = vec![object("text", 0.0, 0.0, 0), object("shape", 5.0, 0.0, 1)];
        assert_eq!(collision_at(&objects, &objects[1], 3.0, 0.0, |_, _| false), Some("text"));
        assert_eq!(collision_at(&objects, &objects[1], 4.0, 0.0, |_, _| false), None);
        assert_eq!(collision_at(&objects, &objects[1], 3.0, 0.0, |_, _| true), None);
    }
    #[test] fn snapping_prefers_nearest_and_clamps_to_canvas() {
        let (x, guide) = snap_axis(10.15, 4.0, 40.0, std::iter::once((14.0, 4.0)), 0.3);
        assert!((x - 10.0).abs() < 1e-8); assert_eq!(guide, Some(14.0));
        assert_eq!(snap_axis(-5.0, 4.0, 40.0, std::iter::empty(), 0.1).0, 0.0);
        assert_eq!(snap_axis(100.0, 4.0, 40.0, std::iter::empty(), 0.1).0, 36.0);
    }
    #[test] fn snapshot_requires_finite_geometry_and_unique_names_and_layers() {
        let input = json!([{"Name":"b","Xmm":4,"Ymm":0,"WidthMm":4,"HeightMm":2,"Layer":1}, {"Name":"a","Xmm":0,"Ymm":0,"WidthMm":4,"HeightMm":2,"Layer":0}]);
        let objects = objects_from_snapshot(&input).unwrap();
        assert_eq!(objects[0].name, "a");
        let mut duplicate = input.clone(); duplicate[1]["Name"] = json!("B"); assert!(objects_from_snapshot(&duplicate).is_err());
        let mut missing = input; missing[1]["WidthMm"] = Value::Null; assert!(objects_from_snapshot(&missing).is_err());
    }
    #[test] fn pointer_drag_selects_and_commits_only_on_release() {
        let ctx = egui::Context::default();
        let mut interaction = CanvasInteraction::default();
        let mut guides = crate::guides::Guides::default(); guides.set_snapping(false);
        let objects = vec![object("one", 1.0, 1.0, 0)];
        let mut origin = Pos2::ZERO;
        let mut frame = |events: Vec<egui::Event>| {
            let mut action = CanvasAction::default();
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))), events, ..Default::default() };
            let _ = ctx.run_ui(input, |ui| {
                let (rect, response) = ui.allocate_exact_size(egui::vec2(400.0, 100.0), egui::Sense::click_and_drag());
                origin = rect.min;
                let surface = Surface { rect, width_mm: 40.0, height_mm: 10.0 };
                action = interaction.show(ui, &response, surface, &objects, &[], false, &guides, None);
            });
            action
        };
        frame(vec![]);
        // End the mutable borrow before using the observed rectangle.
        drop(frame);
        let start = origin + egui::vec2(15.0, 15.0);
        let mut frame = |events: Vec<egui::Event>| {
            let mut action = CanvasAction::default();
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))), events, ..Default::default() };
            let _ = ctx.run_ui(input, |ui| {
                let (rect, response) = ui.allocate_exact_size(egui::vec2(400.0, 100.0), egui::Sense::click_and_drag());
                action = interaction.show(ui, &response, Surface { rect, width_mm: 40.0, height_mm: 10.0 }, &objects, &[], false, &guides, None);
            });
            action
        };
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        assert!(frame(vec![egui::Event::PointerMoved(start), button(start, true)]).move_to.is_none());
        let end = start + egui::vec2(20.0, 0.0);
        let dragged = frame(vec![egui::Event::PointerMoved(end)]);
        assert_eq!(dragged.select.as_deref(), Some("one"));
        assert!(dragged.move_to.is_none());
        let released = frame(vec![button(end, false)]);
        let (name, x, y) = released.move_to.expect("pointer release must commit the drag");
        assert_eq!(name, "one"); assert!((x - 3.0).abs() < 1e-5 && (y - 1.0).abs() < 1e-5);
    }
}
