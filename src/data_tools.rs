//! Local CSV import and contact-list support.
//!
//! Contact files are chosen by the user and never uploaded. CSV import is
//! deliberately separates preparing rows from printing; the caller must check
//! the printer and preflight every row. Spreadsheet formats are not parsed here.
use eframe::egui::{self, RichText};
use serde_json::{Value, json};
use std::{fs, io::Read, path::{Path, PathBuf}};

const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_ROWS: usize = 10_000;
const MAX_COLUMNS: usize = 256;
const MAX_CELL_BYTES: usize = 64 * 1024;
const MAX_CONTACTS: usize = 10_000;
const MAX_LABEL_CHARS: usize = 500;
const MAX_BATCH_ROWS: usize = 100;

fn valid_xml_text(text: &str) -> bool {
    text.chars().all(|ch| {
        let code = ch as u32;
        matches!(code, 0x09 | 0x0a | 0x0d) || (0x20..=0xd7ff).contains(&code)
            || (0xe000..=0xfffd).contains(&code) || (0x10000..=0x10ffff).contains(&code)
    })
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|e| format!("Could not open the file: {e}"))?;
    if file.metadata().map_err(|e| format!("Could not inspect the file: {e}"))?.len() > MAX_FILE_BYTES as u64 {
        return Err("The file is larger than 2 MiB.".into());
    }
    let mut data = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64).read_to_end(&mut data).map_err(|e| format!("Could not read the file: {e}"))?;
    if data.len() > MAX_FILE_BYTES { return Err("The file is larger than 2 MiB.".into()); }
    Ok(data)
}

/// Parsed comma-, semicolon- or tab-delimited UTF-8 data. The first row may be
/// used as column headings; its cells are retained so that choice is reversible.
#[derive(Debug, Clone)]
pub struct CsvTable { rows: Vec<Vec<String>>, columns: usize }

impl CsvTable {
    pub fn read(path: &Path, delimiter: u8) -> Result<Self, String> { Self::parse(&read_bounded(path)?, delimiter) }

    pub fn parse(bytes: &[u8], delimiter: u8) -> Result<Self, String> {
        if bytes.len() > MAX_FILE_BYTES { return Err("The CSV file is larger than 2 MiB.".into()); }
        if ![b',', b';', b'\t'].contains(&delimiter) { return Err("Unsupported delimiter.".into()); }
        let input = std::str::from_utf8(bytes).map_err(|_| "The data file must be UTF-8 encoded.".to_owned())?.trim_start_matches('\u{feff}');
        if input.is_empty() { return Err("The data file is empty.".into()); }
        let mut chars = input.chars().peekable();
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut closed_quote = false;
        let mut at_field_start = true;
        let mut ended_row = false;
        while let Some(ch) = chars.next() {
            ended_row = false;
            if quoted {
                if ch == '"' {
                    if chars.peek() == Some(&'"') { chars.next(); field.push('"'); }
                    else { quoted = false; closed_quote = true; }
                } else { field.push(ch); }
            } else if ch == delimiter as char {
                push_field(&mut row, &mut field)?; at_field_start = true; closed_quote = false;
            } else if ch == '\n' || ch == '\r' {
                if ch == '\r' && chars.peek() == Some(&'\n') { chars.next(); }
                push_field(&mut row, &mut field)?; push_row(&mut rows, &mut row)?;
                at_field_start = true; closed_quote = false; ended_row = true;
            } else if ch == '"' && at_field_start {
                quoted = true; at_field_start = false;
            } else if ch == '"' || closed_quote {
                return Err("The CSV contains an unexpected quote or text after a closing quote.".into());
            } else {
                field.push(ch); at_field_start = false;
            }
            if field.len() > MAX_CELL_BYTES { return Err("A CSV cell is too large.".into()); }
        }
        if quoted { return Err("The CSV ends inside a quoted field.".into()); }
        if !ended_row { push_field(&mut row, &mut field)?; push_row(&mut rows, &mut row)?; }
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 { return Err("The data file has no columns.".into()); }
        Ok(Self { rows, columns })
    }

    pub fn column_count(&self) -> usize { self.columns }
    pub fn row_count(&self, headings: bool) -> usize { self.rows.len().saturating_sub(usize::from(headings)) }
    pub fn column_name(&self, column: usize, headings: bool) -> String {
        if headings { if let Some(name) = self.rows.first().and_then(|row| row.get(column)).filter(|name| !name.trim().is_empty()) { return name.clone(); } }
        format!("Column {}", column + 1)
    }
    pub fn row_text(&self, index: usize, headings: bool, selected: &[bool]) -> Result<String, String> {
        let row = self.rows.get(index + usize::from(headings)).ok_or("That row no longer exists.")?;
        let parts = row.iter().enumerate().filter(|(i, value)| selected.get(*i).copied().unwrap_or(false) && !value.trim().is_empty()).map(|(_, value)| value.as_str()).collect::<Vec<_>>();
        if parts.is_empty() { return Err("Select a column containing text in this row.".into()); }
        let text = parts.join("\n");
        if text.chars().count() > MAX_LABEL_CHARS { return Err("The selected row is longer than the 500-character label limit.".into()); }
        if !valid_xml_text(&text) { return Err("The selected row contains characters the DYMO label format cannot represent.".into()); }
        Ok(text)
    }
    /// Return an explicitly selected inclusive row range. Nothing is printed or
    /// changed; every row must later receive its own SDK preview and preflight.
    pub fn prepare_batch(&self, headings: bool, selected: &[bool], first: usize, last: usize) -> Result<Vec<String>, String> {
        let count = self.row_count(headings);
        if first > last || last >= count { return Err("Select a valid range of data rows.".into()); }
        if last - first + 1 > MAX_BATCH_ROWS { return Err(format!("A batch can contain at most {MAX_BATCH_ROWS} rows.")); }
        (first..=last).map(|index| self.row_text(index, headings, selected).map_err(|error| format!("Row {}: {error}", index + 1))).collect()
    }
}

fn push_field(row: &mut Vec<String>, field: &mut String) -> Result<(), String> {
    if row.len() >= MAX_COLUMNS { return Err("The data file has more than 256 columns.".into()); }
    row.push(std::mem::take(field)); Ok(())
}
fn push_row(rows: &mut Vec<Vec<String>>, row: &mut Vec<String>) -> Result<(), String> {
    if rows.len() >= MAX_ROWS { return Err("The data file has more than 10,000 rows.".into()); }
    rows.push(std::mem::take(row)); Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contact { pub name: String, pub address: String }

fn validate_contact(contact: &Contact) -> Result<(), String> {
    if contact.name.trim().is_empty() || contact.name.chars().count() > 120 { return Err("A contact needs a name of up to 120 characters.".into()); }
    if contact.address.trim().is_empty() || contact.address.chars().count() > MAX_LABEL_CHARS { return Err("An address must contain 1–500 characters.".into()); }
    if !valid_xml_text(&contact.name) || !valid_xml_text(&contact.address) { return Err("The contact contains unsupported control characters.".into()); }
    Ok(())
}

/// A local, explicitly chosen JSON contact file. Existing files are checked for
/// external changes before being replaced using the shared safe-save routine.
pub struct ContactBook {
    pub contacts: Vec<Contact>,
    pub path: PathBuf,
    snapshot: Option<Vec<u8>>,
    pub dirty: bool,
}
impl ContactBook {
    pub fn new(path: PathBuf) -> Self { Self { contacts: Vec::new(), path, snapshot: None, dirty: false } }
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let bytes = read_bounded(&path)?;
        let root: Value = serde_json::from_slice(&bytes).map_err(|e| format!("Invalid contact file: {e}"))?;
        let root_fields = root.as_object().ok_or("The contact file must contain an object.")?;
        if root_fields.keys().any(|key| key != "version" && key != "contacts") { return Err("The contact file contains unsupported fields; it will not be modified.".into()); }
        if root["version"].as_u64() != Some(1) { return Err("Unsupported contact file version.".into()); }
        let records = root["contacts"].as_array().ok_or("The contact list is missing.")?;
        if records.len() > MAX_CONTACTS { return Err("The contact file contains too many contacts.".into()); }
        let mut contacts = Vec::with_capacity(records.len());
        for record in records {
            let fields = record.as_object().ok_or("Each contact must be an object.")?;
            if fields.keys().any(|key| key != "name" && key != "address") { return Err("A contact contains unsupported fields; the file will not be modified.".into()); }
            let name = record["name"].as_str().ok_or("A contact name is missing.")?;
            let address = record["address"].as_str().ok_or("A contact address is missing.")?;
            let contact = Contact { name: name.to_owned(), address: address.to_owned() };
            validate_contact(&contact)?; contacts.push(contact);
        }
        Ok(Self { contacts, path, snapshot: Some(bytes), dirty: false })
    }
    pub fn save(&mut self) -> Result<(), String> {
        if self.contacts.len() > MAX_CONTACTS { return Err("The contact list is too large.".into()); }
        for contact in &self.contacts { validate_contact(contact)?; }
        let contents = json!({"version":1,"contacts":self.contacts.iter().map(|c| json!({"name":c.name,"address":c.address})).collect::<Vec<_>>()});
        let bytes = serde_json::to_vec_pretty(&contents).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_FILE_BYTES { return Err("The contact list is larger than 2 MiB.".into()); }
        if let Some(previous) = &self.snapshot {
            let current = read_bounded(&self.path)?;
            if &current != previous { return Err("The contact file changed outside this app. Open it again before saving.".into()); }
        }
        crate::project_io::write_project(&self.path, &bytes, self.snapshot.is_some())?;
        self.snapshot = Some(bytes); self.dirty = false; Ok(())
    }
}

/// The parent should append this text to the current label only when doing so
/// does not exceed the editor's 500-character limit.
pub enum DataAction { AppendText(String), PrepareBatch(Vec<String>) }

pub struct DataTools {
    pub show_import: bool,
    pub show_contacts: bool,
    table: Option<CsvTable>,
    csv_delimiter: u8,
    csv_has_headings: bool,
    csv_columns: Vec<bool>,
    csv_row: usize,
    csv_batch_start: usize,
    csv_batch_end: usize,
    csv_status: String,
    contacts: Option<ContactBook>,
    selected_contact: Option<usize>,
    edit_name: String,
    edit_address: String,
    draft_dirty: bool,
    contact_status: String,
}
impl Default for DataTools {
    fn default() -> Self { Self { show_import: false, show_contacts: false, table: None, csv_delimiter: b',', csv_has_headings: true, csv_columns: Vec::new(), csv_row: 0, csv_batch_start: 1, csv_batch_end: 1, csv_status: String::new(), contacts: None, selected_contact: None, edit_name: String::new(), edit_address: String::new(), draft_dirty: false, contact_status: String::new() } }
}

impl DataTools {
    /// The parent should warn before exiting while this is true.
    pub fn has_unsaved_contacts(&self) -> bool { self.draft_dirty || self.contacts.as_ref().is_some_and(|book| book.dirty) }
    pub fn show(&mut self, ctx: &egui::Context) -> Option<DataAction> {
        let mut action = None;
        if self.show_import {
            let mut open = self.show_import;
            egui::Window::new("Import CSV data").open(&mut open).default_width(480.0).show(ctx, |ui| {
                ui.label("Select a UTF-8 CSV or TSV file. Choose columns and a row to append to the label.");
                let previous_delimiter = self.csv_delimiter;
                ui.horizontal(|ui| {
                    ui.label("Delimiter");
                    ui.selectable_value(&mut self.csv_delimiter, b',', "Comma");
                    ui.selectable_value(&mut self.csv_delimiter, b';', "Semicolon");
                    ui.selectable_value(&mut self.csv_delimiter, b'\t', "Tab");
                });
                if previous_delimiter != self.csv_delimiter && self.table.is_some() {
                    self.table = None; self.csv_columns.clear(); self.csv_row = 0;
                    self.csv_status = "Choose the data file again to apply the new delimiter.".into();
                }
                if ui.button("Choose data file…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("CSV or TSV", &["csv", "tsv", "txt"]).pick_file() {
                        match CsvTable::read(&path, self.csv_delimiter) {
                            Ok(table) => { self.csv_columns = vec![true; table.column_count()]; self.csv_row = 0; self.csv_batch_start = 1; self.csv_batch_end = table.row_count(self.csv_has_headings).clamp(1, MAX_BATCH_ROWS); self.csv_status = format!("Loaded {} rows.", table.row_count(self.csv_has_headings)); self.table = Some(table); }
                            Err(error) => { self.csv_status = error; self.table = None; }
                        }
                    }
                }
                ui.checkbox(&mut self.csv_has_headings, "First row contains column headings");
                if let Some(table) = &self.table {
                    let count = table.row_count(self.csv_has_headings);
                    if count > 0 { self.csv_row = self.csv_row.min(count - 1); }
                    ui.label(format!("{} data rows", count));
                    egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                        for col in 0..table.column_count() { ui.checkbox(&mut self.csv_columns[col], table.column_name(col, self.csv_has_headings)); }
                    });
                    if count > 0 {
                        let mut row_number = self.csv_row + 1;
                        ui.horizontal(|ui| { ui.label("Row"); ui.add(egui::DragValue::new(&mut row_number).range(1..=count)); });
                        self.csv_row = row_number - 1;
                        match table.row_text(self.csv_row, self.csv_has_headings, &self.csv_columns) {
                            Ok(text) => { ui.label(RichText::new(&text).small()); if ui.button("Append this row to label").clicked() { action = Some(DataAction::AppendText(text)); } }
                            Err(error) => { ui.label(error); }
                        }
                        self.csv_batch_start = self.csv_batch_start.clamp(1, count);
                        self.csv_batch_end = self.csv_batch_end.clamp(1, count);
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Batch rows");
                            ui.add(egui::DragValue::new(&mut self.csv_batch_start).range(1..=count));
                            ui.label("to");
                            ui.add(egui::DragValue::new(&mut self.csv_batch_end).range(1..=count));
                        });
                        if ui.button("Prepare batch (no printing)").clicked() {
                            match table.prepare_batch(self.csv_has_headings, &self.csv_columns, self.csv_batch_start - 1, self.csv_batch_end - 1) {
                                Ok(rows) => { self.csv_status = format!("Prepared {} rows. Review them before printing.", rows.len()); action = Some(DataAction::PrepareBatch(rows)); }
                                Err(error) => self.csv_status = error,
                            }
                        }
                    }
                }
                if !self.csv_status.is_empty() { ui.label(&self.csv_status); }
                ui.label(RichText::new("Preparing a batch does not print. Each row must be previewed and checked first.").small());
            });
            self.show_import = open;
        }
        if self.show_contacts {
            let mut open = self.show_contacts;
            egui::Window::new("Contacts").open(&mut open).default_width(480.0).show(ctx, |ui| {
                ui.label("Contacts stay in the local file you choose.");
                let can_switch = !self.draft_dirty && self.contacts.as_ref().is_none_or(|book| !book.dirty);
                ui.horizontal(|ui| {
                    if ui.add_enabled(can_switch, egui::Button::new("Open contacts…")).clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("Contacts", &["json"]).pick_file() {
                            match ContactBook::open(path) { Ok(book) => { self.contacts = Some(book); self.selected_contact = None; self.edit_name.clear(); self.edit_address.clear(); self.draft_dirty = false; self.contact_status = "Contacts opened.".into(); }, Err(error) => self.contact_status = error }
                        }
                    }
                    if ui.add_enabled(can_switch, egui::Button::new("New contacts file…")).clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("Contacts", &["json"]).set_file_name("contacts.json").save_file() {
                            self.contacts = Some(ContactBook::new(path)); self.selected_contact = None; self.edit_name.clear(); self.edit_address.clear(); self.draft_dirty = false; self.contact_status = "Choose a contact and save the new file.".into();
                        }
                    }
                });
                if let Some(book) = &mut self.contacts {
                    ui.label(book.path.file_name().and_then(|n| n.to_str()).unwrap_or("Contacts file")).on_hover_text(book.path.to_string_lossy());
                    if book.dirty || self.draft_dirty { ui.label(RichText::new("Unsaved contact changes").strong()); }
                    egui::ScrollArea::vertical().max_height(130.0).show(ui, |ui| {
                        for (index, contact) in book.contacts.iter().enumerate() {
                            if ui.add_enabled(!self.draft_dirty || self.selected_contact == Some(index), egui::Button::selectable(self.selected_contact == Some(index), &contact.name)).clicked() { self.selected_contact = Some(index); self.edit_name = contact.name.clone(); self.edit_address = contact.address.clone(); self.draft_dirty = false; }
                        }
                    });
                    ui.label("Name"); self.draft_dirty |= ui.add(egui::TextEdit::singleline(&mut self.edit_name).char_limit(120)).changed();
                    ui.label("Address"); self.draft_dirty |= ui.add(egui::TextEdit::multiline(&mut self.edit_address).desired_rows(3).char_limit(MAX_LABEL_CHARS)).changed();
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Add contact").clicked() {
                            let contact = Contact { name: self.edit_name.clone(), address: self.edit_address.clone() };
                            match validate_contact(&contact) {
                                Ok(()) if book.contacts.len() < MAX_CONTACTS => { book.contacts.push(contact); self.selected_contact = Some(book.contacts.len() - 1); book.dirty = true; self.draft_dirty = false; self.contact_status = "Contact added; save the file to keep it.".into(); }
                                Ok(()) => self.contact_status = "The contact list is full.".into(), Err(error) => self.contact_status = error,
                            }
                        }
                        let selected = self.selected_contact.filter(|index| *index < book.contacts.len());
                        if ui.add_enabled(selected.is_some(), egui::Button::new("Update selected")).clicked() {
                            let contact = Contact { name: self.edit_name.clone(), address: self.edit_address.clone() };
                            match validate_contact(&contact) { Ok(()) => { book.contacts[selected.unwrap()] = contact; book.dirty = true; self.draft_dirty = false; self.contact_status = "Contact updated; save the file to keep it.".into(); }, Err(error) => self.contact_status = error }
                        }
                        if ui.add_enabled(selected.is_some() && !self.draft_dirty, egui::Button::new("Remove selected")).clicked() { book.contacts.remove(selected.unwrap()); self.selected_contact = None; book.dirty = true; self.edit_name.clear(); self.edit_address.clear(); self.contact_status = "Contact removed; save the file to keep the change.".into(); }
                    });
                    if self.draft_dirty {
                        ui.label("Add or update the contact before switching entries.");
                        if ui.button("Discard contact draft").clicked() {
                            if let Some(contact) = self.selected_contact.and_then(|index| book.contacts.get(index)) { self.edit_name = contact.name.clone(); self.edit_address = contact.address.clone(); }
                            else { self.edit_name.clear(); self.edit_address.clear(); }
                            self.draft_dirty = false;
                        }
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(!self.draft_dirty && (book.dirty || book.snapshot.is_none()), egui::Button::new("Save contacts")).clicked() { self.contact_status = match book.save() { Ok(()) => "Contacts saved locally.".into(), Err(error) => error }; }
                        if ui.add_enabled(!self.draft_dirty && self.selected_contact.is_some(), egui::Button::new("Append selected address to label")).clicked() {
                            if let Some(contact) = self.selected_contact.and_then(|index| book.contacts.get(index)) { action = Some(DataAction::AppendText(contact.address.clone())); }
                        }
                    });
                    if book.dirty && ui.button("Discard unsaved contact changes").clicked() {
                        let result = if book.snapshot.is_some() { ContactBook::open(book.path.clone()) } else { Ok(ContactBook::new(book.path.clone())) };
                        match result {
                            Ok(restored) => { *book = restored; self.selected_contact = None; self.edit_name.clear(); self.edit_address.clear(); self.draft_dirty = false; self.contact_status = "Unsaved contact changes discarded.".into(); }
                            Err(error) => self.contact_status = error,
                        }
                    }
                }
                if !self.contact_status.is_empty() { ui.label(&self.contact_status); }
            });
            self.show_contacts = open;
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn csv_parses_bom_quotes_commas_and_embedded_newlines() {
        let csv = CsvTable::parse("\u{feff}Name,Address\r\n\"A, B\",\"First\nSecond\"\r\n".as_bytes(), b',').unwrap();
        assert_eq!(csv.column_count(), 2); assert_eq!(csv.row_count(true), 1); assert_eq!(csv.column_name(0, true), "Name");
        assert_eq!(csv.row_text(0, true, &[true, true]).unwrap(), "A, B\nFirst\nSecond");
    }
    #[test] fn csv_rejects_bad_quotes_and_oversized_selected_rows() {
        assert!(CsvTable::parse(b"a,\"unfinished", b',').is_err());
        assert!(CsvTable::parse(b"\"closed\"x", b',').is_err());
        let csv = CsvTable::parse("x".repeat(501).as_bytes(), b',').unwrap();
        assert!(csv.row_text(0, false, &[true]).is_err());
        let csv = CsvTable::parse(b"bad\0character", b',').unwrap();
        assert!(csv.row_text(0, false, &[true]).is_err());
    }
    #[test] fn csv_supports_missing_cells_and_explicit_delimiters() {
        let csv = CsvTable::parse(b"one;two\nleft;right\nlast", b';').unwrap();
        assert_eq!(csv.row_count(true), 2); assert_eq!(csv.row_text(1, true, &[true, true]).unwrap(), "last");
        assert!(csv.row_text(0, true, &[false, false]).is_err());
    }
    #[test] fn batch_validates_every_row_and_enforces_the_limit() {
        let csv = CsvTable::parse(b"title\none\ntwo\n", b',').unwrap();
        assert_eq!(csv.prepare_batch(true, &[true], 0, 1).unwrap(), vec!["one", "two"]);
        assert!(csv.prepare_batch(true, &[false], 0, 1).unwrap_err().contains("Row 1"));
        assert!(csv.prepare_batch(true, &[true], 1, 0).is_err());
        let input = "x\n".repeat(MAX_BATCH_ROWS + 1);
        let large = CsvTable::parse(input.as_bytes(), b',').unwrap();
        assert!(large.prepare_batch(false, &[true], 0, MAX_BATCH_ROWS).is_err());
    }
    #[test] fn contact_book_round_trips_and_rejects_external_changes() {
        let dir = std::env::temp_dir().join(format!("dymo-contacts-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&dir).unwrap(); let path = dir.join("contacts.json");
        let mut book = ContactBook::new(path.clone()); book.contacts.push(Contact { name: "Example".into(), address: "Synthetic address".into() }); book.dirty = true; book.save().unwrap();
        let mut opened = ContactBook::open(path.clone()).unwrap(); assert_eq!(opened.contacts, book.contacts);
        fs::write(&path, "external change").unwrap(); opened.dirty = true;
        assert!(opened.save().unwrap_err().contains("changed outside")); assert_eq!(fs::read_to_string(&path).unwrap(), "external change");
        fs::remove_file(path).unwrap(); fs::remove_dir(dir).unwrap();
    }
    #[test] fn contact_import_rejects_unknown_data_instead_of_overwriting_it() {
        let dir = std::env::temp_dir().join(format!("dymo-contacts-extra-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&dir).unwrap(); let path = dir.join("contacts.json");
        let original = br#"{"version":1,"contacts":[{"name":"Synthetic","address":"Example","phone":"extra"}]}"#;
        fs::write(&path, original).unwrap();
        assert!(matches!(ContactBook::open(path.clone()), Err(error) if error.contains("unsupported fields")));
        assert_eq!(fs::read(&path).unwrap(), original);
        fs::remove_file(path).unwrap(); fs::remove_dir(dir).unwrap();
    }
}
