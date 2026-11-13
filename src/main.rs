#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, RichText, Vec2};
mod dymo_service;
mod sdk;
mod app_updates;
mod update_package;
mod updater;
mod branding;
mod toolbar;
mod features;
mod project_io;
mod sequence;
mod data_tools;
mod batch;
mod layout_tools;
mod canvas_layout;
mod guides;
#[allow(dead_code)]
mod monochrome;

enum PendingDocumentAction { New, Open(std::path::PathBuf), Exit, InstallUpdate }
enum PrintEvent { Single(Result<(), String>), Progress(batch::Progress), BatchDone(batch::Outcome) }
struct PreviewAttempt { layout: Option<layout_tools::LayoutState>, preview: Result<sdk::Preview, String> }

#[derive(Clone, Debug, PartialEq)]
enum PreviewContextTarget { PrimaryText(String), Image(String), Feature(String), TemplateObject(String) }
impl PreviewContextTarget {
    fn name(&self) -> &str { match self { Self::PrimaryText(name) | Self::Image(name) | Self::Feature(name) | Self::TemplateObject(name) => name } }
}

struct LabelApp {
    app_update_repository: String,
    app_update_status: String,
    app_update_release: Option<app_updates::Release>,
    app_update_receiver: Option<std::sync::mpsc::Receiver<(String, Result<app_updates::Release, String>)>>,
    app_update_download: Option<std::sync::mpsc::Receiver<update_package::DownloadEvent>>,
    app_update_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    app_update_prepared: Option<update_package::Prepared>,
    app_update_progress: Option<(u64, u64)>,
    app_update_install_receiver: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    app_update_restarting: bool,
    app_update_acknowledge: Option<String>,
    app_update_exit_after_download: bool,
    app_update_startup_checked: bool,
    app_update_dismissed_version: Option<String>,
    app_update_startup_message: Option<String>,
    app_update_scroll_requested: bool,
    model_picture: Option<egui::TextureHandle>,
    export_receiver: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    text: String,
    width_mm: u8,
    copies: u16,
    service_status: String,
    status_receiver: Option<std::sync::mpsc::Receiver<Result<sdk::DeviceStatus, String>>>,
    device_status: Option<sdk::DeviceStatus>,
    device_status_time: Option<std::time::Instant>,
    template_path: String,
    preview_receiver: Option<std::sync::mpsc::Receiver<Result<PreviewAttempt, String>>>,
    preview_image: Option<egui::TextureHandle>,
    highres_design: Option<egui::TextureHandle>,
    preview_dimensions: Option<(f64, f64)>,
    printable_dimensions: Option<(f64, f64)>,
    printable_origin: Option<(f64, f64)>,
    preview_snapshot: Option<std::sync::Arc<sdk::PrintSnapshot>>,
    preview_objects: Option<Vec<serde_json::Value>>,
    all_layouts: Option<serde_json::Value>,
    empty_primary_text: Option<String>,
    pending_move: Option<canvas_layout::MoveVisual>,
    preview_paper: Color32,
    design_mode: bool,
    design_image: Option<egui::TextureHandle>,
    design_image_key: String,
    design_image_receiver: Option<std::sync::mpsc::Receiver<(String, Result<image::RgbaImage, String>)>>,
    right_click_selection: Option<egui::text::CCursorRange>,
    preview_context_target: Option<PreviewContextTarget>,
    show_rulers: bool,
    metric_units: bool,
    show_about: bool,
    show_preferences: bool,
    show_tips: bool,
    show_layout: bool,
    show_guides: bool,
    fit_requested: bool,
    features: features::Features,
    layout: layout_tools::LayoutState,
    guides: guides::Guides,
    canvas: canvas_layout::CanvasInteraction,
    known_features: Vec<String>,
    known_image: bool,
    removed_objects: Vec<String>,
    object_layouts: Vec<serde_json::Value>,
    project_path: Option<std::path::PathBuf>,
    saved_disk_snapshot: Option<Vec<u8>>,
    saved_key: String,
    pending_document_action: Option<PendingDocumentAction>,
    data_tools: data_tools::DataTools,
    prepared_batch: Vec<String>,
    show_batch: bool,
    batch_selected: usize,
    batch_preview_receiver: Option<std::sync::mpsc::Receiver<(String, Result<sdk::Preview, String>)>>,
    batch_preview_key: Option<String>,
    batch_preview_texture: Option<egui::TextureHandle>,
    batch_status: String,
    allow_close: bool,
    preview_status: String,
    preview_base_key: Option<String>,
    threshold_image_blank: bool,
    image_path: String,
    black_and_white: bool,
    threshold: u8,
    preview_key: Option<String>,
    pending_key: Option<String>,
    print_receiver: Option<std::sync::mpsc::Receiver<PrintEvent>>,
    print_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    print_status: String,
    connected: Option<bool>,
    bold: bool,
    italic: bool,
    underline: bool,
    font: String,
    fonts: Vec<String>,
    fonts_receiver: Option<std::sync::mpsc::Receiver<Result<Vec<String>, String>>>,
    align: String,
    next_status_check: std::time::Instant,
    file_status: String,
    observed_key: String,
    preview_due: Option<std::time::Instant>,
    zoom: f32,
    tray: Option<tray_icon::TrayIcon>,
    tray_error: Option<String>,
}

impl Default for LabelApp {
    fn default() -> Self {
        let template_path = built_in_template(9).unwrap_or_default();
        let mut app = Self { app_update_repository: app_updates::DEFAULT_REPOSITORY.to_owned(), app_update_status: "Check GitHub for the latest app release.".into(), app_update_release: None, app_update_receiver: None, app_update_download: None, app_update_cancel: None, app_update_prepared: None, app_update_progress: None, app_update_install_receiver: None, app_update_restarting: false, app_update_acknowledge: None, app_update_exit_after_download: false, app_update_startup_checked: false, app_update_dismissed_version: None, app_update_startup_message: None, app_update_scroll_requested: false, model_picture: None, export_receiver: None, text: String::new(), width_mm: 9, copies: 1, service_status: "Not checked".into(), status_receiver: None, device_status: None, device_status_time: None, template_path, preview_receiver: None, preview_image: None, highres_design: None, preview_dimensions: None, printable_dimensions: None, printable_origin: None, preview_snapshot: None, preview_objects: None, all_layouts: None, empty_primary_text: None, pending_move: None, preview_paper: Color32::WHITE, design_mode: true, design_image: None, design_image_key: String::new(), design_image_receiver: None, right_click_selection: None, preview_context_target: None, show_rulers: true, metric_units: true, show_about: false, show_preferences: false, show_tips: false, show_layout: false, show_guides: false, fit_requested: false, features: features::Features::default(), layout: layout_tools::LayoutState::default(), guides: guides::Guides::default(), canvas: canvas_layout::CanvasInteraction::default(), known_features: Vec::new(), known_image: false, removed_objects: Vec::new(), object_layouts: Vec::new(), project_path: None, saved_disk_snapshot: None, saved_key: String::new(), pending_document_action: None, data_tools: data_tools::DataTools::default(), prepared_batch: Vec::new(), show_batch: false, batch_selected: 0, batch_preview_receiver: None, batch_preview_key: None, batch_preview_texture: None, batch_status: String::new(), allow_close: false, preview_status: "Generate a preview to see the label.".into(), preview_base_key: None, threshold_image_blank: false, image_path: String::new(), black_and_white: true, threshold: 128, preview_key: None, pending_key: None, print_receiver: None, print_cancel: None, print_status: String::new(), connected: None, bold: false, italic: false, underline: false, font: "Segoe UI".into(), fonts: vec!["Segoe UI".into(), "Arial".into(), "Calibri".into()], fonts_receiver: None, align: "Center".into(), next_status_check: std::time::Instant::now(), file_status: String::new(), observed_key: String::new(), preview_due: None, zoom: 2.0, tray: None, tray_error: None };
        app.saved_key = app.document_key();
        app
    }
}

impl LabelApp {
    fn start_status_check(&mut self) {
        if self.status_receiver.is_some() || self.print_receiver.is_some() { return; }
        let (send, receive) = std::sync::mpsc::channel();
        self.status_receiver = Some(receive);
        std::thread::spawn(move || { let _ = send.send(dymo_service::status()); });
    }

    fn apply_device_status(&mut self, result: Result<sdk::DeviceStatus, String>) {
        match result {
            Ok(status) => {
                self.connected = Some(status.connected);
                self.service_status = if status.connected { "Connected" } else { "Disconnected" }.into();
                self.device_status_time = status.available.then(std::time::Instant::now);
                self.device_status = Some(status);
            }
            Err(error) => {
                self.connected = None;
                self.service_status = error;
                self.device_status = None;
                self.device_status_time = None;
            }
        }
    }

    fn device_status_ui(&self, ui: &mut egui::Ui, expanded: bool) {
        if self.connected != Some(true) {
            if expanded { ui.label("Connect the 420P to read battery and cassette details."); }
            return;
        }
        let Some(status) = &self.device_status else { return; };
        if !status.available {
            ui.label(status.message.as_deref().unwrap_or("Printer details are unavailable. Try refreshing the connection."));
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Battery");
            if let Some(percent) = status.battery_percent {
                ui.add(egui::ProgressBar::new(f32::from(percent) / 100.0).desired_width(90.0).text(format!("{percent}%")));
            } else { ui.label("Unavailable"); }
            match status.charging {
                Some(true) => { ui.colored_label(Color32::from_rgb(83, 210, 134), "Charging"); }
                Some(false) => { ui.label("Not charging"); }
                None => { ui.label("Charging status unavailable"); }
            }
            if let Some(width) = status.tape_width_mm { ui.label(format!("Detected tape: {}", tape_width_label(width))); }
        });
        for issue in &status.issues { ui.colored_label(Color32::from_rgb(239, 189, 85), issue); }
        if let Some(width) = status.tape_width_mm.filter(|width| *width != self.width_mm) {
            ui.colored_label(Color32::from_rgb(239, 189, 85), format!("Detected {width} mm tape; {} mm is selected for this label.", self.width_mm));
        }
        if let Some(time) = self.device_status_time {
            if expanded || time.elapsed() > std::time::Duration::from_secs(60) {
                ui.small(format!("Last status read {} seconds ago.", time.elapsed().as_secs()));
            }
        }
    }

    fn start_app_update_check(&mut self) {
        self.start_app_update_check_with(true, app_updates::check);
    }

    fn start_startup_update_check(&mut self) {
        self.start_startup_update_check_with(app_updates::check);
    }

    fn start_startup_update_check_with(&mut self, check: impl FnOnce(&str) -> Result<app_updates::Release, String> + Send + 'static) {
        if self.app_update_startup_checked { return; }
        self.app_update_startup_checked = true;
        if self.app_update_repository.trim().is_empty() { return; }
        self.start_app_update_check_with(false, check);
    }

    fn start_app_update_check_with(&mut self, save_repository: bool, check: impl FnOnce(&str) -> Result<app_updates::Release, String> + Send + 'static) {
        if self.app_update_receiver.is_some() || self.app_update_download.is_some() || self.app_update_restarting { return; }
        let repository = match app_updates::normalise_repository(&self.app_update_repository) {
            Ok(repository) => repository,
            Err(error) => { self.app_update_status = error; return; }
        };
        if save_repository {
            if let Err(error) = app_updates::save_repository(&repository) { self.app_update_status = error; return; }
        }
        self.app_update_repository = repository.clone();
        self.app_update_release = None;
        self.app_update_progress = None;
        self.app_update_status = "Checking the latest stable app release…".into();
        let (send, receive) = std::sync::mpsc::channel();
        self.app_update_receiver = Some(receive);
        std::thread::spawn(move || { let result = check(&repository); let _ = send.send((repository, result)); });
    }

    fn app_update_notice_version(&self) -> Option<&str> {
        self.app_update_release.as_ref().filter(|release| release.newer && self.app_update_dismissed_version.as_deref() != Some(release.version.as_str())).map(|release| release.version.as_str())
    }

    fn view_app_update(&mut self) {
        self.show_preferences = true;
        self.app_update_scroll_requested = true;
        self.app_update_dismissed_version = self.app_update_release.as_ref().map(|release| release.version.clone());
    }

    fn app_update_notice_ui(&mut self, ctx: &egui::Context) {
        let Some(version) = self.app_update_notice_version().map(str::to_owned) else { return; };
        // A foreground notice leaves the editor's geometry unchanged when a
        // background reply arrives, including while an object is being dragged.
        egui::Area::new(egui::Id::new("app-update-notice")).anchor(egui::Align2::RIGHT_TOP, egui::vec2(-20.0, 20.0))
            .order(egui::Order::Foreground).movable(false).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).inner_margin(10).show(ui, |ui| {
                    ui.set_max_width(330.0);
                    ui.strong(format!("Version {version} is available"));
                    ui.horizontal(|ui| {
                        if ui.button("View update").clicked() { self.view_app_update(); }
                        if ui.button("Dismiss").clicked() { self.app_update_dismissed_version = Some(version.clone()); }
                    });
                });
            });
    }

    fn app_updates_ui(&mut self, ui: &mut egui::Ui) {
        ui.strong("App updates");
        ui.label(format!("Installed app version: {}", env!("CARGO_PKG_VERSION")));
        ui.label("GitHub repository");
        let busy = self.app_update_receiver.is_some() || self.app_update_download.is_some() || self.app_update_restarting;
        if ui.add_enabled(!busy, egui::TextEdit::singleline(&mut self.app_update_repository).hint_text("owner/repository").char_limit(180).desired_width(400.0)).changed() {
            self.app_update_release = None;
            self.app_update_prepared = None;
            self.app_update_dismissed_version = None;
            self.app_update_status = "Repository changed. Save it or check for an app update.".into();
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Check app updates")).clicked() { self.start_app_update_check(); }
            if ui.add_enabled(!busy, egui::Button::new("Save repository")).clicked() {
                self.app_update_status = match app_updates::save_repository(&self.app_update_repository) {
                    Ok(()) if self.app_update_repository.trim().is_empty() => "App update repository cleared.".into(),
                    Ok(()) => "App update repository saved locally.".into(),
                    Err(error) => error,
                };
            }
        });
        ui.label(&self.app_update_status);
        if let Some(message) = &self.app_update_startup_message {
            if message != &self.app_update_status { ui.label(message); }
        }
        if self.app_update_download.is_some() {
            if let Some((received, total)) = self.app_update_progress.filter(|(_, total)| *total > 0) {
                ui.add(egui::ProgressBar::new((received as f64 / total as f64).clamp(0.0, 1.0) as f32).text(format!("{:.1} / {:.1} MB", received as f64 / 1_048_576.0, total as f64 / 1_048_576.0)));
            } else { ui.spinner(); }
            if ui.button("Cancel download").clicked() {
                if let Some(cancel) = &self.app_update_cancel { cancel.store(true, std::sync::atomic::Ordering::Release); }
                self.app_update_status = "Cancelling the update download…".into();
            }
        }
        let mut download = false;
        if let Some(release) = &self.app_update_release {
            if release.newer && self.app_update_prepared.as_ref().is_none_or(|prepared| prepared.version != release.version) {
                let label = if self.app_update_prepared.is_some() { "Download latest update" } else { "Download update" };
                download = ui.add_enabled(!busy && release.package.is_some(), egui::Button::new(label)).clicked();
                if release.package.is_none() { ui.small("Automatic installation requires an official Windows release with verified package and checksum assets."); }
            }
            ui.hyperlink_to("Open release and downloads", &release.url);
            if !release.notes.is_empty() {
                ui.collapsing("Release notes", |ui| { egui::ScrollArea::vertical().id_salt("app-release-notes").max_height(140.0).show(ui, |ui| { ui.label(&release.notes); }); });
            }
        }
        if download { self.start_app_update_download(); }
        if let Some(prepared) = &self.app_update_prepared {
            ui.label(format!("Version {} is downloaded and verified.", prepared.version));
            let ready = !busy && !self.update_printer_work_busy();
            if ui.add_enabled(ready, egui::Button::new("Restart and install")).on_disabled_hover_text("Wait for printer operations and exports to finish.").clicked() {
                self.request_document_action(PendingDocumentAction::InstallUpdate, ui.ctx());
            }
        }
        ui.small("Downloads are verified before installation. Save your work before restarting. The previous version is kept locally for recovery.");
    }

    fn update_printer_work_busy(&self) -> bool {
        self.print_receiver.is_some() || self.export_receiver.is_some() || self.preview_receiver.is_some()
            || self.batch_preview_receiver.is_some() || self.status_receiver.is_some() || self.fonts_receiver.is_some()
    }

    fn start_app_update_download(&mut self) {
        if self.app_update_download.is_some() || self.app_update_restarting { return; }
        let Some(package) = self.app_update_release.as_ref().filter(|release| release.newer).and_then(|release| release.package.clone()) else { return; };
        let install = match update_package::install_directory() { Ok(path) => path, Err(error) => { self.app_update_status = error; return; } };
        let (send, receive) = std::sync::mpsc::channel();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.app_update_cancel = Some(cancel.clone()); self.app_update_download = Some(receive);
        self.app_update_prepared = None; self.app_update_progress = None;
        self.app_update_status = "Preparing the verified update download…".into();
        std::thread::spawn(move || {
            let result = update_package::prepare(&package, &install, &cancel, |message, done, total| { let _ = send.send(update_package::DownloadEvent::Progress(message, done, total)); });
            let _ = send.send(update_package::DownloadEvent::Finished(result));
        });
    }

    fn poll_app_update_download(&mut self, ctx: &egui::Context) {
        for _ in 0..64 {
            let Some(receiver) = &self.app_update_download else { break; };
            match receiver.try_recv() {
                Ok(update_package::DownloadEvent::Progress(message, done, total)) => {
                    if !self.app_update_cancel.as_ref().is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Acquire)) { self.app_update_status = message; }
                    self.app_update_progress = Some((done, total));
                }
                Ok(update_package::DownloadEvent::Finished(result)) => {
                    let cancelled = self.app_update_cancel.as_ref().is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Acquire));
                    match result {
                        Ok(prepared) if !cancelled => { self.app_update_status = "Update verified. Save your work, then restart to install it.".into(); self.app_update_prepared = Some(prepared); }
                        Ok(_) => self.app_update_status = "Update download cancelled.".into(),
                        Err(error) => self.app_update_status = error,
                    }
                    self.app_update_download = None; self.app_update_cancel = None; self.app_update_progress = None;
                    if self.app_update_exit_after_download { self.app_update_exit_after_download = false; self.request_document_action(PendingDocumentAction::Exit, ctx); }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.app_update_download = None; self.app_update_cancel = None; self.app_update_progress = None;
                    self.app_update_status = "The update download stopped unexpectedly. Nothing was installed.".into();
                    if self.app_update_exit_after_download { self.app_update_exit_after_download = false; self.request_document_action(PendingDocumentAction::Exit, ctx); }
                }
            }
        }
        if self.app_update_download.is_some() { ctx.request_repaint_after(std::time::Duration::from_millis(200)); }
    }

    fn can_export(&self) -> bool {
        self.preview_key.as_ref() == Some(&self.key()) && self.preview_receiver.is_none()
            && !self.canvas.is_dragging() && self.preview_snapshot.is_some()
            && self.preview_objects.is_some() && self.export_receiver.is_none()
    }

    fn export_dymo(&mut self) {
        if !self.can_export() { return; }
        let Some(path) = rfd::FileDialog::new().add_filter("DYMO labels", &["dymo"]).set_file_name("label.dymo").save_file() else { return; };
        let original = match destination_contents(&path) {
            Ok(original) => original,
            Err(error) => { self.file_status = error; return; }
        };
        let snapshot = self.preview_snapshot.clone().expect("export requires a current snapshot");
        let objects = self.preview_objects.clone().expect("export requires current objects");
        let text = self.text.clone();
        let (send, receive) = std::sync::mpsc::channel();
        self.export_receiver = Some(receive);
        self.file_status = "Exporting DYMO label…".into();
        std::thread::spawn(move || {
            let result = sdk::export_snapshot(&snapshot, text, objects).and_then(|bytes| {
                if destination_contents(&path)? != original { return Err("The export file changed while preparing it. Choose another file to avoid overwriting those changes.".into()); }
                project_io::write_project(&path, &bytes, original.is_some())?;
                Ok("DYMO label exported.".to_owned())
            });
            let _ = send.send(result);
        });
    }

    fn can_print(&self) -> bool {
        (!self.text.trim().is_empty() || !self.image_path.trim().is_empty() || !self.features.is_empty())
            && self.connected == Some(true) && self.preview_key.as_ref() == Some(&self.key())
            && self.preview_receiver.is_none() && self.print_receiver.is_none() && !self.canvas.is_dragging() && self.preview_snapshot.is_some() && self.preview_objects.is_some()
            && ((self.features.is_empty() && !self.layout.is_manual()) || !self.design_mode)
    }

    fn queue_print(&mut self) {
        if !self.can_print() { return; }
        let objects = match self.preview_objects.clone() {
            Some(objects) => objects,
            None => { self.print_status = "Check the additional objects before printing.".into(); return; }
        };
        let snapshot = self.preview_snapshot.clone().expect("print requires a current preview snapshot");
        if self.features.has_counter() && self.copies > 1 {
            match sequence::plan(&objects, self.copies) {
                Ok(plan) => {
                    let jobs = plan.labels.into_iter().map(|objects| batch::Job { text: self.text.clone(), objects }).collect();
                    self.start_batch_jobs(jobs, snapshot);
                }
                Err(error) => self.print_status = error,
            }
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        self.print_receiver = Some(receive);
        self.print_status = "Sending to DYMO…".into();
        let (text, copies) = (self.text.clone(), self.copies);
        std::thread::spawn(move || { let _ = send.send(PrintEvent::Single(sdk::print_snapshot(&snapshot, text, copies, objects))); });
    }

    fn start_batch_jobs(&mut self, jobs: Vec<batch::Job>, snapshot: std::sync::Arc<sdk::PrintSnapshot>) {
        if jobs.is_empty() || jobs.len() > batch::MAX_JOBS { self.print_status = "Choose between 1 and 100 labels.".into(); return; }
        if self.print_receiver.is_some() { self.print_status = "A print job is already running.".into(); return; }
        let (send, receive) = std::sync::mpsc::channel();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.print_cancel = Some(cancel.clone()); self.print_receiver = Some(receive);
        self.print_status = format!("Checking {} labels before printing…", jobs.len());
        std::thread::spawn(move || {
            let result = batch::run(&jobs, &cancel,
                |job| sdk::preview_snapshot(snapshot.clone(), job.text.clone(), job.objects.clone()).map(|_| ()),
                |job, _| sdk::print_snapshot(&snapshot, job.text.clone(), 1, job.objects.clone()),
                |progress| { let _ = send.send(PrintEvent::Progress(progress)); });
            let _ = send.send(PrintEvent::BatchDone(result));
        });
    }

    fn prepared_batch_jobs(&self) -> Result<Vec<batch::Job>, String> {
        if self.prepared_batch.is_empty() || self.prepared_batch.len() > batch::MAX_JOBS { return Err("Choose between 1 and 100 rows.".into()); }
        let objects = self.preview_objects.clone().ok_or("Generate the current label preview first.")?;
        let each = if self.features.has_counter() {
            sequence::plan(&objects, self.prepared_batch.len() as u16)?.labels
        } else { vec![objects; self.prepared_batch.len()] };
        self.prepared_batch.iter().zip(each).enumerate().map(|(index, (row, objects))| {
            let text = if self.text.is_empty() { row.clone() } else { format!("{}\n{row}", self.text) };
            if text.chars().count() > 500 { return Err(format!("Row {} would exceed the 500-character limit.", index + 1)); }
            Ok(batch::Job { text, objects })
        }).collect()
    }

    fn batch_key(&self) -> String {
        format!("{:?}", (self.key(), self.batch_selected, self.prepared_batch.get(self.batch_selected)))
    }

    fn preview_batch_row(&mut self) {
        if self.preview_receiver.is_some() || self.preview_key.as_ref() != Some(&self.key()) {
            self.batch_status = "Wait for the current label preview before previewing a batch row.".into(); return;
        }
        let Some(snapshot) = self.preview_snapshot.clone() else { self.batch_status = "Generate the current label preview first.".into(); return; };
        let job = match self.prepared_batch_jobs().and_then(|jobs| jobs.get(self.batch_selected).cloned().ok_or("Select a batch row.".into())) {
            Ok(job) => job,
            Err(error) => { self.batch_status = error; return; }
        };
        let key = self.batch_key();
        let (send, receive) = std::sync::mpsc::channel();
        self.batch_preview_key = None; self.batch_preview_texture = None; self.batch_preview_receiver = Some(receive); self.batch_status = "Generating row preview…".into();
        std::thread::spawn(move || { let _ = send.send((key, sdk::preview_snapshot(snapshot, job.text, job.objects))); });
    }

    fn queue_prepared_batch(&mut self) {
        if self.connected != Some(true) || self.print_receiver.is_some() || self.canvas.is_dragging() || self.preview_key.as_ref() != Some(&self.key()) || self.batch_preview_key.as_ref() != Some(&self.batch_key()) {
            self.batch_status = "Connect the 420P and preview the selected row before printing.".into(); return;
        }
        let Some(snapshot) = self.preview_snapshot.clone() else { self.batch_status = "Generate the label preview first.".into(); return; };
        match self.prepared_batch_jobs() {
            Ok(jobs) => { self.batch_status = "Checking every row before printing.".into(); self.start_batch_jobs(jobs, snapshot); }
            Err(error) => self.batch_status = error,
        }
    }

    fn toolbar_state(&self, ctx: &egui::Context) -> toolbar::ToolbarState {
        let state = egui::text_edit::TextEditState::load(ctx, egui::Id::new("label-text-editor")).unwrap_or_default();
        let cursor = state.cursor.char_range().unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(self.text.chars().count())));
        let current = (cursor, self.text.clone());
        let undoer = state.undoer();
        toolbar::ToolbarState { can_print: self.can_print(), has_text: !self.text.is_empty(), can_undo: undoer.has_undo(&current), can_redo: undoer.has_redo(&current), has_selection: !cursor.is_empty(), bold: self.bold, italic: self.italic, underline: self.underline, rulers: self.show_rulers, can_toggle_rulers: true, can_change_units: true, metric: self.metric_units, can_fit: true, can_show_about: true, can_save: self.project_path.is_some(), can_export: self.can_export(), layout_locked: self.layout.is_locked(), can_order: self.layout.can_order(), can_align_objects: self.layout.can_align_objects(), can_align_label: self.layout.can_align_to_label() && self.preview_key.as_ref() == Some(&self.key()) && self.printable_dimensions.is_some(), guides_visible: self.guides.is_visible(), guide_snap: self.guides.snapping_enabled() }
    }

    fn toolbar_action(&mut self, action: toolbar::Action, ctx: &egui::Context) {
        use toolbar::Action;
        match action {
            Action::LayoutPanel => self.show_layout = true,
            Action::GuidesPanel => self.show_guides = true,
            Action::ToggleGuides => self.guides.set_visible(!self.guides.is_visible()),
            Action::ToggleGuideSnap => self.guides.set_snapping(!self.guides.snapping_enabled()),
            Action::ToggleLayoutLock => self.layout.toggle_locked(),
            Action::LayoutOrder(order) => { self.layout.move_selected(order); },
            Action::LayoutAlignObjects(alignment) => if let Err(error) = self.layout.align_selected(alignment) { self.file_status = error; },
            Action::LayoutAlignLabel(alignment) => if self.preview_key.as_ref() == Some(&self.key()) { if let Some((width, height)) = self.printable_dimensions { if let Err(error) = self.layout.align_to_canvas(alignment, width, height) { self.file_status = error; } } },
            Action::New => self.request_document_action(PendingDocumentAction::New, ctx),
            Action::Open => if let Some(path) = rfd::FileDialog::new().add_filter("420P labels", &["420p"]).pick_file() { self.request_document_action(PendingDocumentAction::Open(path), ctx); },
            Action::Save => self.file_status = match self.save_current() { Ok(()) => "Label saved.".into(), Err(e) => e },
            Action::SaveAs => if let Some(path) = rfd::FileDialog::new().add_filter("420P labels", &["420p"]).set_file_name("label.420p").save_file() { self.file_status = match self.save_as_from_dialog(&path) { Ok(()) => "Label saved.".into(), Err(e) => e }; },
            Action::ExportDymo => self.export_dymo(),
            Action::ImportData => self.data_tools.show_import = true,
            Action::ManageContacts => self.data_tools.show_contacts = true,
            Action::ImportTemplate => if let Some(path) = rfd::FileDialog::new().add_filter("DYMO labels", &["dymo", "label"]).pick_file() { self.template_path = path.to_string_lossy().into_owned(); },
            Action::Image => if let Some(path) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg"]).pick_file() { self.image_path = path.to_string_lossy().into_owned(); },
            Action::Print => self.queue_print(),
            Action::Text => if self.text.is_empty() { ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("label-text-editor"))); } else { self.add_feature(features::FeatureKind::Text); },
            Action::Address => self.add_feature(features::FeatureKind::Address),
            Action::Shape => self.add_feature(features::FeatureKind::Shape),
            Action::Line => self.add_feature(features::FeatureKind::Line),
            Action::Barcode => self.add_feature(features::FeatureKind::Barcode),
            Action::Qr => self.add_feature(features::FeatureKind::Qr),
            Action::Date => self.add_feature(features::FeatureKind::Date),
            Action::Counter => self.add_feature(features::FeatureKind::Counter),
            Action::ToggleBold => self.bold = !self.bold,
            Action::ToggleItalic => self.italic = !self.italic,
            Action::ToggleUnderline => self.underline = !self.underline,
            Action::AlignLeft => self.align = "Left".into(),
            Action::AlignCentre => self.align = "Center".into(),
            Action::AlignRight => self.align = "Right".into(),
            Action::ZoomIn => self.zoom = (self.zoom + 1.0).min(4.0),
            Action::ZoomOut => self.zoom = (self.zoom - 1.0).max(1.0),
            Action::Zoom100 => self.zoom = 1.0,
            Action::Fit => self.fit_requested = true,
            Action::ToggleRulers => self.show_rulers = !self.show_rulers,
            Action::UnitsMetric => self.metric_units = true,
            Action::UnitsInches => self.metric_units = false,
            Action::HelpContents | Action::UserGuides => ctx.open_url(egui::OpenUrl::new_tab("https://www.dymo.co.uk/support?cfid=user-guide")),
            Action::About => self.show_about = true,
            Action::QuickTips => self.show_tips = true,
            Action::BuyLabels => ctx.open_url(egui::OpenUrl::new_tab("https://www.dymo.co.uk/labels-tapes/labelmanager-d1-tapes/")),
            Action::Register => ctx.open_url(egui::OpenUrl::new_tab("https://www.dymo.co.uk/support?cfid=product-registration")),
            Action::Preferences => self.show_preferences = true,
            Action::Exit => self.request_document_action(PendingDocumentAction::Exit, ctx),
            Action::CheckAppUpdates => {
                self.show_preferences = true;
                if !self.app_update_repository.trim().is_empty() { self.start_app_update_check(); }
            }
            Action::Undo | Action::Redo | Action::Cut | Action::Copy | Action::Paste | Action::SelectAll => self.toolbar_text_action(action, ctx),
        }
    }

    fn add_feature(&mut self, kind: features::FeatureKind) {
        match self.features.add(kind, self.width_mm) {
            Ok(()) => { self.design_mode = false; self.file_status = format!("Added {}. Edit it in Objects.", kind.label()); }
            Err(error) => self.file_status = error,
        }
    }

    fn toolbar_text_action(&mut self, action: toolbar::Action, ctx: &egui::Context) {
        use toolbar::Action;
        let id = egui::Id::new("label-text-editor");
        let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        let cursor = state.cursor.char_range().unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(self.text.chars().count())));
        let count = self.text.chars().count();
        let [start, end] = cursor.sorted_cursors();
        let range = start.index.0.min(count)..end.index.0.min(count);
        let current = (cursor, self.text.clone());
        let mut undoer = state.undoer();
        match action {
            Action::Undo | Action::Redo => {
                let value = if action == Action::Undo { undoer.undo(&current) } else { undoer.redo(&current) }.cloned();
                if let Some((selection, text)) = value { self.text = text; state.cursor.set_char_range(Some(selection)); }
            }
            Action::Copy | Action::Cut if !range.is_empty() => {
                ctx.copy_text(text_range(&self.text, range.clone()).to_owned());
                if action == Action::Cut { undoer.add_undo(&current); replace_text_range(&mut self.text, range.clone(), ""); state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(range.start)))); }
            }
            Action::Paste => match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
                Ok(value) => { undoer.add_undo(&current); let caret = replace_text_range(&mut self.text, range, &value); state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(caret)))); }
                Err(e) => self.file_status = format!("Could not read clipboard: {e}"),
            },
            Action::SelectAll => state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(count)))),
            _ => {}
        }
        state.set_undoer(undoer); state.store(ctx, id); ctx.memory_mut(|memory| memory.request_focus(id));
    }
    fn key(&self) -> String {
        format!("{:?}", ((&self.template_path, &self.text, &self.image_path, self.black_and_white, self.threshold, self.width_mm, self.bold, self.italic, self.underline, &self.font, &self.align, self.features.save_value()), self.layout.save_value(), &self.removed_objects))
    }
    fn threshold_base_key(&self) -> String {
        format!("{:?}", ((&self.template_path, &self.text, &self.image_path, self.black_and_white, self.width_mm, self.bold, self.italic, self.underline, &self.font, &self.align, self.features.save_value()), self.layout.save_value(), &self.removed_objects))
    }
    fn keep_preview_during_threshold_change(&self) -> bool {
        !self.image_path.is_empty() && self.preview_image.is_some() && self.preview_base_key.as_ref() == Some(&self.threshold_base_key())
    }
    fn document_key(&self) -> String { format!("{:?}", (self.key(), self.guides.save_value())) }
    fn start_preview(&mut self) {
        let key = self.key();
        let objects = match self.features.capture_request(self.width_mm) {
            Ok(value) => value.as_array().cloned().unwrap_or_default(),
            Err(error) => { self.preview_key = None; self.preview_image = None; self.highres_design = None; self.preview_dimensions = None; self.printable_dimensions = None; self.printable_origin = None; self.preview_snapshot = None; self.preview_objects = None; self.all_layouts = None; self.empty_primary_text = None; self.pending_move = None; self.object_layouts.clear(); self.preview_status = error; self.preview_due = None; return; }
        };
        let image_key = format!("{:?}", (&self.image_path, self.black_and_white, self.threshold));
        if image_key != self.design_image_key {
            self.design_image_key = image_key.clone();
            self.design_image = None;
            self.design_image_receiver = None;
            if !self.image_path.trim().is_empty() {
                let (send, receive) = std::sync::mpsc::channel();
                self.design_image_receiver = Some(receive);
                let path = self.image_path.clone();
                let (monochrome, threshold) = (self.black_and_white, self.threshold);
                std::thread::spawn(move || {
                    let result = sdk::prepare_image(&path, monochrome, threshold).map(|mut image| {
                        if monochrome {
                            for pixel in image.pixels_mut() {
                                if pixel[0] == 255 { pixel[3] = 0; }
                            }
                        }
                        image
                    });
                    let _ = send.send((image_key, result));
                });
            }
        }
        let (send, receive) = std::sync::mpsc::channel();
        self.preview_receiver = Some(receive); self.preview_key = None;
        if self.pending_move.is_none() && !self.keep_preview_during_threshold_change() { self.preview_image = None; self.highres_design = None; self.preview_dimensions = None; self.printable_dimensions = None; self.printable_origin = None; self.all_layouts = None; self.empty_primary_text = None; }
        self.preview_snapshot = None; self.preview_objects = None;
        self.preview_status = if self.keep_preview_during_threshold_change() { "Updating image threshold… Previous preview shown.".into() } else { "Generating preview…".into() }; self.pending_key = Some(key); self.preview_due = None;
        let (template, text, image_path) = (self.template_path.clone(), self.text.clone(), self.image_path.clone());
        let (black_and_white, threshold, width, bold, italic) = (self.black_and_white, self.threshold, self.width_mm, self.bold, self.italic);
        let (underline, font, align) = (self.underline, self.font.clone(), self.align.clone());
        let layout = self.layout.clone();
        let removed = self.removed_objects.clone();
        std::thread::spawn(move || {
            let result = if layout.is_manual() {
                sdk::preview_reconciled(template, text, image_path, black_and_white, threshold, width, bold, italic, underline, font, align, objects, layout, removed)
                    .map(|(layout, preview)| PreviewAttempt { layout: Some(layout), preview })
            } else {
                sdk::preview(template, text, image_path, black_and_white, threshold, width, bold, italic, underline, font, align, objects, None)
                    .map(|preview| PreviewAttempt { layout: None, preview: Ok(preview) })
            };
            let _ = send.send(result);
        });
    }
    fn project_bytes(&self) -> Result<Vec<u8>, String> {
        let template = if built_in_template(self.width_mm).as_ref() == Some(&self.template_path) { format!("builtin:{}", self.width_mm) } else { self.template_path.clone() };
        let value = serde_json::json!({"version":1,"text":self.text,"width":self.width_mm,"template":template,"image":self.image_path,"monochrome":self.black_and_white,"threshold":self.threshold,"bold":self.bold,"italic":self.italic,"underline":self.underline,"font":self.font,"align":self.align,"objects":self.features.save_value(),"layout":self.layout.save_value(),"guides":self.guides.save_value(),"pendingLayoutRemovals":self.removed_objects});
        serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())
    }
    fn save_as(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.save_as_expected(path, None)
    }
    /// Only call after the native Save As dialogue has accepted the path,
    /// including its overwrite confirmation when the destination exists.
    fn save_as_from_dialog(&mut self, path: &std::path::Path) -> Result<(), String> {
        match destination_contents(path)? {
            None => self.save_as(path),
            previous => self.save_as_expected(path, previous),
        }
    }
    fn save_as_expected(&mut self, path: &std::path::Path, previous: Option<Vec<u8>>) -> Result<(), String> {
        let bytes = self.project_bytes()?;
        if destination_contents(path)? != previous {
            return Err("The label file changed while preparing the save. Choose another file to avoid overwriting those changes.".into());
        }
        project_io::write_project(path, &bytes, previous.is_some())?;
        self.project_path = Some(path.to_owned());
        self.saved_disk_snapshot = Some(bytes);
        self.saved_key = self.document_key();
        Ok(())
    }
    fn save_current(&mut self) -> Result<(), String> {
        let path = self.project_path.as_ref().ok_or("Use Save as to choose a file first.")?;
        let current = std::fs::read(path).map_err(|e| format!("Could not read the existing label: {e}"))?;
        if self.saved_disk_snapshot.as_ref() != Some(&current) { return Err("The label file changed outside this app. Use Save as to avoid overwriting those changes.".into()); }
        let bytes = self.project_bytes()?;
        project_io::write_project(path, &bytes, true)?;
        self.saved_disk_snapshot = Some(bytes);
        self.saved_key = self.document_key();
        Ok(())
    }
    fn request_document_action(&mut self, action: PendingDocumentAction, ctx: &egui::Context) {
        if self.app_update_restarting { return; }
        if matches!(action, PendingDocumentAction::Exit) && self.app_update_download.is_some() {
            self.app_update_exit_after_download = true;
            if let Some(cancel) = &self.app_update_cancel { cancel.store(true, std::sync::atomic::Ordering::Release); }
            self.app_update_status = "Cancelling the download before closing…".into(); return;
        }
        let closing = matches!(action, PendingDocumentAction::Exit | PendingDocumentAction::InstallUpdate);
        if closing && self.export_receiver.is_some() { self.file_status = "Wait for the DYMO label export to finish before closing or restarting.".into(); return; }
        if closing && self.print_receiver.is_some() {
            self.print_status = "Wait for the current print job to finish, or cancel remaining labels, before exiting.".into();
            return;
        }
        if matches!(action, PendingDocumentAction::InstallUpdate) && self.update_printer_work_busy() { self.app_update_status = "Wait for printer operations to finish before restarting.".into(); return; }
        if self.document_key() != self.saved_key || (closing && self.data_tools.has_unsaved_contacts()) { self.pending_document_action = Some(action); }
        else { self.perform_document_action(action, ctx); }
    }
    fn perform_document_action(&mut self, action: PendingDocumentAction, ctx: &egui::Context) {
        match action {
            PendingDocumentAction::New => {
                self.text.clear(); self.image_path.clear(); self.features = features::Features::default(); self.layout = layout_tools::LayoutState::default(); self.guides = guides::Guides::default(); self.known_features.clear(); self.known_image = false; self.removed_objects.clear(); self.pending_move = None; self.empty_primary_text = None; self.preview_context_target = None; self.canvas.cancel();
                self.bold = false; self.italic = false; self.underline = false; self.font = "Segoe UI".into(); self.align = "Center".into();
                self.project_path = None; self.saved_disk_snapshot = None; self.preview_image = None; self.highres_design = None; self.preview_dimensions = None; self.printable_dimensions = None; self.preview_snapshot = None; self.preview_objects = None; self.all_layouts = None; self.preview_key = None; self.object_layouts.clear();
                self.design_mode = true; self.file_status = "New label.".into(); self.saved_key = self.document_key();
                ctx.data_mut(|data| data.remove::<egui::text_edit::TextEditState>(egui::Id::new("label-text-editor")));
            }
            PendingDocumentAction::Open(path) => {
                self.file_status = match self.load_project(&path) { Ok(()) => { ctx.data_mut(|data| data.remove::<egui::text_edit::TextEditState>(egui::Id::new("label-text-editor"))); "Label opened.".into() }, Err(e) => e };
            }
            PendingDocumentAction::Exit => {
                if self.app_update_download.is_some() { self.request_document_action(PendingDocumentAction::Exit, ctx); return; }
                if self.export_receiver.is_some() { self.file_status = "Wait for the DYMO label export to finish before exiting.".into(); return; }
                // The unsaved-changes dialogue is non-modal: printing may have
                // started since the exit request. Recheck at the final gate.
                if self.print_receiver.is_some() {
                    self.print_status = "Wait for the current print job to finish, or cancel remaining labels, before exiting.".into();
                    return;
                }
                self.allow_close = true; ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            PendingDocumentAction::InstallUpdate => {
                if self.update_printer_work_busy() || self.app_update_download.is_some() || self.app_update_restarting {
                    self.app_update_status = "Wait for printer operations and downloads to finish before restarting.".into(); return;
                }
                let Some(prepared) = self.app_update_prepared.clone() else { self.app_update_status = "Download and verify an update first.".into(); return; };
                let (send, receive) = std::sync::mpsc::channel();
                self.app_update_install_receiver = Some(receive); self.app_update_restarting = true;
                self.app_update_status = "Preparing the verified restart. Your current app will stay open if this fails.".into();
                std::thread::spawn(move || { let _ = send.send(updater::start(&prepared)); });
            }
        }
    }
    fn load_project(&mut self, path: &std::path::Path) -> Result<(), String> {
        let metadata = std::fs::metadata(path).map_err(|e| format!("Could not open label: {e}"))?;
        if metadata.len() > 1_000_000 { return Err("Label file is too large.".into()); }
        let bytes = std::fs::read(path).map_err(|e| format!("Could not open label: {e}"))?;
        let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| format!("Invalid label file: {e}"))?;
        if v["version"].as_u64() != Some(1) { return Err("Unsupported label version.".into()); }
        let text = v["text"].as_str().ok_or("Label text missing")?;
        let width = v["width"].as_u64().ok_or("Tape width missing")?;
        if ![6, 9, 12, 19].contains(&width) || text.chars().count() > 500 { return Err("Invalid label text or tape width.".into()); }
        let template = v["template"].as_str().ok_or("Template missing")?;
        let image = v["image"].as_str().ok_or("Image path missing")?;
        let threshold = v["threshold"].as_u64().filter(|n| (1..=254).contains(n)).ok_or("Invalid image threshold")?;
        let monochrome = v["monochrome"].as_bool().ok_or("Image setting missing")?;
        let bold = v["bold"].as_bool().ok_or("Bold setting missing")?;
        let italic = v["italic"].as_bool().ok_or("Italic setting missing")?;
        let underline = v["underline"].as_bool().unwrap_or(false);
        let font = v["font"].as_str().unwrap_or("Segoe UI");
        let align = v["align"].as_str().unwrap_or("Center");
        if font.trim().is_empty() || font.len() > 128 || font.chars().any(char::is_control) || !["Left", "Center", "Right"].contains(&align) { return Err("Invalid text formatting.".into()); }
        let template = if template == format!("builtin:{width}") { built_in_template(width as u8).ok_or("Built-in tape template is missing")? } else if template.starts_with("builtin:") { return Err("Invalid built-in template".into()); } else { template.to_owned() };
        let mut features = if let Some(objects) = v.get("objects") { features::Features::load_value(objects, width as u8)? } else { features::Features::default() };
        let layout = layout_tools::LayoutState::load_value(v.get("layout").unwrap_or(&serde_json::Value::Null))?;
        let guides = guides::Guides::load_value(v.get("guides").unwrap_or(&serde_json::Value::Null))?;
        let mut pending_removals = Vec::new();
        if let Some(value) = v.get("pendingLayoutRemovals") {
            let rows = value.as_array().ok_or("Invalid pending layout removals.")?;
            if rows.len() > 64 { return Err("Too many pending layout removals.".into()); }
            for row in rows {
                let name = row.as_str().ok_or("Invalid pending layout removal.")?;
                let extra = name.strip_prefix("Extra").and_then(|s| s.parse::<u32>().ok()).is_some_and(|n| n > 0 && format!("Extra{n}") == name);
                if (!extra && name != "ImageObject0") || pending_removals.iter().any(|old: &String| old.eq_ignore_ascii_case(name)) { return Err("Invalid pending layout removal.".into()); }
                pending_removals.push(name.to_owned());
            }
        }
        let layout_value = layout.save_value();
        let layout_names = layout_value["objects"].as_array().into_iter().flatten().filter_map(|item| item["name"].as_str().map(str::to_owned));
        features.reserve_layout_names(layout_names.chain(pending_removals.iter().cloned()));
        self.text = text.to_owned(); self.width_mm = width as u8; self.template_path = template; self.image_path = image.to_owned();
        self.threshold = threshold as u8; self.black_and_white = monochrome; self.bold = bold; self.italic = italic;
        self.underline = underline; self.font = font.into(); self.align = align.into();
        self.features = features; self.layout = layout; self.guides = guides; self.known_features = self.features.names(); self.known_image = !self.image_path.is_empty(); self.removed_objects = pending_removals; self.pending_move = None; self.empty_primary_text = None; self.preview_context_target = None; self.canvas.cancel();
        if !self.features.is_empty() || self.layout.is_manual() { self.design_mode = false; }
        self.preview_key = None; self.preview_image = None; self.highres_design = None; self.preview_dimensions = None; self.printable_dimensions = None; self.preview_snapshot = None; self.preview_objects = None; self.all_layouts = None; self.preview_status = "Generate a preview to see the label.".into();
        self.project_path = Some(path.to_owned()); self.saved_disk_snapshot = Some(bytes); self.saved_key = self.document_key();
        Ok(())
    }

    fn preview_target_at(&self, point: egui::Pos2, label_rect: egui::Rect) -> Option<PreviewContextTarget> {
        if self.preview_key.as_ref() != Some(&self.key()) || (self.design_mode && self.highres_design.is_none()) { return None; }
        let ((label_width, label_height), (origin_x, origin_y), (width, height), snapshot) =
            (self.preview_dimensions?, self.printable_origin?, self.printable_dimensions?, self.all_layouts.as_ref()?);
        if label_width <= 0.0 || label_height <= 0.0 { return None; }
        let surface = canvas_layout::Surface { rect: egui::Rect::from_min_max(
            egui::pos2(label_rect.left() + label_rect.width() * (origin_x / label_width) as f32, label_rect.top() + label_rect.height() * (origin_y / label_height) as f32),
            egui::pos2(label_rect.left() + label_rect.width() * ((origin_x + width) / label_width) as f32, label_rect.top() + label_rect.height() * ((origin_y + height) / label_height) as f32)), width_mm: width, height_mm: height };
        if !surface.valid() { return None; }
        let mut objects = canvas_layout::objects_from_snapshot(snapshot).ok()?;
        if let Some(empty) = &self.empty_primary_text { objects.retain(|item| !item.name.eq_ignore_ascii_case(empty)); }
        let name = canvas_layout::topmost_at(&objects, surface, point)?.name.clone();
        let feature_names = self.features.names();
        if feature_names.iter().any(|item| item.eq_ignore_ascii_case(&name)) { return Some(PreviewContextTarget::Feature(name)); }
        let rows = snapshot.as_array()?;
        let template_objects = |kind: &str| rows.iter().filter(|row| row["Kind"].as_str() == Some(kind) && !feature_names.iter().any(|item| row["Name"].as_str().is_some_and(|name| item.eq_ignore_ascii_case(name)))).filter_map(|row| row["Name"].as_str()).collect::<Vec<_>>();
        if template_objects("TextObject") == [name.as_str()] { return Some(PreviewContextTarget::PrimaryText(name)); }
        if !self.image_path.is_empty() && template_objects("ImageObject") == [name.as_str()] { return Some(PreviewContextTarget::Image(name)); }
        Some(PreviewContextTarget::TemplateObject(name))
    }

    fn delete_preview_target(&mut self, target: &PreviewContextTarget) {
        if self.layout.is_locked() { self.file_status = "Unlock the layout before removing an object.".into(); return; }
        match target {
            PreviewContextTarget::PrimaryText(_) => self.text.clear(),
            PreviewContextTarget::Image(_) => self.image_path.clear(),
            PreviewContextTarget::Feature(name) => { self.features.remove_by_name(name); }
            PreviewContextTarget::TemplateObject(_) => { self.file_status = "Edit this object in the original DYMO template.".into(); }
        }
    }

    fn preview_context_menu(&mut self, response: &egui::Response, label_rect: egui::Rect) {
        if response.secondary_clicked() {
            self.preview_context_target = response.interact_pointer_pos().and_then(|point| self.preview_target_at(point, label_rect));
            if let Some(target) = &self.preview_context_target {
                self.features.select_by_name(target.name());
                if self.layout.is_manual() { self.layout.select_only(target.name()); }
            }
        }
        response.context_menu(|ui| {
            if let Some(target) = self.preview_context_target.clone() {
                ui.strong(target.name());
                if matches!(&target, PreviewContextTarget::PrimaryText(_)) {
                    if ui.button("Edit text").clicked() { ui.ctx().memory_mut(|memory| memory.request_focus(egui::Id::new("label-text-editor"))); ui.close(); }
                    if ui.button("Copy text").clicked() { ui.ctx().copy_text(self.text.clone()); ui.close(); }
                    ui.checkbox(&mut self.bold, "Bold"); ui.checkbox(&mut self.italic, "Italic"); ui.checkbox(&mut self.underline, "Underline");
                } else if let PreviewContextTarget::Feature(name) = &target {
                    if ui.button("Edit object in sidebar").clicked() { self.features.select_by_name(name); ui.close(); }
                } else if matches!(&target, PreviewContextTarget::Image(_)) {
                    ui.checkbox(&mut self.black_and_white, "Convert image to black and white");
                }
                let removable = !matches!(&target, PreviewContextTarget::TemplateObject(_)) && !self.layout.is_locked();
                if ui.add_enabled(removable, egui::Button::new("Delete object")).clicked() { self.delete_preview_target(&target); ui.close(); }
                ui.separator();
                if !self.layout.is_manual() {
                    if ui.add_enabled(!self.layout.is_locked() && self.preview_key.as_ref() == Some(&self.key()), egui::Button::new("Enable manual layout")).clicked() {
                        if let Some(snapshot) = &self.all_layouts { match self.layout.enable_manual(snapshot) { Ok(()) => { self.layout.select_only(target.name()); self.design_mode = false; }, Err(error) => self.file_status = error } }
                        ui.close();
                    }
                }
                for (label, order) in [("Bring to front", layout_tools::Order::Front), ("Bring forward", layout_tools::Order::Forward), ("Send backward", layout_tools::Order::Backward), ("Send to back", layout_tools::Order::Back)] {
                    if ui.add_enabled(self.layout.can_order(), egui::Button::new(label)).clicked() { self.layout.move_selected(order); ui.close(); }
                }
                ui.menu_button("Align to label", |ui| {
                    let enabled = self.layout.can_align_to_label() && self.preview_key.as_ref() == Some(&self.key()) && self.printable_dimensions.is_some();
                    for (label, direction) in [("Left", layout_tools::Alignment::Left), ("Centre", layout_tools::Alignment::Centre), ("Right", layout_tools::Alignment::Right), ("Top", layout_tools::Alignment::Top), ("Middle", layout_tools::Alignment::Middle), ("Bottom", layout_tools::Alignment::Bottom)] {
                        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() { self.toolbar_action(toolbar::Action::LayoutAlignLabel(direction), ui.ctx()); ui.close(); }
                    }
                });
                if ui.button("Object layout…").clicked() { self.show_layout = true; ui.close(); }
            } else {
                ui.label("Label preview");
                if ui.button("Add text").clicked() { self.toolbar_action(toolbar::Action::Text, ui.ctx()); ui.close(); }
                if ui.button("Add image…").clicked() { self.toolbar_action(toolbar::Action::Image, ui.ctx()); ui.close(); }
                if ui.button("Object layout…").clicked() { self.show_layout = true; ui.close(); }
            }
        });
    }

    fn text_context_menu(&mut self, response: &egui::Response) {
        let text_id = egui::Id::new("label-text-editor");
        response.context_menu(|ui| {
            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), text_id).unwrap_or_default();
            let cursor = state.cursor.char_range().unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(self.text.chars().count())));
            let [start, end] = cursor.sorted_cursors();
            let count = self.text.chars().count();
            let range = start.index.0.min(count)..end.index.0.min(count);
            let selected = range.start < range.end;
            let current = (cursor, self.text.clone());
            let mut undoer = state.undoer();
            if ui.add_enabled(undoer.has_undo(&current), egui::Button::new("Undo")).clicked() {
                if let Some((selection, text)) = undoer.undo(&current).cloned() { self.text = text; state.cursor.set_char_range(Some(selection)); }
                state.set_undoer(undoer); state.store(ui.ctx(), text_id); ui.ctx().memory_mut(|memory| memory.request_focus(text_id)); ui.close(); return;
            }
            if ui.add_enabled(undoer.has_redo(&current), egui::Button::new("Redo")).clicked() {
                if let Some((selection, text)) = undoer.redo(&current).cloned() { self.text = text; state.cursor.set_char_range(Some(selection)); }
                state.set_undoer(undoer); state.store(ui.ctx(), text_id); ui.ctx().memory_mut(|memory| memory.request_focus(text_id)); ui.close(); return;
            }
            ui.separator();
            if ui.add_enabled(selected, egui::Button::new("Cut")).clicked() {
                ui.ctx().copy_text(text_range(&self.text, range.clone()).to_owned());
                undoer.add_undo(&current);
                replace_text_range(&mut self.text, range.clone(), "");
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(range.start))));
                state.set_undoer(undoer); state.store(ui.ctx(), text_id); ui.ctx().memory_mut(|memory| memory.request_focus(text_id)); ui.close(); return;
            }
            if ui.add_enabled(selected, egui::Button::new("Copy")).clicked() {
                ui.ctx().copy_text(text_range(&self.text, range.clone()).to_owned()); ui.close(); return;
            }
            if ui.button("Paste").clicked() {
                match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
                    Ok(value) => {
                        undoer.add_undo(&current);
                        let caret = replace_text_range(&mut self.text, range.clone(), &value);
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(caret))));
                        state.set_undoer(undoer); state.store(ui.ctx(), text_id); ui.ctx().memory_mut(|memory| memory.request_focus(text_id));
                    }
                    Err(e) => self.file_status = format!("Could not read clipboard: {e}"),
                }
                ui.close(); return;
            }
            if ui.add_enabled(selected, egui::Button::new("Delete selection")).clicked() {
                undoer.add_undo(&current); replace_text_range(&mut self.text, range.clone(), "");
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(range.start))));
                state.set_undoer(undoer); state.store(ui.ctx(), text_id); ui.ctx().memory_mut(|memory| memory.request_focus(text_id)); ui.close(); return;
            }
            if ui.add_enabled(count > 0, egui::Button::new("Select all text")).clicked() {
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(count))));
                state.store(ui.ctx(), text_id); ui.ctx().memory_mut(|memory| memory.request_focus(text_id)); ui.close(); return;
            }
            ui.separator();
            ui.checkbox(&mut self.bold, "Bold");
            ui.checkbox(&mut self.italic, "Italic");
            ui.checkbox(&mut self.underline, "Underline");
            ui.menu_button("Alignment", |ui| {
                for (value, display) in [("Left", "Left"), ("Center", "Centre"), ("Right", "Right")] {
                    if ui.selectable_value(&mut self.align, value.to_owned(), display).clicked() { ui.close(); }
                }
            });
            if !self.image_path.is_empty() && ui.button("Remove image").clicked() { self.image_path.clear(); ui.close(); }
            if ui.button("Object layout…").clicked() { self.show_layout = true; ui.close(); }
        });
    }
}

fn text_range(text: &str, range: std::ops::Range<usize>) -> &str {
    let byte = |i| text.char_indices().nth(i).map(|(offset, _)| offset).unwrap_or(text.len());
    &text[byte(range.start)..byte(range.end)]
}

fn destination_contents(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Cannot inspect destination: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(4_194_305).read_to_end(&mut bytes).map_err(|e| format!("Cannot read destination: {e}"))?;
    if bytes.len() > 4_194_304 { return Err("Choose another file; the existing file exceeds 4 MiB.".into()); }
    Ok(Some(bytes))
}

fn replace_text_range(text: &mut String, range: std::ops::Range<usize>, replacement: &str) -> usize {
    let count = text.chars().count();
    let start = range.start.min(count);
    let end = range.end.max(start).min(count);
    let permitted = 500_usize.saturating_sub(count - (end - start));
    let insertion: String = replacement.chars().take(permitted).collect();
    let before = text_range(text, 0..start).to_owned();
    let after = text_range(text, end..count).to_owned();
    *text = before + &insertion + &after;
    start + insertion.chars().count()
}

impl LabelApp {
    fn draw(&mut self, ui: &mut egui::Ui) {
            ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
            if self.app_update_restarting {
                if ui.input(|input| input.viewport().close_requested()) && !self.allow_close { ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose); }
                let received = self.app_update_install_receiver.as_ref().map(|receiver| receiver.try_recv());
                match received {
                    Some(Ok(Ok(()))) => { self.app_update_install_receiver = None; self.allow_close = true; ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                    Some(Ok(Err(error))) => { self.app_update_install_receiver = None; self.app_update_restarting = false; self.app_update_status = error; }
                    Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => { self.app_update_install_receiver = None; self.app_update_restarting = false; self.app_update_status = "The restart worker stopped. The current app has been kept open.".into(); }
                    _ => {}
                }
                if self.app_update_restarting {
                    ui.heading("Installing app update"); ui.label(&self.app_update_status); ui.spinner();
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)); return;
                }
            }
            self.poll_app_update_download(ui.ctx());
            if ui.input(|input| input.viewport().close_requested()) && !self.allow_close {
                if self.app_update_download.is_some() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.request_document_action(PendingDocumentAction::Exit, ui.ctx());
                } else if self.print_receiver.is_some() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.print_status = "Wait for the current print job to finish, or cancel remaining labels, before exiting.".into();
                } else if self.export_receiver.is_some() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.file_status = "Wait for the DYMO label export to finish before exiting.".into();
                } else if self.document_key() != self.saved_key || self.data_tools.has_unsaved_contacts() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    if self.pending_document_action.is_none() { self.pending_document_action = Some(PendingDocumentAction::Exit); }
                }
            }
            if self.pending_document_action.is_none() {
                let action = ui.input_mut(|input| {
                    if input.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::S) { Some(toolbar::Action::SaveAs) }
                    else if input.consume_key(egui::Modifiers::CTRL, egui::Key::N) { Some(toolbar::Action::New) }
                    else if input.consume_key(egui::Modifiers::CTRL, egui::Key::O) { Some(toolbar::Action::Open) }
                    else if input.consume_key(egui::Modifiers::CTRL, egui::Key::S) { Some(if self.project_path.is_some() { toolbar::Action::Save } else { toolbar::Action::SaveAs }) }
                    else if input.consume_key(egui::Modifiers::CTRL, egui::Key::P) { Some(toolbar::Action::Print) }
                    else if input.consume_key(egui::Modifiers::NONE, egui::Key::F1) { Some(toolbar::Action::HelpContents) }
                    else { None }
                });
                if let Some(action) = action { self.toolbar_action(action, ui.ctx()); }
            }
            if let Some(receiver) = &self.fonts_receiver {
                match receiver.try_recv() {
                    Ok(Ok(fonts)) => { self.fonts = fonts; self.fonts_receiver = None; }
                    Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.fonts_receiver = None; }
                    Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(500)),
                }
            }
            if let Some(receiver) = &self.export_receiver {
                match receiver.try_recv() {
                    Ok(Ok(status)) | Ok(Err(status)) => { self.file_status = status; self.export_receiver = None; }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.file_status = "Export stopped unexpectedly. Check the destination before retrying.".into(); self.export_receiver = None; }
                    Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(250)),
                }
            }
            if let Some(receiver) = &self.app_update_receiver {
                match receiver.try_recv() {
                    Ok((repository, result)) => {
                        if app_updates::normalise_repository(&self.app_update_repository).ok().as_deref() == Some(repository.as_str()) {
                            match result {
                                Ok(release) => {
                                    self.app_update_status = if release.newer { format!("App update available: {}", release.version) } else { format!("No newer app release. Latest published version: {}", release.version) };
                                    self.app_update_release = Some(release);
                                }
                                Err(error) => { self.app_update_status = error; self.app_update_release = None; }
                            }
                        }
                        self.app_update_receiver = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.app_update_status = "App update check stopped unexpectedly. Try again.".into(); self.app_update_receiver = None; }
                    Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(250)),
                }
            }
            self.app_update_notice_ui(ui.ctx());
            if let Some(receiver) = &self.print_receiver {
                match receiver.try_recv() {
                    Ok(PrintEvent::Single(Ok(()))) => { self.print_status = "Print job accepted by DYMO.".into(); self.print_receiver = None; }
                    Ok(PrintEvent::Single(Err(error))) => { self.print_status = error; self.print_receiver = None; }
                    Ok(PrintEvent::Progress(progress)) => {
                        self.print_status = match progress.phase {
                            batch::Phase::Preflight => format!("Checking label {} of {} before printing…", progress.completed, progress.total),
                            batch::Phase::Submit => format!("{} of {} label jobs accepted by DYMO…", progress.accepted, progress.total),
                        };
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    }
                    Ok(PrintEvent::BatchDone(outcome)) => {
                        self.print_status = match outcome {
                            batch::Outcome::Completed { accepted } => format!("{accepted} label jobs accepted by DYMO."),
                            batch::Outcome::Cancelled { accepted, .. } => format!("Cancelled; {accepted} label jobs were accepted by DYMO."),
                            batch::Outcome::InvalidInput(error) => error,
                            batch::Outcome::PreflightFailed { label, reason } => format!("Label {label} could not be checked: {reason}. No labels were sent."),
                            batch::Outcome::SubmitFailed { label, accepted, reason, .. } => format!("Stopped at label {label}; {accepted} earlier jobs were accepted. The last job may have been submitted; check the queue before retrying. {reason}"),
                        };
                        self.print_receiver = None; self.print_cancel = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.print_status = "Print worker stopped unexpectedly. Check the print queue before retrying.".into(); self.print_receiver = None; self.print_cancel = None; }
                    Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(250)),
                }
            }
            if let Some(receiver) = &self.preview_receiver {
                match receiver.try_recv() {
                    Ok(Ok(attempt)) => {
                        let completed_key = self.pending_key.take();
                        if completed_key.as_ref() == Some(&self.key()) {
                            if let Some(mut layout) = attempt.layout { layout.restore_selection(&self.layout.selected_names()); self.layout = layout; self.removed_objects.clear(); self.observed_key = self.key(); }
                            match attempt.preview {
                            Ok(preview) => {
                            self.preview_dimensions = preview.dimensions_mm;
                            self.printable_dimensions = preview.printable_dimensions_mm;
                            self.printable_origin = preview.printable_origin_mm;
                            self.preview_objects = Some(preview.objects);
                            self.all_layouts = preview.all_layouts.map(serde_json::Value::Array);
                            self.empty_primary_text = preview.empty_primary_text;
                            self.threshold_image_blank = preview.threshold_image_blank;
                            self.pending_move = None;
                            self.preview_base_key = Some(self.threshold_base_key());
                            self.preview_snapshot = Some(preview.snapshot);
                            self.highres_design = preview.design_image.map(|image| {
                                let pixels = egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw());
                                ui.ctx().load_texture("dymo-highres-design", pixels, egui::TextureOptions::LINEAR)
                            });
                            self.object_layouts = preview.object_layouts;
                            let adjustments = preview.adjusted_objects;
                            let image = preview.image;
                            if image.width() > 0 && image.height() > 0 {
                                let [r, g, b, a] = image.get_pixel(0, 0).0;
                                self.preview_paper = if a == 0 { Color32::WHITE } else { Color32::from_rgb(r, g, b) };
                            }
                            let size = [image.width() as usize, image.height() as usize];
                            let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                            self.preview_image = Some(ui.ctx().load_texture("dymo-preview", pixels, egui::TextureOptions::NEAREST));
                            self.preview_status = if adjustments.is_empty() { "Preview generated by DYMO SDK.".into() } else { "DYMO adjusted object sizes or positions; check the print preview and object measurements.".into() };
                            self.preview_key = Some(self.key());
                            }
                            Err(error) => { self.pending_move = None; self.preview_status = error; }
                            }
                        }
                        self.preview_receiver = None;
                    }
                    Ok(Err(error)) => { self.pending_move = None; self.preview_status = error; self.preview_receiver = None; }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.preview_status = "Preview worker stopped unexpectedly.".into(); self.preview_receiver = None; }
                    Err(std::sync::mpsc::TryRecvError::Empty) => { ui.ctx().request_repaint_after(std::time::Duration::from_millis(250)); }
                }
            }
            if let Some(receiver) = &self.batch_preview_receiver {
                match receiver.try_recv() {
                    Ok((key, Ok(preview))) => {
                        if key == self.batch_key() {
                            let image = preview.image;
                            let pixels = egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw());
                            self.batch_preview_texture = Some(ui.ctx().load_texture("batch-preview", pixels, egui::TextureOptions::NEAREST));
                            self.batch_preview_key = Some(key);
                            self.batch_status = "Row rendered by DYMO. Every batch row will be checked before any are sent.".into();
                        } else { self.batch_status = "The label or selected row changed; preview it again.".into(); }
                        self.batch_preview_receiver = None;
                    }
                    Ok((_, Err(error))) => { self.batch_status = error; self.batch_preview_receiver = None; }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.batch_status = "Row preview stopped unexpectedly.".into(); self.batch_preview_receiver = None; }
                    Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(250)),
                }
            }
            if let Some(receiver) = &self.design_image_receiver {
                match receiver.try_recv() {
                    Ok((key, Ok(image))) => {
                        if key == self.design_image_key {
                            let pixels = egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw());
                            self.design_image = Some(ui.ctx().load_texture("design-image", pixels, egui::TextureOptions::LINEAR));
                        }
                        self.design_image_receiver = None;
                    }
                    Ok((_, Err(_))) | Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.design_image_receiver = None; }
                    Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(250)),
                }
            }
            if let Some(receiver) = &self.status_receiver {
                match receiver.try_recv() {
                    Ok(result) => { self.apply_device_status(result); self.status_receiver = None; self.next_status_check = std::time::Instant::now() + std::time::Duration::from_secs(15); }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.apply_device_status(Err("Printer check failed.".into())); self.status_receiver = None; self.next_status_check = std::time::Instant::now() + std::time::Duration::from_secs(15); }
                    Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(300)),
                }
            }
            if self.status_receiver.is_none() && self.print_receiver.is_none() && std::time::Instant::now() >= self.next_status_check {
                self.start_status_check();
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
            } else if self.status_receiver.is_none() && self.print_receiver.is_none() {
                ui.ctx().request_repaint_after(self.next_status_check.saturating_duration_since(std::time::Instant::now()));
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Frame::new().inner_margin(20).show(ui, |ui| {
            let toolbar_state = self.toolbar_state(ui.ctx());
            if let Some(action) = toolbar::menu_bar(ui, &toolbar_state) { self.toolbar_action(action, ui.ctx()); }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.heading(RichText::new(branding::APP_NAME).size(24.0).strong());
                ui.add_space(16.0);
                let colour = match self.connected { Some(true) => Color32::from_rgb(54, 188, 106), Some(false) => Color32::from_rgb(224, 76, 76), None => Color32::from_gray(140) };
                let (circle, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
                ui.painter().circle_filled(circle.center(), 5.0, colour);
                ui.label(&self.service_status).on_hover_ui(|ui| self.device_status_ui(ui, true));
                if ui.add_enabled(self.status_receiver.is_none() && self.print_receiver.is_none(), egui::Button::new("Refresh")).on_hover_text("Read the printer connection, battery and cassette status").clicked() {
                    self.service_status = "Checking…".into();
                    self.start_status_check();
                }
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Open saved label…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("420P labels", &["420p"]).pick_file() {
                        self.request_document_action(PendingDocumentAction::Open(path), ui.ctx());
                    }
                }
                if ui.button("Save label as…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("420P labels", &["420p"]).set_file_name("label.420p").save_file() {
                        self.file_status = match self.save_as_from_dialog(&path) { Ok(()) => "Label saved.".into(), Err(e) => e };
                    }
                }
                if self.tray.is_some() && ui.button("Minimise to tray").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                ui.label(&self.file_status);
            });
            if let Some(error) = &self.tray_error { ui.label(format!("Notification area unavailable: {error}")); }
            self.device_status_ui(ui, false);
            ui.add_space(14.0);
            let sidebar_width = (ui.available_width() * 0.29).clamp(225.0, 300.0);
            ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(Vec2::new(42.0, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                if let Some(action) = toolbar::object_rail(ui) { self.toolbar_action(action, ui.ctx()); }
            });
            ui.allocate_ui_with_layout(Vec2::new(sidebar_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::Frame::new().fill(Color32::from_rgb(36, 38, 42)).corner_radius(8).inner_margin(16).show(ui, |ui| {
            ui.set_min_width((sidebar_width - 32.0).max(0.0));
            ui.strong("Tape width");
            ui.horizontal_wrapped(|ui| {
                for width in [6, 9, 12, 19] {
                    if ui.selectable_value(&mut self.width_mm, width, tape_width_label(width)).changed() {
                        self.template_path = built_in_template(width).unwrap_or_default();
                        self.preview_image = None;
                        self.preview_key = None;
                    }
                }
            });
            ui.add_space(8.0);
            ui.strong("Template");
            let template_name = std::path::Path::new(&self.template_path).file_name().and_then(|name| name.to_str()).unwrap_or("No template selected");
            ui.label(template_name).on_hover_text(&self.template_path);
            ui.collapsing("Choose a different template", |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.template_path).desired_width(ui.available_width()));
                if ui.button("Browse…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("DYMO labels", &["dymo", "label"]).pick_file() { self.template_path = path.to_string_lossy().into_owned(); }
                }
            });
            ui.add_space(8.0);
            ui.strong("Image");
            ui.horizontal(|ui| {
                if ui.button("Choose…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg"]).pick_file() { self.image_path = path.to_string_lossy().into_owned(); }
                }
                if !self.image_path.is_empty() && ui.button("Remove").clicked() { self.image_path.clear(); }
            });
            if !self.image_path.is_empty() {
                let image_name = std::path::Path::new(&self.image_path).file_name().and_then(|name| name.to_str()).unwrap_or("Selected image");
                ui.label(image_name).on_hover_text(&self.image_path);
                ui.checkbox(&mut self.black_and_white, "Convert image to black and white");
                if self.black_and_white { ui.add(egui::Slider::new(&mut self.threshold, 1..=254).text("Image threshold")); }
                if self.black_and_white && self.preview_key.as_ref() == Some(&self.key()) && self.threshold_image_blank {
                    ui.label(RichText::new("No black pixels at this threshold. Increase it to reveal darker parts of the image.").small().color(Color32::from_rgb(235, 180, 100)));
                }
            }
            if !self.features.is_empty() {
                ui.add_space(8.0);
                ui.separator();
                ui.strong("Objects");
                self.features.ui(ui, self.width_mm);
            }
            ui.add_space(8.0);
            if ui.add_enabled(self.preview_receiver.is_none() && !self.template_path.trim().is_empty(), egui::Button::new("Generate DYMO preview")).clicked() {
                self.start_preview();
            }
            });
            });
            ui.add_space(14.0);
            ui.allocate_ui_with_layout(Vec2::new(ui.available_width().max(0.0), 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            formatting_controls(ui, &mut self.bold, &mut self.italic, &mut self.underline, &mut self.font, &self.fonts, &mut self.align);
            ui.label(RichText::new("Edit text above; drag objects on the label preview below.").small().weak());
            egui::Frame::new().fill(Color32::from_rgb(43, 46, 51)).corner_radius(8).inner_margin(22).show(ui, |ui| {
                egui::Frame::new().fill(Color32::WHITE).stroke(egui::Stroke::new(1.0, Color32::from_gray(160))).inner_margin(8).show(ui, |ui| {
                ui.visuals_mut().override_text_color = Some(Color32::BLACK);
                ui.visuals_mut().text_cursor.stroke = egui::Stroke::new(2.0, Color32::BLACK);
                let text_id = egui::Id::new("label-text-editor");
                let previous_selection = egui::text_edit::TextEditState::load(ui.ctx(), text_id)
                    .and_then(|state| state.cursor.char_range()).filter(|range| !range.is_empty());
                if ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Secondary)) {
                    self.right_click_selection = previous_selection;
                }
                let response = ui.add_sized([ui.available_width(), 64.0], egui::TextEdit::multiline(&mut self.text)
                    .id(text_id)
                    .hint_text("Type your label here")
                    .font(egui::FontId::proportional(20.0))
                    .horizontal_align(match self.align.as_str() { "Left" => egui::Align::Min, "Right" => egui::Align::Max, _ => egui::Align::Center })
                    .vertical_align(egui::Align::Center)
                    .frame(egui::Frame::NONE)
                    .char_limit(500));
                if response.secondary_clicked() {
                    if let (Some(selection), Some(mut state)) = (self.right_click_selection.take(), egui::text_edit::TextEditState::load(ui.ctx(), text_id)) {
                        state.cursor.set_char_range(Some(selection));
                        state.store(ui.ctx(), text_id);
                    }
                }
                self.text_context_menu(&response);
                });
            });
            ui.add_space(12.0);
            ui.strong("Label preview");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.design_mode, true, "Smooth design");
                ui.selectable_value(&mut self.design_mode, false, "DYMO print preview");
            });
            let preview_width = self.preview_image.as_ref().map(|image| image.size_vec2().x).unwrap_or(169.0);
            let available_width = ui.available_width();
            let mut zoom_changed = false;
            if self.fit_requested {
                self.zoom = ((available_width - 36.0).max(20.0) / preview_width).clamp(0.25, 4.0);
                self.fit_requested = false;
                zoom_changed = true;
            }
            ui.horizontal(|ui| {
                for zoom in [1.0_f32, 2.0, 3.0, 4.0] {
                    if ui.selectable_value(&mut self.zoom, zoom, format!("{}×", zoom as u8)).changed() {
                        zoom_changed = true;
                    }
                }
            });
            ui.label(&self.preview_status);
            if let Some((length, width)) = self.preview_dimensions {
                if self.metric_units { ui.label(format!("SDK canvas: {length:.1} × {width:.1} mm · fitted tape: {} · cut length may differ", tape_width_label(self.width_mm))); }
                else { ui.label(format!("SDK canvas: {:.2} × {:.2} in · fitted tape: {} · cut length may differ", length / 25.4, width / 25.4, tape_width_label(self.width_mm))); }
            } else {
                ui.label("Label dimensions will appear when the DYMO preview is ready.");
            }
            if self.design_mode {
                if self.highres_design.is_some() { ui.label("High-resolution SDK design; check DYMO print preview for printer-resolution output."); }
                else {
                    ui.label("Approximate design: check fonts, layout and template objects in DYMO print preview.");
                    if !self.features.is_empty() { ui.label("Additional objects appear in DYMO print preview. Switch to it to move objects accurately."); }
                }
            }
            for layout in &self.object_layouts {
                let name = layout["Name"].as_str().unwrap_or("Object");
                let kind = layout["Kind"].as_str().unwrap_or("object");
                if let (Some(x), Some(y), Some(width), Some(height)) = (layout["Xmm"].as_f64(), layout["Ymm"].as_f64(), layout["WidthMm"].as_f64(), layout["HeightMm"].as_f64()) {
                    let scale = if self.metric_units { 1.0 } else { 25.4 };
                    let unit = if self.metric_units { "mm" } else { "in" };
                    ui.label(RichText::new(format!("{kind} {name}: position {:.2}, {:.2}; size {:.2} × {:.2} {unit}", x / scale, y / scale, width / scale, height / scale)).small());
                }
            }
            ui.scope(|ui| {
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            ui.style_mut().always_scroll_the_only_direction = true;
            let mut scroll = egui::ScrollArea::horizontal().id_salt("label-preview-scroll").max_width(available_width)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
            if zoom_changed { scroll = scroll.horizontal_scroll_offset(((preview_width * self.zoom + if self.preview_dimensions.is_some() { 36.0 } else { 0.0 } - available_width) / 2.0).max(0.0)); }
            scroll.show(ui, |ui| {
            let source_size = self.preview_image.as_ref().map(|image| image.size_vec2()).unwrap_or(Vec2::new(169.0, 38.0));
            let size = source_size * self.zoom;
            let margin = if self.show_rulers && self.preview_dimensions.is_some() { Vec2::new(36.0, 22.0) } else { Vec2::ZERO };
            let (outer_rect, preview_response) = ui.allocate_exact_size(size + margin, egui::Sense::click_and_drag());
            let rect = egui::Rect::from_min_size(outer_rect.min + margin, size);
            #[cfg(test)]
            ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("test-label-preview-rect"), rect));
            if self.show_rulers { if let Some((length, width)) = self.preview_dimensions { draw_rulers(ui, rect, length, width, self.metric_units); } }
            self.preview_context_menu(&preview_response, rect);
            if self.design_mode && self.highres_design.is_some() {
                let image = self.highres_design.as_ref().unwrap();
                ui.painter().image(image.id(), rect, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
            } else if self.design_mode {
                let painter = ui.painter().with_clip_rect(rect);
                painter.rect_filled(rect, 0.0, self.preview_paper);
                let padding = (rect.height() * 0.12).max(3.0);
                let mut text_rect = rect.shrink(padding);
                let current_image_key = format!("{:?}", (&self.image_path, self.black_and_white, self.threshold));
                if let Some(image) = self.design_image.as_ref().filter(|_| current_image_key == self.design_image_key) {
                    let extent = (rect.height() - 2.0 * padding).max(1.0).min(text_rect.width() * 0.45);
                    let source = image.size_vec2();
                    let factor = (extent / source.x).min(extent / source.y);
                    let image_size = source * factor;
                    let image_rect = egui::Rect::from_min_size(egui::pos2(text_rect.left(), rect.center().y - image_size.y / 2.0), image_size);
                    painter.image(image.id(), image_rect, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
                    text_rect.min.x = (image_rect.right() + padding).min(text_rect.right());
                }
                if !self.text.is_empty() && text_rect.is_positive() {
                    let mut font_size = (rect.height() * 0.48).max(6.0);
                    let galley = loop {
                        let mut job = egui::text::LayoutJob::default();
                        job.wrap.max_width = text_rect.width().max(1.0);
                        job.append(&self.text, 0.0, egui::TextFormat {
                            font_id: egui::FontId::proportional(font_size),
                            color: Color32::BLACK,
                            italics: self.italic,
                            underline: if self.underline { egui::Stroke::new(1.0, Color32::BLACK) } else { egui::Stroke::NONE },
                            ..Default::default()
                        });
                        let galley = painter.layout_job(job);
                        if galley.size().y <= text_rect.height() || font_size <= 6.0 { break galley; }
                        font_size = (font_size * 0.8).max(6.0);
                    };
                    let x = match self.align.as_str() {
                        "Left" => text_rect.left(),
                        "Right" => text_rect.right() - galley.size().x,
                        _ => text_rect.center().x - galley.size().x / 2.0,
                    };
                    let pos = egui::pos2(x, text_rect.center().y - galley.size().y / 2.0);
                    painter.galley(pos, galley.clone(), Color32::BLACK);
                    if self.bold { painter.galley(pos + Vec2::new(0.7, 0.0), galley, Color32::BLACK); }
                }
            } else if let Some(image) = &self.preview_image {
                ui.painter().image(image.id(), rect, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
            } else {
            ui.painter().rect_filled(rect, 3.0, Color32::WHITE);
            ui.painter().rect_stroke(
                rect, 3.0, egui::Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Inside
            );
            let preview = if self.text.trim().is_empty() { "Your label" } else { &self.text };
            ui.painter().with_clip_rect(rect.shrink(4.0)).text(
                rect.center(), egui::Align2::CENTER_CENTER, preview,
                egui::FontId::proportional((rect.height() * 0.32).clamp(12.0, 24.0)), Color32::BLACK,
            );
            }
            if self.preview_key.as_ref() == Some(&self.key()) {
                if let (Some((label_width, label_height)), Some((origin_x, origin_y)), Some((print_width, print_height)), Some(snapshot)) = (self.preview_dimensions, self.printable_origin, self.printable_dimensions, self.all_layouts.as_ref()) {
                    if label_width > 0.0 && label_height > 0.0 && origin_x + print_width <= label_width + 0.15 && origin_y + print_height <= label_height + 0.15 {
                        let bounds = egui::Rect::from_min_max(
                            egui::pos2(rect.left() + rect.width() * (origin_x / label_width) as f32, rect.top() + rect.height() * (origin_y / label_height) as f32),
                            egui::pos2(rect.left() + rect.width() * ((origin_x + print_width) / label_width) as f32, rect.top() + rect.height() * ((origin_y + print_height) / label_height) as f32));
                        self.guides.draw(&ui.painter().with_clip_rect(ui.clip_rect()), bounds, print_width, print_height);
                        if (!self.design_mode || self.highres_design.is_some()) && let Ok(mut objects) = canvas_layout::objects_from_snapshot(snapshot) {
                            if let Some(empty) = &self.empty_primary_text { objects.retain(|object| !object.name.eq_ignore_ascii_case(empty)); }
                            let surface = canvas_layout::Surface { rect: bounds, width_mm: print_width, height_mm: print_height };
                            let image = if self.design_mode { self.highres_design.as_ref() } else { self.preview_image.as_ref() }.map(|texture| (rect, texture.id(), self.preview_paper));
                            let action = self.canvas.show(ui, &preview_response, surface, &objects, &self.layout.selected_names(), self.layout.is_locked(), &self.guides, image);
                            if let Some((name, x, y)) = action.move_to {
                                self.features.select_by_name(&name);
                                if !self.layout.is_manual() { if let Err(error) = self.layout.enable_manual(snapshot) { self.file_status = error; } }
                                if self.layout.is_manual() {
                                    self.layout.select_only(&name);
                                    if let Some(moving) = objects.iter().find(|object| object.name.eq_ignore_ascii_case(&name)) {
                                        if let Some(other) = canvas_layout::collision_at(&objects, moving, x, y, |a, b| self.layout.allows_overlap(a, b)) {
                                            self.file_status = format!("That position overlaps {other}. Move the objects apart or allow overlap for both in Object layout.");
                                        } else {
                                            match self.layout.move_object(&name, x, y) {
                                                Ok(true) => { self.pending_move = Some(canvas_layout::MoveVisual { object: moving.clone(), x, y }); self.file_status.clear(); }
                                                Ok(false) => {}
                                                Err(error) => self.file_status = error,
                                            }
                                        }
                                    }
                                    self.design_mode = false;
                                }
                            } else if let Some(name) = action.select {
                                self.features.select_by_name(&name);
                                if self.layout.is_manual() { self.layout.select_only(&name); }
                                else if !self.canvas.is_dragging() {
                                    match self.layout.enable_manual(snapshot) { Ok(()) => { self.layout.select_only(&name); self.design_mode = false; }, Err(error) => self.file_status = error }
                                }
                            }
                        } else { self.canvas.cancel(); }
                    }
                }
            } else { self.canvas.cancel(); }
            if self.preview_key.as_ref() != Some(&self.key()) {
                if let (Some(visual), Some((label_width, label_height)), Some((origin_x, origin_y)), Some((print_width, print_height)), Some(snapshot), Some(texture)) = (&self.pending_move, self.preview_dimensions, self.printable_origin, self.printable_dimensions, self.all_layouts.as_ref(), self.preview_image.as_ref()) {
                    if label_width > 0.0 && label_height > 0.0 {
                        let bounds = egui::Rect::from_min_max(
                            egui::pos2(rect.left() + rect.width() * (origin_x / label_width) as f32, rect.top() + rect.height() * (origin_y / label_height) as f32),
                            egui::pos2(rect.left() + rect.width() * ((origin_x + print_width) / label_width) as f32, rect.top() + rect.height() * ((origin_y + print_height) / label_height) as f32));
                        if let Ok(mut objects) = canvas_layout::objects_from_snapshot(snapshot) {
                            if let Some(empty) = &self.empty_primary_text { objects.retain(|object| !object.name.eq_ignore_ascii_case(empty)); }
                            canvas_layout::paint_move(ui, canvas_layout::Surface { rect: bounds, width_mm: print_width, height_mm: print_height }, rect, texture.id(), self.preview_paper, visual, &objects);
                        }
                    }
                }
            }
            });
            });
            ui.add_space(16.0);

            ui.horizontal(|ui| {
                ui.label("Copies");
                ui.add(egui::DragValue::new(&mut self.copies).range(1..=100));
            });
            ui.add_space(12.0);
            let has_content = !self.text.trim().is_empty() || !self.image_path.trim().is_empty() || !self.features.is_empty();
            let can_print = self.can_print();
            let button_colour = if can_print { Color32::from_rgb(35, 130, 78) } else { Color32::from_gray(55) };
            let icon_colour = if can_print { Color32::WHITE } else { Color32::from_gray(145) };
            let print_button = ui.horizontal_wrapped(|ui| {
                let button = ui.add_enabled(can_print, egui::Button::new(RichText::new("        Print label").strong().color(icon_colour))
                    .fill(button_colour).min_size(Vec2::new(150.0, 40.0)));
                if !has_content { ui.label("Add text or an image before printing."); }
                else if self.connected != Some(true) { ui.label("Connect the 420P and check its status before printing."); }
                else if (!self.features.is_empty() || self.layout.is_manual()) && self.design_mode { ui.label("Select DYMO print preview to check the objects before printing."); }
                else if !can_print { ui.label("Generate a current preview before printing."); }
                button
            }).inner;
            let icon_origin = print_button.rect.left_center() + Vec2::new(12.0, -10.0);
            let stroke = egui::Stroke::new(1.5, icon_colour);
            let printer_body = egui::Rect::from_min_size(icon_origin + Vec2::new(0.0, 6.0), Vec2::new(20.0, 11.0));
            let top_sheet = egui::Rect::from_min_size(icon_origin + Vec2::new(4.0, 0.0), Vec2::new(12.0, 8.0));
            let bottom_sheet = egui::Rect::from_min_size(icon_origin + Vec2::new(4.0, 12.0), Vec2::new(12.0, 8.0));
            ui.painter().rect_stroke(top_sheet, 0.0, stroke, egui::StrokeKind::Inside);
            ui.painter().rect_stroke(printer_body, 2.0, stroke, egui::StrokeKind::Inside);
            ui.painter().rect_filled(bottom_sheet, 0.0, button_colour);
            ui.painter().rect_stroke(bottom_sheet, 0.0, stroke, egui::StrokeKind::Inside);
            ui.painter().line_segment([icon_origin + Vec2::new(7.0, 15.0), icon_origin + Vec2::new(13.0, 15.0)], stroke);
            if print_button.clicked() {
                self.queue_print();
            }
            ui.add_space(8.0);
            ui.label(RichText::new(&self.print_status).strong());
            if let Some(cancel) = &self.print_cancel {
                if ui.button("Cancel remaining labels").clicked() { cancel.store(true, std::sync::atomic::Ordering::Release); }
            }
            });
            });
            ui.add_space(12.0);
            ui.separator();
            ui.collapsing("Updates", |ui| {
                self.app_updates_ui(ui);
            });
            });
            });
            if self.show_about {
                egui::Window::new(format!("About {}", branding::APP_NAME)).open(&mut self.show_about).show(ui.ctx(), |ui| {
                    ui.strong(branding::APP_NAME);
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.small("Copyright (c) 2026 dymo-420p-rust contributors.");
                    ui.label("Uses the locally installed DYMO Connect software and SDK.");
                });
            }
            if self.show_preferences {
                let mut settings_open = self.show_preferences;
                if self.model_picture.is_none() {
                    if let Ok(image) = image::load_from_memory(include_bytes!("../assets/printer-420p-v6.png")) {
                        let image = image.to_rgba8();
                        let pixels = egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw());
                        self.model_picture = Some(ui.ctx().load_texture("settings-420p", pixels, egui::TextureOptions::LINEAR));
                    }
                }
                egui::Window::new("Settings").open(&mut settings_open).default_width(540.0).min_width(500.0).max_height(620.0).vscroll(true).resizable(false).show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        if let Some(picture) = &self.model_picture {
                            let size = picture.size_vec2();
                            let scale = (200.0 / size.x).min(220.0 / size.y);
                            ui.add(egui::Image::new((picture.id(), size * scale))).on_hover_text("DYMO LabelManager 420P");
                        }
                        ui.vertical(|ui| {
                            ui.strong("DYMO LabelManager 420P");
                            ui.label("USB label maker · manual cutter");
                            ui.label("D1 tape: 6, 9, 12 or 19 mm");
                            ui.label(&self.service_status);
                            self.device_status_ui(ui, true);
                        });
                    });
                    ui.separator();
                    ui.strong("Print settings");
                    ui.horizontal(|ui| { ui.label("Copies"); ui.add(egui::DragValue::new(&mut self.copies).range(1..=100)); });
                    ui.add_enabled(false, egui::Button::new("Mirror, collate, auto-cut and print quality")).on_disabled_hover_text("These settings have not been verified for the 420P.");
                    ui.separator();
                    ui.checkbox(&mut self.show_rulers, "Show rulers");
                    ui.horizontal(|ui| { ui.label("Units"); ui.selectable_value(&mut self.metric_units, true, "Metric"); ui.selectable_value(&mut self.metric_units, false, "Inches"); });
                    ui.separator();
                    if self.app_update_scroll_requested { ui.scroll_to_cursor(Some(egui::Align::Min)); self.app_update_scroll_requested = false; }
                    self.app_updates_ui(ui);
                });
                self.show_preferences = settings_open;
            }
            if self.show_tips {
                egui::Window::new("Quick tips").open(&mut self.show_tips).show(ui.ctx(), |ui| {
                    ui.label("Select the width of the cassette fitted to the 420P.");
                    ui.label("Use DYMO print preview to check the label and any additional objects.");
                    ui.label("For images, adjust the black-and-white threshold before previewing.");
                    ui.label("Use the 1×–4× buttons and mouse wheel to inspect an enlarged preview.");
                });
            }
            if let Some(action) = self.data_tools.show(ui.ctx()) {
                match action {
                    data_tools::DataAction::AppendText(value) => {
                        let separator = if self.text.is_empty() { "" } else { "\n" };
                        if self.text.chars().count() + separator.chars().count() + value.chars().count() > 500 {
                            self.file_status = "The combined label would exceed 500 characters; nothing was added.".into();
                        } else {
                            self.text.push_str(separator); self.text.push_str(&value); self.file_status = "Text added to the label.".into();
                        }
                    }
                    data_tools::DataAction::PrepareBatch(rows) => {
                        if rows.len() > 100 { self.file_status = "Choose at most 100 rows per batch.".into(); }
                        else if rows.iter().any(|row| self.text.chars().count() + usize::from(!self.text.is_empty()) + row.chars().count() > 500) {
                            self.file_status = "A row would make the label longer than 500 characters; the batch was not changed.".into();
                        } else {
                            self.prepared_batch = rows; self.show_batch = true; self.batch_selected = 0;
                            self.batch_preview_key = None; self.batch_preview_texture = None; self.batch_preview_receiver = None;
                            self.batch_status = "Preview a row, then print when ready.".into();
                            self.file_status = "Batch prepared; nothing has been printed.".into();
                        }
                    }
                }
            }
            if self.show_batch {
                let mut open = self.show_batch;
                let mut preview = false;
                let mut print = false;
                egui::Window::new("Prepared batch").open(&mut open).default_width(520.0).show(ui.ctx(), |ui| {
                    ui.label(format!("{} rows · one label per row", self.prepared_batch.len()));
                    ui.label("The current label text is added before each row. Counters advance once per row.");
                    if !self.prepared_batch.is_empty() {
                        let mut row = self.batch_selected + 1;
                        ui.horizontal(|ui| { ui.label("Review row"); ui.add(egui::DragValue::new(&mut row).range(1..=self.prepared_batch.len())); });
                        self.batch_selected = row - 1;
                        let preview_is_current = self.batch_preview_key.as_ref() == Some(&self.batch_key());
                        egui::ScrollArea::vertical().max_height(90.0).show(ui, |ui| { ui.label(&self.prepared_batch[self.batch_selected]); });
                        preview = ui.add_enabled(self.batch_preview_receiver.is_none(), egui::Button::new("Preview selected row")).clicked();
                        if preview_is_current {
                            if let Some(image) = &self.batch_preview_texture {
                                egui::ScrollArea::horizontal().show(ui, |ui| { ui.add(egui::Image::new(image).fit_to_exact_size(image.size_vec2() * 2.0)); });
                            }
                        }
                        let ready = self.connected == Some(true) && self.print_receiver.is_none() && !self.canvas.is_dragging() && self.preview_key.as_ref() == Some(&self.key()) && preview_is_current;
                        print = ui.add_enabled(ready, egui::Button::new("Print batch")).on_disabled_hover_text("Connect the 420P and preview the selected row first.").clicked();
                    }
                    ui.label(&self.batch_status);
                    if self.print_receiver.is_some() { ui.label(&self.print_status); }
                    if let Some(cancel) = &self.print_cancel {
                        if ui.button("Cancel remaining labels").clicked() { cancel.store(true, std::sync::atomic::Ordering::Release); }
                    }
                });
                self.show_batch = open;
                if preview { self.preview_batch_row(); }
                if print { self.queue_prepared_batch(); }
            }
            if self.show_layout {
                let mut open = self.show_layout;
                let snapshot = if self.preview_key.as_ref() == Some(&self.key()) { self.all_layouts.as_ref() } else { None };
                egui::Window::new("Object layout").open(&mut open).default_width(400.0).show(ui.ctx(), |ui| {
                    self.layout.ui(ui, snapshot);
                    if self.layout.is_manual() { if let Some(items) = snapshot.and_then(|value| value.as_array()) {
                        ui.separator();
                        ui.label("DYMO resolved positions after preview (mm):");
                        for item in items {
                            if let (Some(name), Some(x), Some(y), Some(width), Some(height)) = (item["Name"].as_str(), item["Xmm"].as_f64(), item["Ymm"].as_f64(), item["WidthMm"].as_f64(), item["HeightMm"].as_f64()) {
                                ui.label(format!("{name}: x {x:.1}, y {y:.1}, {width:.1} × {height:.1}"));
                            }
                        }
                    }}
                });
                self.show_layout = open;
                if self.layout.is_manual() { self.design_mode = false; }
            }
            if self.show_guides {
                let mut open = self.show_guides;
                let dimensions = if self.preview_key.as_ref() == Some(&self.key()) { self.printable_dimensions } else { None };
                egui::Window::new("Guides").open(&mut open).default_width(400.0).show(ui.ctx(), |ui| { self.guides.ui(ui, dimensions); });
                self.show_guides = open;
            }
            if self.pending_document_action.is_some() {
                let mut discard = false;
                let mut cancel = false;
                let mut save = false;
                egui::Window::new("Unsaved changes").collapsible(false).resizable(false).show(ui.ctx(), |ui| {
                    if matches!(self.pending_document_action, Some(PendingDocumentAction::Exit | PendingDocumentAction::InstallUpdate)) && self.data_tools.has_unsaved_contacts() {
                        ui.label("There are unsaved contact changes, and the label may also have unsaved changes. Discard them and continue?");
                    } else { ui.label("This label has unsaved changes. Discard them and continue?"); }
                    ui.horizontal(|ui| {
                        if matches!(self.pending_document_action, Some(PendingDocumentAction::InstallUpdate)) && !self.data_tools.has_unsaved_contacts() {
                            save = ui.button("Save label and continue").clicked();
                        }
                        discard = ui.button("Discard and continue").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
                if save {
                    let result = if self.project_path.is_some() { Some(self.save_current()) }
                        else { rfd::FileDialog::new().add_filter("420P labels", &["420p"]).set_file_name("label.420p").save_file().map(|path| self.save_as_from_dialog(&path)) };
                    match result {
                        Some(Ok(())) => { if let Some(action) = self.pending_document_action.take() { self.perform_document_action(action, ui.ctx()); } }
                        Some(Err(error)) => self.file_status = error,
                        None => {}
                    }
                }
                else if discard { if let Some(action) = self.pending_document_action.take() { self.perform_document_action(action, ui.ctx()); } }
                else if cancel { self.pending_document_action = None; }
            }
            if self.app_update_restarting { ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)); return; }
            let names = self.features.names();
            if self.known_features.iter().any(|name| !names.contains(name)) && self.file_status.starts_with("Added ") {
                self.file_status.clear();
            }
            let existing_layout = self.layout.save_value();
            let existing_names: Vec<String> = existing_layout["objects"].as_array().into_iter().flatten().filter_map(|item| item["name"].as_str().map(str::to_owned)).collect();
            let deliberately_removed = self.known_features.iter().filter(|name| !names.contains(*name) && existing_names.contains(*name)).cloned().collect::<Vec<_>>();
            if !deliberately_removed.is_empty() && self.layout.is_manual() {
                if let Err(error) = self.layout.remove_intentionally(&deliberately_removed) { self.file_status = error; }
            }
            if self.layout.is_manual() {
                for name in &self.known_features {
                    if !names.contains(name) && !self.removed_objects.contains(name) { self.removed_objects.push(name.clone()); }
                }
                if self.known_image && self.image_path.is_empty() && !self.removed_objects.iter().any(|name| name == "ImageObject0") { self.removed_objects.push("ImageObject0".into()); }
            } else { self.removed_objects.clear(); }
            self.known_features = names; self.known_image = !self.image_path.is_empty();
            let current = self.key();
            if current != self.observed_key {
                self.observed_key = current.clone();
                self.preview_key = None;
                let threshold_only = self.keep_preview_during_threshold_change();
                if self.pending_move.is_none() && !threshold_only {
                    self.preview_image = None; self.highres_design = None; self.preview_dimensions = None;
                    self.printable_dimensions = None; self.printable_origin = None; self.all_layouts = None; self.empty_primary_text = None;
                }
                self.preview_snapshot = None;
                self.preview_objects = None;
                self.canvas.cancel();
                self.object_layouts.clear();
                self.preview_status = if self.template_path.trim().is_empty() { "Choose a matching DYMO template.".into() } else if threshold_only { "Updating image threshold… Previous preview shown.".into() } else { "Preview updating…".into() };
                self.preview_due = Some(std::time::Instant::now() + std::time::Duration::from_millis(if threshold_only { 150 } else { 700 }));
            }
            if let Some(due) = self.preview_due {
                if std::time::Instant::now() >= due && self.preview_receiver.is_none() {
                    if !self.template_path.trim().is_empty() { self.start_preview(); }
                    else { self.preview_due = None; }
                } else {
                    ui.ctx().request_repaint_after(due.saturating_duration_since(std::time::Instant::now()).max(std::time::Duration::from_millis(100)));
                }
            }
    }
}

impl eframe::App for LabelApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
        if let Some(token) = self.app_update_acknowledge.take() {
            match updater::acknowledge(&token) {
                Ok(()) => self.app_update_status = format!("Updated to version {}. The previous version is kept locally for recovery.", env!("CARGO_PKG_VERSION")),
                Err(error) => { self.app_update_status = error; self.allow_close = true; self.app_update_restarting = true; ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
            }
        }
    }
}

fn formatting_controls(ui: &mut egui::Ui, bold: &mut bool, italic: &mut bool, underline: &mut bool, font: &mut String, fonts: &[String], align: &mut String) -> (egui::Rect, egui::Rect) {
    ui.scope(|ui| {
        let control_height = ui.text_style_height(&egui::TextStyle::Button).max(ui.spacing().icon_width) + 2.0 * ui.spacing().button_padding.y;
        ui.spacing_mut().interact_size.y = ui.spacing().interact_size.y.max(control_height);
        ui.horizontal_wrapped(|ui| {
            ui.toggle_value(bold, RichText::new("Bold").strong());
            ui.toggle_value(italic, RichText::new("Italic").italics());
            ui.toggle_value(underline, RichText::new("Underline").underline());
            ui.visuals_mut().widgets.hovered.expansion = 0.0;
            ui.visuals_mut().widgets.active.expansion = 0.0;
            let font_box = egui::ComboBox::from_id_salt("font").selected_text(font.as_str()).height(240.0).show_ui(ui, |ui| { for item in fonts { ui.selectable_value(font, item.clone(), item); } });
            let alignment_text = if align == "Center" { "Centre" } else { align.as_str() };
            let alignment_box = egui::ComboBox::from_id_salt("alignment").selected_text(alignment_text).show_ui(ui, |ui| { for (value, label) in [("Left", "Left"), ("Center", "Centre"), ("Right", "Right")] { ui.selectable_value(align, value.to_owned(), label); } });
            (font_box.response.rect, alignment_box.response.rect)
        }).inner
    }).inner
}

fn draw_rulers(ui: &egui::Ui, label: egui::Rect, length_mm: f64, width_mm: f64, metric: bool) {
    let painter = ui.painter();
    let colour = Color32::from_gray(180);
    let stroke = egui::Stroke::new(1.0, colour);
    let font = egui::FontId::proportional(10.0);
    let unit = if metric { 1.0 } else { 25.4 };
    let length = length_mm / unit;
    let width = width_mm / unit;
    let pixels_per_unit = label.width() / length as f32;
    let steps: &[f64] = if metric { &[1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0] } else { &[0.0625, 0.125, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0] };
    let step = steps.iter().copied().find(|step| pixels_per_unit * *step as f32 >= 35.0).unwrap_or(*steps.last().unwrap());
    painter.line_segment([egui::pos2(label.left(), label.top() - 3.0), egui::pos2(label.right(), label.top() - 3.0)], stroke);
    for index in 0..=(length / step).floor() as usize {
        let value = index as f64 * step;
        let x = label.left() + (value as f32 / length as f32) * label.width();
        painter.line_segment([egui::pos2(x, label.top() - 3.0), egui::pos2(x, label.top() - 9.0)], stroke);
        let text = if metric { format!("{value:.0}") } else { format_ruler_inches(value) };
        painter.text(egui::pos2(x, label.top() - 10.0), egui::Align2::CENTER_BOTTOM, text, font.clone(), colour);
    }
    painter.line_segment([egui::pos2(label.left() - 3.0, label.top()), egui::pos2(label.left() - 3.0, label.bottom())], stroke);
    let vertical_step = if metric { 5.0 } else { 0.125 };
    for index in 0..=(width / vertical_step).floor() as usize {
        let value = index as f64 * vertical_step;
        let y = label.top() + (value as f32 / width as f32) * label.height();
        painter.line_segment([egui::pos2(label.left() - 3.0, y), egui::pos2(label.left() - 9.0, y)], stroke);
        let text = if metric { format!("{value:.0}") } else { format_ruler_inches(value) };
        painter.text(egui::pos2(label.left() - 11.0, y), egui::Align2::RIGHT_CENTER, text, font.clone(), colour);
    }
}

fn format_ruler_inches(value: f64) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn tape_width_label(width: u8) -> &'static str {
    // Nominal cassette sizes as labelled by DYMO, not exact conversions.
    match width {
        6 => "6 mm (1/4 in)",
        9 => "9 mm (3/8 in)",
        12 => "12 mm (1/2 in)",
        19 => "19 mm (3/4 in)",
        _ => "Unknown tape size",
    }
}

fn built_in_template(width: u8) -> Option<String> {
    let path = std::env::current_exe().ok()?.parent()?.join("sdk").join("templates").join(format!("{width}mm.dymo"));
    path.is_file().then(|| path.to_string_lossy().into_owned())
}

fn create_tray(ctx: &egui::Context) -> Result<tray_icon::TrayIcon, String> {
    use tray_icon::{menu::{Menu, MenuItem, MenuEvent}, Icon, TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState};
    // Tray icons are normally only 16–24 pixels wide: use a simplified 420P
    // silhouette rather than shrinking the detailed full-size shortcut icon.
    let mut image = image::RgbaImage::new(32, 32);
    for y in 0_i32..32 { for x in 0_i32..32 {
        let dx = x - 16; let dy = y - 16;
        if dx * dx + dy * dy <= 225 { image.put_pixel(x as u32, y as u32, image::Rgba([27, 126, 76, 255])); }
        if (9..=23).contains(&x) && (4..=28).contains(&y) { image.put_pixel(x as u32, y as u32, image::Rgba([242, 244, 239, 255])); }
        if (11..=21).contains(&x) && (6..=26).contains(&y) { image.put_pixel(x as u32, y as u32, image::Rgba([35, 39, 43, 255])); }
        if (12..=20).contains(&x) && (7..=12).contains(&y) { image.put_pixel(x as u32, y as u32, image::Rgba([161, 220, 220, 255])); }
        if [13, 16, 19].contains(&x) && [16, 19, 22, 25].contains(&y) { image.put_pixel(x as u32, y as u32, image::Rgba([242, 244, 239, 255])); }
    }}
    let icon = Icon::from_rgba(image.into_raw(), 32, 32).map_err(|e| e.to_string())?;
    let menu = Menu::new();
    let open = MenuItem::new("Open", true, None);
    let quit = MenuItem::new("Quit", true, None);
    menu.append_items(&[&open, &quit]).map_err(|e| e.to_string())?;
    let open_id = open.id().clone(); let quit_id = quit.id().clone();
    let context = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) {
            context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            context.send_viewport_cmd(egui::ViewportCommand::Focus);
            context.request_repaint();
        }
    }));
    let context = ctx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == open_id {
            context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            context.send_viewport_cmd(egui::ViewportCommand::Focus);
            context.request_repaint();
        } else if event.id == quit_id { context.send_viewport_cmd(egui::ViewportCommand::Close); context.request_repaint(); }
    }));
    TrayIconBuilder::new().with_icon(icon).with_menu(Box::new(menu)).with_tooltip(branding::APP_NAME).build().map_err(|e| e.to_string())
}

#[cfg(test)]
mod project_tests {
    use super::*;
    #[test]
    fn printer_status_updates_do_not_change_the_label_or_retain_stale_details() {
        let mut app = LabelApp { text: "Keep this label".into(), ..Default::default() };
        let document = app.document_key();
        let connected = sdk::DeviceStatus { connected: true, available: true, battery_percent: Some(75), charging: Some(true), tape_width_mm: Some(9), issues: vec![], message: None };
        app.apply_device_status(Ok(connected.clone()));
        assert_eq!(app.service_status, "Connected");
        assert!(app.device_status_time.is_some());
        for status in [sdk::DeviceStatus::unavailable(), sdk::DeviceStatus::disconnected()] {
            app.apply_device_status(Ok(status.clone()));
            assert_eq!(app.connected, Some(status.connected));
            assert_eq!(app.device_status.as_ref().unwrap().battery_percent, None);
            assert!(app.device_status_time.is_none());
            assert_eq!(app.document_key(), document);
        }
        app.apply_device_status(Ok(connected));
        app.apply_device_status(Err("Synthetic status failure".into()));
        assert_eq!(app.connected, None);
        assert!(app.device_status.is_none() && app.device_status_time.is_none());
        assert_eq!(app.document_key(), document);
    }

    #[test]
    fn printer_details_render_in_compact_and_settings_views() {
        let mut app = LabelApp::default();
        for status in [
            sdk::DeviceStatus { connected: true, available: true, battery_percent: Some(75), charging: Some(true), tape_width_mm: Some(12), issues: vec!["Synthetic printer error".into()], message: None },
            sdk::DeviceStatus::unavailable(), sdk::DeviceStatus::disconnected(),
        ] {
            app.apply_device_status(Ok(status));
            egui::__run_test_ui(|ui| {
                ui.set_width(360.0);
                app.device_status_ui(ui, false);
                app.device_status_ui(ui, true);
            });
        }
    }

    #[test]
    fn app_update_results_follow_the_current_repository_without_changing_the_label() {
        for current_repository in ["example/label-app", "example/other-app"] {
            let mut app = LabelApp { app_update_repository: current_repository.into(), text: "Keep this label".into(), ..Default::default() };
            app.next_status_check = std::time::Instant::now() + std::time::Duration::from_secs(60);
            let (send, receive) = std::sync::mpsc::channel();
            app.app_update_receiver = Some(receive);
            send.send(("example/label-app".into(), Ok(app_updates::Release { version: "1.0.0".into(), notes: "Synthetic notes".into(), url: "https://github.com/example/label-app/releases/tag/v1.0.0".into(), newer: true, package: None }))).unwrap();
            egui::__run_test_ui(|ui| { ui.set_width(1000.0); app.draw(ui); });
            assert_eq!(app.text, "Keep this label");
            assert_eq!(app.app_update_release.is_some(), current_repository == "example/label-app");
            assert!(app.app_update_receiver.is_none());
        }
    }
    fn finish_update_check(app: &mut LabelApp) {
        app.next_status_check = std::time::Instant::now() + std::time::Duration::from_secs(600);
        for _ in 0..100 {
            egui::__run_test_ui(|ui| { ui.set_width(1000.0); app.draw(ui); });
            if app.app_update_receiver.is_none() { return; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("synthetic update worker did not finish");
    }
    fn synthetic_update(version: &str, newer: bool) -> app_updates::Release {
        app_updates::Release { version: version.into(), notes: "Synthetic release notes".into(), url: format!("https://github.com/example/label-app/releases/tag/v{version}"), newer, package: None }
    }
    #[test]
    fn startup_checks_once_and_notifies_without_changing_work_or_a_verified_download() {
        let mut app = LabelApp { app_update_repository: "example/label-app".into(), text: "Keep this label".into(), ..Default::default() };
        app.app_update_prepared = Some(update_package::Prepared { version: "9.0.0".into(), install: std::path::PathBuf::from("synthetic"), files: Vec::new() });
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)); let count = calls.clone();
        app.start_startup_update_check_with(move |repository| {
            assert_eq!(repository, "example/label-app"); count.fetch_add(1, std::sync::atomic::Ordering::Relaxed); Ok(synthetic_update("9.0.0", true))
        });
        assert!(app.app_update_receiver.is_some()); finish_update_check(&mut app);
        assert_eq!(app.app_update_notice_version(), Some("9.0.0")); assert_eq!(app.text, "Keep this label");
        assert_eq!(app.app_update_prepared.as_ref().unwrap().version, "9.0.0"); assert!(app.app_update_download.is_none()); assert!(!app.app_update_restarting);
        let count = calls.clone(); app.start_startup_update_check_with(move |_| { count.fetch_add(1, std::sync::atomic::Ordering::Relaxed); Ok(synthetic_update("9.0.0", true)) });
        assert!(app.app_update_receiver.is_none()); assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
    #[test]
    fn disabled_or_invalid_startup_update_settings_never_start_a_worker() {
        for repository in ["", "  ", "invalid repository"] {
            let mut app = LabelApp { app_update_repository: repository.into(), ..Default::default() };
            let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)); let count = calls.clone();
            app.start_startup_update_check_with(move |_| { count.fetch_add(1, std::sync::atomic::Ordering::Relaxed); Err("should not run".into()) });
            assert!(app.app_update_receiver.is_none()); assert!(app.app_update_startup_checked); assert!(app.app_update_notice_version().is_none()); assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0);
        }
    }
    #[test]
    fn up_to_date_and_offline_startup_checks_do_not_raise_update_notices() {
        for offline in [false, true] {
            let mut app = LabelApp { app_update_repository: "example/label-app".into(), text: "Keep this label".into(), ..Default::default() };
            app.start_startup_update_check_with(move |_| if offline { Err("Synthetic offline result".into()) } else { Ok(synthetic_update("0.1.0", false)) });
            finish_update_check(&mut app); assert!(app.app_update_notice_version().is_none()); assert_eq!(app.text, "Keep this label"); assert!(!app.show_preferences);
            if offline { assert!(app.app_update_status.contains("offline")); }
        }
    }
    #[test]
    fn dismissing_an_update_is_per_version_and_does_not_disable_future_launch_notices() {
        let mut app = LabelApp::default(); app.app_update_release = Some(synthetic_update("9.0.0", true));
        assert_eq!(app.app_update_notice_version(), Some("9.0.0")); app.app_update_dismissed_version = Some("9.0.0".into()); assert!(app.app_update_notice_version().is_none());
        app.app_update_release = Some(synthetic_update("9.0.1", true)); assert_eq!(app.app_update_notice_version(), Some("9.0.1"));
        let mut reopened = LabelApp::default(); reopened.app_update_release = Some(synthetic_update("9.0.0", true)); assert_eq!(reopened.app_update_notice_version(), Some("9.0.0"));
    }
    #[test]
    fn background_update_results_preserve_the_startup_recovery_message() {
        let mut app = LabelApp { app_update_repository: "example/label-app".into(), app_update_startup_message: Some("Synthetic interrupted update was recovered.".into()), ..Default::default() };
        app.start_startup_update_check_with(|_| Ok(synthetic_update("9.0.0", true))); finish_update_check(&mut app);
        assert_eq!(app.app_update_startup_message.as_deref(), Some("Synthetic interrupted update was recovered.")); assert!(app.app_update_status.contains("9.0.0"));
    }
    #[test]
    fn viewing_a_notice_opens_update_details_without_downloading_or_closing() {
        let mut app = LabelApp::default(); app.app_update_release = Some(synthetic_update("9.0.0", true));
        app.view_app_update(); assert!(app.show_preferences); assert!(app.app_update_scroll_requested); assert!(app.app_update_notice_version().is_none());
        assert!(app.app_update_download.is_none()); assert!(!app.app_update_restarting); assert!(!app.allow_close);
    }
    #[test]
    fn final_exit_guard_preserves_a_print_started_after_the_unsaved_prompt() {
        let mut app = LabelApp::default();
        let (_sender, receiver) = std::sync::mpsc::channel();
        app.print_receiver = Some(receiver);
        let ctx = egui::Context::default();
        app.perform_document_action(PendingDocumentAction::Exit, &ctx);
        assert!(!app.allow_close, "a late print job must block confirmed exit");
        assert!(app.print_status.contains("current print job"));
        app.print_receiver = None;
        app.perform_document_action(PendingDocumentAction::Exit, &ctx);
        assert!(app.allow_close);
    }
    #[test]
    fn update_restart_preserves_unsaved_work_until_the_user_decides() {
        let mut app = LabelApp { text: "Unsaved label".into(), ..Default::default() };
        app.request_document_action(PendingDocumentAction::InstallUpdate, &egui::Context::default());
        assert!(matches!(app.pending_document_action, Some(PendingDocumentAction::InstallUpdate)));
        assert_eq!(app.text, "Unsaved label"); assert!(!app.allow_close); assert!(!app.app_update_restarting);
        app.pending_document_action = None;
        assert_eq!(app.text, "Unsaved label"); assert!(!app.allow_close);
    }
    #[test]
    fn update_restart_rechecks_late_printer_work_before_starting_a_helper() {
        let mut app = LabelApp::default(); let ctx = egui::Context::default();
        let (_send, receive) = std::sync::mpsc::channel(); app.print_receiver = Some(receive);
        app.perform_document_action(PendingDocumentAction::InstallUpdate, &ctx);
        assert!(!app.allow_close); assert!(!app.app_update_restarting); assert!(app.app_update_install_receiver.is_none());
        app.print_receiver = None;
        let (_send, receive) = std::sync::mpsc::channel(); app.preview_receiver = Some(receive);
        app.perform_document_action(PendingDocumentAction::InstallUpdate, &ctx);
        assert!(!app.allow_close); assert!(!app.app_update_restarting); assert!(app.app_update_install_receiver.is_none());
    }
    #[test]
    fn closing_cancels_a_download_and_waits_for_it_without_discarding_a_label() {
        let mut app = LabelApp { text: "Keep this work".into(), ..Default::default() };
        let (send, receive) = std::sync::mpsc::channel(); app.app_update_download = Some(receive);
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)); app.app_update_cancel = Some(cancel.clone());
        app.request_document_action(PendingDocumentAction::Exit, &egui::Context::default());
        assert!(cancel.load(std::sync::atomic::Ordering::Acquire)); assert!(!app.allow_close); assert!(app.pending_document_action.is_none());
        send.send(update_package::DownloadEvent::Finished(Err("Cancelled".into()))).unwrap();
        app.poll_app_update_download(&egui::Context::default());
        assert!(matches!(app.pending_document_action, Some(PendingDocumentAction::Exit))); assert_eq!(app.text, "Keep this work"); assert!(!app.allow_close);
    }
    #[test]
    fn a_cancelled_download_cannot_become_an_installable_update() {
        let mut app = LabelApp::default(); let (send, receive) = std::sync::mpsc::channel(); app.app_update_download = Some(receive);
        app.app_update_cancel = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)));
        send.send(update_package::DownloadEvent::Finished(Ok(update_package::Prepared { version: "9.0.0".into(), install: std::path::PathBuf::from("synthetic"), files: Vec::new() }))).unwrap();
        app.poll_app_update_download(&egui::Context::default()); assert!(app.app_update_prepared.is_none()); assert!(app.app_update_download.is_none());
    }
    #[test]
    fn a_restart_helper_error_keeps_the_editor_open_and_the_label_intact() {
        let mut app = LabelApp { text: "Keep this label".into(), app_update_restarting: true, ..Default::default() };
        app.next_status_check = std::time::Instant::now() + std::time::Duration::from_secs(600);
        let (send, receive) = std::sync::mpsc::channel(); app.app_update_install_receiver = Some(receive);
        send.send(Err("Synthetic permission failure".into())).unwrap();
        egui::__run_test_ui(|ui| { ui.set_width(1000.0); app.draw(ui); });
        assert!(!app.allow_close); assert!(!app.app_update_restarting); assert_eq!(app.text, "Keep this label"); assert!(app.app_update_status.contains("Synthetic permission failure"));
    }
    #[test]
    fn full_editor_primary_drag_moves_shape_with_empty_text_and_nested_scroll_area() {
        let mut app = LabelApp { template_path: "synthetic.dymo".into(), design_mode: false, zoom: 1.0, ..Default::default() };
        app.next_status_check = std::time::Instant::now() + std::time::Duration::from_secs(600);
        app.features.add(features::FeatureKind::Shape, 9).unwrap();
        app.guides.set_snapping(false);
        app.preview_dimensions = Some((40.866664338111875, 9.03111057281494));
        app.printable_origin = Some((10.583333081007003, 0.9877777189016341));
        app.printable_dimensions = Some((19.699999690055847, 7.055555891990661));
        app.all_layouts = Some(serde_json::json!([
            {"Name":"TextObject0","Kind":"TextObject","Xmm":0,"Ymm":0,"WidthMm":12.7,"HeightMm":7.0555559,"Layer":0,"IsValid":true,"InCanvas":true},
            {"Name":"Extra1","Kind":"ShapeObject","Xmm":12.69999924,"Ymm":0,"WidthMm":6.99999969,"HeightMm":7.0555559,"Layer":1,"IsValid":true,"InCanvas":true}
        ]));
        app.empty_primary_text = Some("TextObject0".into());
        app.preview_snapshot = Some(sdk::test_snapshot()); app.preview_objects = Some(vec![]);
        app.preview_key = Some(app.key()); app.observed_key = app.key();
        let ctx = egui::Context::default();
        let mut frame = |events| {
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1280.0, 960.0))), events, ..Default::default() };
            let _ = ctx.run_ui(raw, |ui| {
                if app.preview_image.is_none() { app.preview_image = Some(ui.ctx().load_texture("synthetic-preview", egui::ColorImage::filled([290, 64], Color32::YELLOW), egui::TextureOptions::NEAREST)); }
                app.draw(ui);
            });
        };
        frame(vec![]);
        let rect = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("test-label-preview-rect"))).unwrap();
        let start = egui::pos2(rect.left() + rect.width() * (26.783333 / 40.866664) as f32, rect.center().y);
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        frame(vec![egui::Event::PointerMoved(start), button(start, true)]);
        let end = start - Vec2::new(25.0, 0.0);
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(vec![button(end, false)]);
        drop(frame);
        assert!(app.layout.is_manual(), "full editor should receive the primary drag, not the scroll area");
        let saved = app.layout.save_value();
        let shape = saved["objects"].as_array().unwrap().iter().find(|item| item["name"] == "Extra1").unwrap();
        assert!(shape["x"].as_f64().unwrap() < 12.0, "shape did not move: {saved}");
        assert!(app.pending_move.is_some());
        assert!(!app.can_print(), "moving invalidates the print snapshot until rerendered");
    }
    #[test]
    fn preview_context_targets_the_clicked_object_and_deletes_only_that_identity() {
        let mut app = LabelApp { text: "Keep this text".into(), image_path: "image.png".into(), design_mode: false, ..Default::default() };
        app.features.add(features::FeatureKind::Shape, 9).unwrap();
        app.preview_dimensions = Some((40.0, 9.0));
        app.printable_dimensions = Some((40.0, 9.0));
        app.printable_origin = Some((0.0, 0.0));
        app.all_layouts = Some(serde_json::json!([
            {"Name":"TextObject0","Kind":"TextObject","Xmm":0,"Ymm":0,"WidthMm":8,"HeightMm":7,"Layer":0},
            {"Name":"Extra1","Kind":"ShapeObject","Xmm":10,"Ymm":0,"WidthMm":8,"HeightMm":7,"Layer":1},
            {"Name":"ImageObject0","Kind":"ImageObject","Xmm":20,"Ymm":0,"WidthMm":8,"HeightMm":7,"Layer":2}
        ]));
        app.preview_key = Some(app.key());
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(400.0, 90.0));
        assert_eq!(app.preview_target_at(egui::pos2(30.0, 30.0), rect), Some(PreviewContextTarget::PrimaryText("TextObject0".into())));
        let shape = app.preview_target_at(egui::pos2(130.0, 30.0), rect).unwrap();
        assert_eq!(shape, PreviewContextTarget::Feature("Extra1".into()));
        let image = app.preview_target_at(egui::pos2(230.0, 30.0), rect).unwrap();
        assert_eq!(image, PreviewContextTarget::Image("ImageObject0".into()));
        app.delete_preview_target(&shape);
        assert!(app.features.is_empty());
        assert_eq!(app.text, "Keep this text");
        assert_eq!(app.image_path, "image.png");
        assert!(app.preview_target_at(egui::pos2(230.0, 30.0), rect).is_none(), "stale geometry must not select an object");
        app.delete_preview_target(&image);
        assert!(app.image_path.is_empty());
        assert_eq!(app.text, "Keep this text");
    }
    #[test]
    fn editor_renders_with_object_tools_and_an_added_shape() {
        let mut app = LabelApp::default();
        app.next_status_check = std::time::Instant::now() + std::time::Duration::from_secs(60);
        app.features.add(features::FeatureKind::Shape, 9).unwrap();
        app.design_mode = false;
        egui::__run_test_ui(|ui| { ui.set_width(1000.0); app.draw(ui); });
        assert!(!app.features.is_empty());
    }
    #[test]
    fn move_keeps_old_preview_visible_but_invalidates_print_snapshot() {
        let mut app = LabelApp::default();
        app.next_status_check = std::time::Instant::now() + std::time::Duration::from_secs(60);
        app.preview_snapshot = Some(sdk::test_snapshot());
        app.preview_objects = Some(vec![]);
        app.preview_key = Some("old key".into());
        app.observed_key = "old key".into();
        app.pending_move = Some(canvas_layout::MoveVisual { object: canvas_layout::Object { name: "shape".into(), x: 1.0, y: 0.0, width: 2.0, height: 2.0, layer: 0 }, x: 2.0, y: 0.0 });
        egui::__run_test_ui(|ui| {
            app.preview_image = Some(ui.ctx().load_texture("test-old-preview", egui::ColorImage::filled([8, 8], Color32::YELLOW), egui::TextureOptions::NEAREST));
            app.draw(ui);
        });
        assert!(app.preview_image.is_some(), "the old preview must remain visible while the SDK renders");
        assert!(app.preview_key.is_none() && app.preview_snapshot.is_none(), "a stale preview must never be printed");
    }
    #[test]
    fn threshold_change_keeps_previous_image_visible_but_not_printable() {
        let mut app = LabelApp { template_path: "example.dymo".into(), image_path: "example.png".into(), connected: Some(true), ..Default::default() };
        app.next_status_check = std::time::Instant::now() + std::time::Duration::from_secs(60);
        app.preview_snapshot = Some(sdk::test_snapshot());
        app.preview_objects = Some(vec![]);
        app.preview_key = Some(app.key());
        app.observed_key = app.key();
        app.preview_base_key = Some(app.threshold_base_key());
        app.threshold = 200;
        egui::__run_test_ui(|ui| {
            app.preview_image = Some(ui.ctx().load_texture("threshold-old-preview", egui::ColorImage::filled([8, 8], Color32::YELLOW), egui::TextureOptions::NEAREST));
            app.draw(ui);
        });
        assert!(app.preview_image.is_some());
        assert!(app.preview_status.contains("Previous preview"), "{}", app.preview_status);
        assert!(!app.can_print());
        assert!(app.preview_snapshot.is_none());
    }
    #[test]
    fn formatting_dropdowns_share_the_same_vertical_bounds() {
        egui::__run_test_ui(|ui| {
            ui.set_width(700.0);
            let (mut bold, mut italic, mut underline) = (false, false, false);
            let mut font = "Segoe UI".to_string();
            let mut align = "Center".to_string();
            let fonts = vec![font.clone()];
            let (first, second) = formatting_controls(ui, &mut bold, &mut italic, &mut underline, &mut font, &fonts, &mut align);
            assert!((first.top() - second.top()).abs() < 0.01, "formatting controls must have the same top edge");
            assert!((first.bottom() - second.bottom()).abs() < 0.01, "formatting controls must have the same bottom edge");
        });
    }
    #[test]
    fn added_objects_require_dymo_preview_and_counters_are_planned() {
        let mut app = LabelApp { connected: Some(true), ..Default::default() };
        app.features.add(features::FeatureKind::Shape, 9).unwrap();
        app.preview_key = Some(app.key());
        app.preview_snapshot = Some(sdk::test_snapshot());
        app.preview_objects = Some(vec![]);
        assert!(!app.can_print(), "the smooth design does not show SDK objects");
        app.design_mode = false;
        assert!(app.can_print());
        app.features.add(features::FeatureKind::Counter, 9).unwrap();
        app.preview_key = Some(app.key());
        app.preview_objects = Some(app.features.request_value(9).unwrap().as_array().unwrap().clone());
        app.copies = 2;
        assert!(app.can_print());
        let objects = app.features.request_value(9).unwrap().as_array().unwrap().clone();
        assert_eq!(sequence::plan(&objects, app.copies).unwrap().labels.len(), 2);
        app.copies = 1;
        assert!(app.can_print());
    }
    #[test]
    fn prepared_rows_reuse_the_captured_preview_objects() {
        let mut app = LabelApp { connected: Some(true), text: "Item".into(), ..Default::default() };
        app.features.add(features::FeatureKind::Date, 9).unwrap();
        assert!(app.features.request_value(9).is_err(), "captured values must not be generated at print time");
        let objects = app.features.capture_request(9).unwrap().as_array().unwrap().clone();
        app.preview_snapshot = Some(sdk::test_snapshot());
        app.preview_objects = Some(objects.clone());
        app.preview_key = Some(app.key());
        app.design_mode = false;
        app.prepared_batch = vec!["one".into(), "two".into()];
        assert!(app.can_print());
        let jobs = app.prepared_batch_jobs().unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].objects, objects);
        assert_eq!(jobs[1].objects, objects);
    }
    #[test]
    fn context_edits_handle_unicode_selection_and_character_limit() {
        let mut text = "A£猫B".to_owned();
        assert_eq!(text_range(&text, 1..3), "£猫");
        assert_eq!(replace_text_range(&mut text, 1..3, "é"), 2);
        assert_eq!(text, "AéB");
        let mut text = "x".repeat(499);
        assert_eq!(replace_text_range(&mut text, 499..499, "猫🐈"), 500);
        assert_eq!(text.chars().count(), 500);
        assert!(text.ends_with('猫'));
    }
    #[test]
    fn saved_label_round_trips_and_refuses_overwrite() {
        let path = std::env::temp_dir().join(format!("dymo420p-test-{}-{}.420p", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let feature_data = serde_json::json!([{"kind":"qr", "value":"test content"}]);
        let features = features::Features::load_value(&feature_data, 19).unwrap();
        let mut original = LabelApp { text: "A & B".into(), width_mm: 19, template_path: "template.dymo".into(), bold: true, features, ..Default::default() };
        original.save_as(&path).unwrap();
        assert!(original.save_as(&path).is_err());
        let mut loaded = LabelApp::default();
        loaded.load_project(&path).unwrap();
        assert_eq!(loaded.text, "A & B"); assert_eq!(loaded.width_mm, 19); assert!(loaded.bold);
        assert_eq!(loaded.features.save_value(), original.features.save_value());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn confirmed_save_as_replaces_only_the_expected_file() {
        let path = std::env::temp_dir().join(format!("dymo420p-save-as-{}-{}.420p", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut app = LabelApp { text: "Initial".into(), ..Default::default() };
        app.save_as(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        app.text = "Replacement".into();
        app.save_as_expected(&path, Some(original)).unwrap();
        let saved = std::fs::read(&path).unwrap();
        assert!(String::from_utf8_lossy(&saved).contains("Replacement"));
        assert_eq!(app.saved_disk_snapshot.as_ref(), Some(&saved));
        assert_eq!(app.project_path.as_deref(), Some(path.as_path()));
        assert_eq!(app.saved_key, app.document_key());

        std::fs::write(&path, "external change").unwrap();
        app.text = "Must not replace external change".into();
        assert!(app.save_as_expected(&path, Some(saved.clone())).unwrap_err().contains("changed while preparing"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external change");
        assert_eq!(app.saved_disk_snapshot.as_ref(), Some(&saved));
        assert_ne!(app.saved_key, app.document_key());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn guides_and_manual_object_identities_survive_project_save_and_reopen() {
        let path = std::env::temp_dir().join(format!("dymo-layout-project-{}-{}.420p", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut app = LabelApp::default();
        app.features = features::Features::load_value(&serde_json::json!([{"kind":"shape","name":"Extra2","variant":"Rectangle","value":""}]), 9).unwrap();
        let snapshot = serde_json::json!([
            {"Name":"TextObject0","Kind":"TextObject","Xmm":1.0,"Ymm":0.0,"WidthMm":8.0,"HeightMm":5.0,"Layer":0,"IsValid":true,"InCanvas":true},
            {"Name":"Extra2","Kind":"ShapeObject","Xmm":12.0,"Ymm":0.0,"WidthMm":5.0,"HeightMm":5.0,"Layer":1,"IsValid":true,"InCanvas":true}
        ]);
        app.layout.enable_manual(&snapshot).unwrap();
        app.guides = guides::Guides::load_value(&serde_json::json!({"version":1,"visible":true,"snap":true,"verticalMm":[10.0],"horizontalMm":[2.0]})).unwrap();
        app.removed_objects.push("ImageObject0".into());
        app.save_as(&path).unwrap();
        let mut reopened = LabelApp::default(); reopened.load_project(&path).unwrap();
        assert_eq!(reopened.features.names(), vec!["Extra2"]);
        assert_eq!(reopened.layout.save_value(), app.layout.save_value());
        assert_eq!(reopened.guides.save_value(), app.guides.save_value());
        assert_eq!(reopened.removed_objects, vec!["ImageObject0"]);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn save_detects_external_changes_before_replacing_a_label() {
        let path = std::env::temp_dir().join(format!("dymo420p-conflict-{}-{}.420p", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut app = LabelApp { text: "Initial".into(), ..Default::default() };
        app.save_as(&path).unwrap();
        app.text = "Edited in app".into();
        app.save_current().unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("Edited in app"));
        std::fs::write(&path, "external update").unwrap();
        app.text = "Another change".into();
        assert!(app.save_current().unwrap_err().contains("changed outside"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external update");
        std::fs::remove_file(path).unwrap();
    }
}

fn main() -> eframe::Result {
    let (acknowledge, update_message, _instance) = match updater::startup() {
        Ok(updater::Startup::Run { acknowledge, message, instance }) => (acknowledge, message, instance),
        Ok(updater::Startup::Exit) => return Ok(()),
        Err(error) => { updater::show_error(&error); return Ok(()); }
    };
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/printer-420p-v6.png"))
        .expect("bundled DYMO icon must be a valid PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_icon(std::sync::Arc::new(icon))
            .with_inner_size([1060.0, 760.0])
            .with_min_inner_size([720.0, 560.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        branding::APP_NAME,
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            let mut style = (*cc.egui_ctx.style_of(egui::Theme::Dark)).clone();
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(10.0, 6.0);
            style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
            style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
            style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
            style.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
            cc.egui_ctx.set_style_of(egui::Theme::Dark, style);
            let mut app = LabelApp::default();
            app.app_update_acknowledge = acknowledge;
            if let Some(message) = update_message { app.app_update_startup_message = Some(message.clone()); app.app_update_status = message; }
            let update_preferences_loaded = match app_updates::load_repository() {
                Ok(repository) => { app.app_update_repository = repository; true }
                Err(error) => { app.app_update_status = error; false }
            };
            if app_updates::normalise_repository(&app.app_update_repository).ok().is_some_and(|repo| repo.eq_ignore_ascii_case(app_updates::DEFAULT_REPOSITORY)) {
                match update_package::install_directory().and_then(|path| update_package::resume(&path)) {
                    Ok(Some(prepared)) => { app.app_update_status = format!("Version {} is already downloaded and verified. Restart when ready.", prepared.version); app.app_update_prepared = Some(prepared); }
                    Ok(None) => {}
                    Err(_) => app.app_update_status = "A previous download could not be verified. Check for updates and download it again.".into(),
                }
            }
            match create_tray(&cc.egui_ctx) { Ok(tray) => app.tray = Some(tray), Err(e) => app.tray_error = Some(e) };
            let (send, receive) = std::sync::mpsc::channel();
            app.fonts_receiver = Some(receive);
            std::thread::spawn(move || { let _ = send.send(sdk::installed_fonts()); });
            if update_preferences_loaded { app.start_startup_update_check(); }
            Ok(Box::new(app))
        }),
    )
}
