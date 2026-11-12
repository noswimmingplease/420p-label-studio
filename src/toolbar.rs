//! Menus and tool rail for the single-label editor.
//!
//! Commands which depend on an independent object model or other unavailable
//! features stay disabled; their labels are retained to make the gaps clear.
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    LayoutPanel,
    GuidesPanel,
    ToggleGuides,
    ToggleGuideSnap,
    ToggleLayoutLock,
    LayoutOrder(crate::layout_tools::Order),
    LayoutAlignObjects(crate::layout_tools::Alignment),
    LayoutAlignLabel(crate::layout_tools::Alignment),
    New,
    Open,
    Save,
    SaveAs,
    ExportDymo,
    Preferences,
    ImportTemplate,
    ImportData,
    ManageContacts,
    Print,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    ToggleBold,
    ToggleItalic,
    ToggleUnderline,
    Text,
    Image,
    Address,
    Shape,
    Line,
    Barcode,
    Qr,
    Date,
    Counter,
    AlignLeft,
    AlignCentre,
    AlignRight,
    ZoomIn,
    ZoomOut,
    Fit,
    Zoom100,
    ToggleRulers,
    UnitsMetric,
    UnitsInches,
    CheckUpdates,
    CheckAppUpdates,
    HelpContents,
    UserGuides,
    About,
    QuickTips,
    BuyLabels,
    Register,
    Exit,
}

/// States which the parent can currently execute and reflect in the menus.
#[derive(Default)]
pub struct ToolbarState {
    pub layout_locked: bool,
    pub can_order: bool,
    pub can_align_objects: bool,
    pub can_align_label: bool,
    pub guides_visible: bool,
    pub guide_snap: bool,
    pub can_print: bool,
    pub has_text: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub rulers: bool,
    pub can_toggle_rulers: bool,
    pub can_change_units: bool,
    pub metric: bool,
    pub can_fit: bool,
    pub can_show_about: bool,
    pub can_save: bool,
    pub can_export: bool,
}

fn command(ui: &mut egui::Ui, label: &str, enabled: bool, why: &str, action: Action, out: &mut Option<Action>) {
    let response = ui.add_enabled(enabled, egui::Button::new(label));
    let response = if enabled { response } else { response.on_disabled_hover_text(why) };
    if response.clicked() { *out = Some(action); ui.close(); }
}

fn unsupported(ui: &mut egui::Ui, label: &str, why: &str) {
    ui.add_enabled(false, egui::Button::new(label)).on_disabled_hover_text(why);
}

/// Draw the six DYMO-style top menus and return the selected command, if any.
pub fn menu_bar(ui: &mut egui::Ui, state: &ToolbarState) -> Option<Action> {
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        ui.menu_button("File", |ui| {
            command(ui, "New                 Ctrl+N", true, "", Action::New, &mut action);
            command(ui, "Open…               Ctrl+O", true, "", Action::Open, &mut action);
            command(ui, "Save                 Ctrl+S", state.can_save, "Open or save a label first.", Action::Save, &mut action);
            command(ui, "Save as…      Ctrl+Shift+S", true, "", Action::SaveAs, &mut action);
            command(ui, "Export DYMO label…", state.can_export, "Generate a current label preview first.", Action::ExportDymo, &mut action);
            ui.separator();
            command(ui, "Import data…", true, "", Action::ImportData, &mut action);
            command(ui, "Import custom label…", true, "", Action::ImportTemplate, &mut action);
            command(ui, "Manage contacts…", true, "", Action::ManageContacts, &mut action);
            command(ui, "Settings…", true, "", Action::Preferences, &mut action);
            unsupported(ui, "Add network printer", "This app is limited to the USB LabelManager 420P.");
            ui.separator();
            command(ui, "Print                  Ctrl+P", state.can_print, "Connect the 420P and generate a current preview first.", Action::Print, &mut action);
            command(ui, "Exit", true, "", Action::Exit, &mut action);
        });
        ui.menu_button("Edit", |ui| {
            command(ui, "Undo", state.can_undo, "Nothing to undo.", Action::Undo, &mut action);
            command(ui, "Redo", state.can_redo, "Nothing to redo.", Action::Redo, &mut action);
            ui.separator();
            command(ui, "Cut", state.has_selection, "Select text first.", Action::Cut, &mut action);
            command(ui, "Copy", state.has_selection, "Select text first.", Action::Copy, &mut action);
            command(ui, "Paste", true, "", Action::Paste, &mut action);
            command(ui, "Select all", state.has_text, "There is no text to select.", Action::SelectAll, &mut action);
            ui.separator();
            if ui.selectable_label(state.bold, "Bold").clicked() { action = Some(Action::ToggleBold); ui.close(); }
            if ui.selectable_label(state.italic, "Italic").clicked() { action = Some(Action::ToggleItalic); ui.close(); }
            if ui.selectable_label(state.underline, "Underline").clicked() { action = Some(Action::ToggleUnderline); ui.close(); }
        });
        ui.menu_button("Add", |ui| {
            command(ui, "Text", true, "", Action::Text, &mut action);
            command(ui, "Address", true, "", Action::Address, &mut action);
            command(ui, "Image…", true, "", Action::Image, &mut action);
            command(ui, "Shape", true, "", Action::Shape, &mut action);
            command(ui, "Line", true, "", Action::Line, &mut action);
            command(ui, "Barcode", true, "", Action::Barcode, &mut action);
            command(ui, "QR Code", true, "", Action::Qr, &mut action);
            command(ui, "Date and Time", true, "", Action::Date, &mut action);
            command(ui, "Counter", true, "", Action::Counter, &mut action);
        });
        ui.menu_button("Layout", |ui| {
            use crate::layout_tools::{Alignment, Order};
            command(ui, "Object layout…", true, "", Action::LayoutPanel, &mut action);
            for (label, order) in [("Bring to front", Order::Front), ("Bring forward", Order::Forward), ("Send backward", Order::Backward), ("Send to back", Order::Back)] {
                command(ui, label, state.can_order, "Enable manual layout and select one object.", Action::LayoutOrder(order), &mut action);
            }
            ui.menu_button("Align objects", |ui| {
                for (label, direction) in [("Left", Alignment::Left), ("Centre", Alignment::Centre), ("Right", Alignment::Right), ("Top", Alignment::Top), ("Middle", Alignment::Middle), ("Bottom", Alignment::Bottom)] {
                    command(ui, label, state.can_align_objects, "Select at least two objects in manual layout.", Action::LayoutAlignObjects(direction), &mut action);
                }
            });
            ui.menu_button("Align to label", |ui| {
                for (label, direction) in [("Left", Alignment::Left), ("Centre", Alignment::Centre), ("Right", Alignment::Right), ("Top", Alignment::Top), ("Middle", Alignment::Middle), ("Bottom", Alignment::Bottom)] {
                    command(ui, label, state.can_align_label, "Select an object in manual layout and generate a preview.", Action::LayoutAlignLabel(direction), &mut action);
                }
            });
            ui.menu_button("Text alignment", |ui| {
                command(ui, "Left", true, "", Action::AlignLeft, &mut action);
                command(ui, "Centre", true, "", Action::AlignCentre, &mut action);
                command(ui, "Right", true, "", Action::AlignRight, &mut action);
            });
            if ui.selectable_label(state.layout_locked, "Lock layout").clicked() { action = Some(Action::ToggleLayoutLock); ui.close(); }
        });
        ui.menu_button("View", |ui| {
            command(ui, "Zoom in", true, "", Action::ZoomIn, &mut action);
            command(ui, "Zoom out", true, "", Action::ZoomOut, &mut action);
            command(ui, "Fit to design area", state.can_fit, "Fit is not available yet.", Action::Fit, &mut action);
            command(ui, "100%", true, "", Action::Zoom100, &mut action);
            ui.separator();
            ui.menu_button("Rulers and guides", |ui| {
                let rulers = ui.add_enabled(state.can_toggle_rulers, egui::Button::selectable(state.rulers, "Show rulers"));
                if rulers.on_disabled_hover_text("Ruler visibility cannot be changed yet.").clicked() { action = Some(Action::ToggleRulers); ui.close(); }
                if ui.selectable_label(state.guides_visible, "Show guides").clicked() { action = Some(Action::ToggleGuides); ui.close(); }
                if ui.selectable_label(state.guide_snap, "Snap to guides and objects").clicked() { action = Some(Action::ToggleGuideSnap); ui.close(); }
                command(ui, "Edit guides…", true, "", Action::GuidesPanel, &mut action);
            });
            ui.menu_button("Units of measure", |ui| {
                let metric = ui.add_enabled(state.can_change_units, egui::Button::selectable(state.metric, "Metric"));
                if metric.on_disabled_hover_text("Unit selection is not available yet.").clicked() { action = Some(Action::UnitsMetric); ui.close(); }
                let inches = ui.add_enabled(state.can_change_units, egui::Button::selectable(!state.metric, "Inches"));
                if inches.on_disabled_hover_text("Unit selection is not available yet.").clicked() { action = Some(Action::UnitsInches); ui.close(); }
            });
        });
        ui.menu_button("Help", |ui| {
            command(ui, "Help contents                 F1", true, "", Action::HelpContents, &mut action);
            command(ui, "Quick tips", true, "", Action::QuickTips, &mut action);
            command(ui, "User guides", true, "", Action::UserGuides, &mut action);
            command(ui, "Buy labels", true, "", Action::BuyLabels, &mut action);
            command(ui, "Check app updates", true, "", Action::CheckAppUpdates, &mut action);
            command(ui, "Check DYMO Connect updates", true, "", Action::CheckUpdates, &mut action);
            command(ui, "Register", true, "", Action::Register, &mut action);
            command(ui, "About", state.can_show_about, "About information is not available yet.", Action::About, &mut action);
        });
    });
    action
}

#[derive(Clone, Copy)]
enum Glyph { Text, Address, Image, Shape, Line, Barcode, Qr, Date, Counter, More }

fn draw_glyph(ui: &egui::Ui, rect: Rect, glyph: Glyph, enabled: bool) {
    let colour = if enabled { Color32::from_gray(215) } else { Color32::from_gray(105) };
    let stroke = Stroke::new(1.6, colour);
    let p = ui.painter();
    let c = rect.center();
    let point = |x: f32, y: f32| c + Vec2::new(x, y);
    let line = |a: Pos2, b: Pos2| p.line_segment([a, b], stroke);
    match glyph {
        Glyph::Text => { p.text(c, egui::Align2::CENTER_CENTER, "T", egui::FontId::proportional(24.0), colour); }
        Glyph::Address => {
            p.rect_stroke(Rect::from_center_size(c, Vec2::new(21.0, 14.0)), 1.0, stroke, egui::StrokeKind::Inside);
            line(point(-10.0, -7.0), point(0.0, 1.0)); line(point(0.0, 1.0), point(10.0, -7.0));
        }
        Glyph::Image => {
            p.rect_stroke(Rect::from_center_size(c, Vec2::splat(20.0)), 1.0, stroke, egui::StrokeKind::Inside);
            p.circle_filled(point(5.0, -5.0), 2.0, colour);
            line(point(-7.0, 6.0), point(-2.0, 0.0)); line(point(-2.0, 0.0), point(3.0, 6.0)); line(point(3.0, 6.0), point(6.0, 3.0));
        }
        Glyph::Shape => {
            p.circle_stroke(point(-3.0, -3.0), 7.0, stroke);
            p.rect_stroke(Rect::from_center_size(point(4.0, 4.0), Vec2::splat(13.0)), 1.0, stroke, egui::StrokeKind::Inside);
        }
        Glyph::Line => { line(point(-8.0, 8.0), point(8.0, -8.0)); }
        Glyph::Barcode => { for (x, width) in [(-9.0, 2.0), (-5.0, 1.0), (-2.0, 2.0), (2.0, 1.0), (5.0, 2.0), (9.0, 1.0)] { p.line_segment([point(x, -9.0), point(x, 9.0)], Stroke::new(width, colour)); } }
        Glyph::Qr => {
            for (x, y) in [(-7.0, -7.0), (5.0, -7.0), (-7.0, 5.0)] {
                p.rect_stroke(Rect::from_center_size(point(x, y), Vec2::splat(6.0)), 0.0, stroke, egui::StrokeKind::Inside);
            }
            for (x, y) in [(1.0, 2.0), (5.0, 5.0), (9.0, 2.0), (1.0, 9.0), (9.0, 9.0)] { p.rect_filled(Rect::from_center_size(point(x, y), Vec2::splat(2.0)), 0.0, colour); }
        }
        Glyph::Date => { p.circle_stroke(c, 10.0, stroke); line(c, point(0.0, -6.0)); line(c, point(5.0, 2.0)); }
        Glyph::Counter => { p.text(c, egui::Align2::CENTER_CENTER, "#", egui::FontId::proportional(22.0), colour); }
        Glyph::More => { for x in [-6.0, 0.0, 6.0] { p.circle_filled(point(x, 0.0), 1.8, colour); } }
    }
}

/// Draw the object tools in the order used by DYMO Connect.
pub fn object_rail(ui: &mut egui::Ui) -> Option<Action> {
    let mut action = None;
    let entries = [
        ("Text", Glyph::Text, Some(Action::Text), ""),
        ("Address", Glyph::Address, Some(Action::Address), ""),
        ("Image", Glyph::Image, Some(Action::Image), ""),
        ("Shape", Glyph::Shape, Some(Action::Shape), ""),
        ("Line", Glyph::Line, Some(Action::Line), ""),
        ("Barcode", Glyph::Barcode, Some(Action::Barcode), ""),
        ("QR Code", Glyph::Qr, Some(Action::Qr), ""),
        ("Date and Time", Glyph::Date, Some(Action::Date), ""),
        ("Counter", Glyph::Counter, Some(Action::Counter), ""),
        ("More objects", Glyph::More, None, "Additional objects are not implemented yet."),
    ];
    ui.vertical(|ui| {
        for (label, glyph, command, why) in entries {
            let enabled = command.is_some();
            let response = ui.add_enabled(enabled, egui::Button::new("").min_size(Vec2::splat(36.0)).sense(Sense::click()));
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
            draw_glyph(ui, response.rect, glyph, enabled);
            let response = if enabled { response.on_hover_text(label) } else { response.on_disabled_hover_text(why) };
            if response.clicked() { action = command; }
        }
    });
    action
}
