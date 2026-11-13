//! Restart helper, recoverable file replacement and first-window acknowledgement.
use std::{fs, io::Read, os::windows::process::CommandExt, path::{Path, PathBuf}, process::{Command, Stdio}, time::{Duration, Instant}};
use crate::update_package::{self as package, FileRecord, Prepared, APP_EXE};

const MAX_JSON: u64 = 2 * 1024 * 1024;
const SCHEMA: u64 = 1;

#[derive(Clone, Copy, Debug)]
struct ProcessIdentity { pid: u32, created: u64 }
struct Plan { prepared: Prepared, source_version: String, source_hash: String, parent: ProcessIdentity, nonce: String, helper_hash: String, recovery: bool }
#[derive(Clone, Debug)]
struct Previous { file: FileRecord, old: Option<(u64, String)> }
struct Journal { source_version: String, records: Vec<Previous> }

pub struct InstanceGuard { _handle: native::Instance }
pub fn download_guard(install: &Path) -> Result<InstanceGuard, String> { Ok(InstanceGuard { _handle: native::named_guard(install, "download", true)? }) }
pub enum Startup { Run { acknowledge: Option<String>, message: Option<String>, instance: InstanceGuard }, Exit }

pub(crate) fn version(value: &str) -> Result<[u32; 4], String> {
    let parts: Vec<_> = value.split('.').collect();
    if !(3..=4).contains(&parts.len()) { return Err("Unsupported app update version.".into()); }
    let mut result = [0; 4];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || part.len() > 5 || (part.len() > 1 && part.starts_with('0')) || !part.bytes().all(|b| b.is_ascii_digit()) { return Err("Invalid app update version.".into()); }
        result[index] = part.parse::<u16>().map_err(|_| "Unsupported app update version.")? as u32;
    }
    Ok(result)
}

fn read_json(path: &Path) -> Result<serde_json::Value, String> {
    package::plain_file(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(|e| e.to_string())?.take(MAX_JSON + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_JSON { return Err("Update recovery metadata is too large.".into()); }
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid update recovery metadata: {e}"))
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_JSON { return Err("Update recovery metadata is too large.".into()); }
    if path.exists() { package::plain_file(path)?; }
    crate::project_io::write_project(path, &bytes, path.exists())
}

fn string(value: &serde_json::Value, key: &str) -> Result<String, String> { value[key].as_str().filter(|s| s.len() <= 4096).map(str::to_owned).ok_or_else(|| format!("Invalid update metadata field: {key}")) }
fn record_value(file: &FileRecord) -> serde_json::Value { serde_json::json!({"path":file.path,"bytes":file.bytes,"sha256":file.sha256}) }
fn record(value: &serde_json::Value) -> Result<FileRecord, String> {
    let file = FileRecord { path: string(value, "path")?, bytes: value["bytes"].as_u64().ok_or("Invalid update file size.")?, sha256: string(value, "sha256")? };
    package::validate_records(std::slice::from_ref(&file))?;
    Ok(file)
}
fn nonce() -> String { format!("{:032x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() ^ ((std::process::id() as u128) << 64)) }
fn valid_nonce(value: &str) -> bool { value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit()) }

impl Plan {
    fn value(&self) -> serde_json::Value {
        serde_json::json!({"schema":SCHEMA,"version":self.prepared.version,"sourceVersion":self.source_version,"sourceSha256":self.source_hash,
            "parentPid":self.parent.pid,"parentCreated":self.parent.created,"nonce":self.nonce,"helperSha256":self.helper_hash,"recovery":self.recovery,
            "files":self.prepared.files.iter().map(record_value).collect::<Vec<_>>()})
    }
    fn read(root: &Path) -> Result<Self, String> {
        let value = read_json(&root.join("plan.json"))?;
        if value["schema"].as_u64() != Some(SCHEMA) { return Err("Unsupported update protocol.".into()); }
        let target_version = string(&value, "version")?;
        let source_version = string(&value, "sourceVersion")?;
        version(&target_version)?; version(&source_version)?;
        let files = value["files"].as_array().filter(|a| a.len() <= package::MAX_FILES).ok_or("Invalid update file list.")?.iter().map(record).collect::<Result<Vec<_>, _>>()?;
        package::validate_records(&files)?;
        let plan = Self { prepared: Prepared { version: target_version, install: root.parent().ok_or("Invalid update workspace.")?.to_owned(), files },
            source_version, source_hash: string(&value, "sourceSha256")?, parent: ProcessIdentity {
                pid: u32::try_from(value["parentPid"].as_u64().filter(|n| *n != 0).ok_or("Invalid parent process.")?).map_err(|_| "Invalid parent process.")?,
                created: value["parentCreated"].as_u64().filter(|n| *n != 0).ok_or("Invalid parent process creation time.")? },
            nonce: string(&value, "nonce")?, helper_hash: string(&value, "helperSha256")?, recovery: value["recovery"].as_bool().ok_or("Invalid update mode.")? };
        if !valid_nonce(&plan.nonce) || !package::valid_hash(&plan.source_hash) || !package::valid_hash(&plan.helper_hash) { return Err("Invalid update integrity metadata.".into()); }
        Ok(plan)
    }
}

impl Journal {
    fn value(&self) -> serde_json::Value {
        serde_json::json!({"schema":SCHEMA,"sourceVersion":self.source_version,"records":self.records.iter().map(|r| {
            let old = r.old.as_ref().map(|(bytes, hash)| serde_json::json!({"bytes":bytes,"sha256":hash}));
            serde_json::json!({"file":record_value(&r.file),"old":old})
        }).collect::<Vec<_>>()})
    }
    fn read(backup: &Path) -> Result<Self, String> {
        package::plain_directory(backup)?;
        let value = read_json(&backup.join("journal.json"))?;
        if value["schema"].as_u64() != Some(SCHEMA) { return Err("Unsupported update journal.".into()); }
        let source_version = string(&value, "sourceVersion")?; version(&source_version)?;
        let mut records = Vec::new();
        for row in value["records"].as_array().filter(|a| a.len() <= package::MAX_FILES).ok_or("Invalid update journal.")? {
            let file = record(&row["file"])?;
            let old = if row["old"].is_null() { None } else {
                let hash = string(&row["old"], "sha256")?;
                let bytes = row["old"]["bytes"].as_u64().filter(|n| *n <= package::MAX_EXPANDED).ok_or("Invalid rollback file size.")?;
                if !package::valid_hash(&hash) { return Err("Invalid rollback file digest.".into()); }
                Some((bytes, hash))
            };
            records.push(Previous { file, old });
        }
        package::validate_records(&records.iter().map(|r| r.file.clone()).collect::<Vec<_>>())?;
        Ok(Self { source_version, records })
    }
}

fn remove_file(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => { package::plain_file(path)?; fs::remove_file(path).map_err(|e| e.to_string()) }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn write_result(root: &Path, message: &str) { let _ = write_json(&root.join("result.json"), &serde_json::json!({"schema":SCHEMA,"message":message})); }

fn check_lock(root: &Path) -> Result<bool, String> {
    let lock = root.join("install.lock.json");
    if !lock.try_exists().map_err(|e| e.to_string())? { return Ok(false); }
    let value = read_json(&lock)?;
    let identity = ProcessIdentity { pid: u32::try_from(value["pid"].as_u64().filter(|n| *n != 0).ok_or("Invalid updater lock.")?).map_err(|_| "Invalid updater lock.")?, created: value["created"].as_u64().filter(|n| *n != 0).ok_or("Invalid updater lock.")? };
    if native::alive(identity)? { Ok(true) } else { remove_file(&lock)?; Ok(false) }
}

fn clear_empty_intent(root: &Path) -> Result<(), String> {
    let pending = root.join("pending-backup");
    if pending.exists() && !pending.join("journal.json").exists() {
        package::plain_directory(&pending)?;
        if fs::read_dir(&pending).map_err(|e| e.to_string())?.next().is_some() {
            return Err("An interrupted update has incomplete recovery metadata. Its files have been retained for manual recovery.".into());
        }
        // The journal is persisted before any original file is moved. An empty
        // directory here therefore represents an interruption before changes.
        package::remove_owned(root, "pending-backup")?;
    }
    Ok(())
}

pub fn start(prepared: &Prepared) -> Result<(), String> {
    if version(&prepared.version)? <= version(env!("CARGO_PKG_VERSION"))? { return Err("Only a newer app version can be installed.".into()); }
    let install = package::install_directory()?;
    let _download_guard = download_guard(&install)?;
    if prepared.install != install || std::env::current_exe().map_err(|e| e.to_string())?.file_name().and_then(|s| s.to_str()) != Some(APP_EXE) { return Err("The downloaded update belongs to a different app folder.".into()); }
    let root = package::workspace(&install)?;
    if check_lock(&root)? || root.join("pending-backup").exists() { return Err("Another update or recovery is already in progress.".into()); }
    package::verify_files(&root.join("staging/payload"), &prepared.files)?;
    verify_executable(&root.join("staging/payload").join(APP_EXE), &prepared.version)?;
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let plan = Plan { prepared: prepared.clone(), source_version: env!("CARGO_PKG_VERSION").into(), source_hash: package::hash_file(&current)?,
        parent: native::current()?, nonce: nonce(), helper_hash: package::hash_file(&current)?, recovery: false };
    launch_helper(&root, plan)
}

fn launch_helper(root: &Path, plan: Plan) -> Result<(), String> {
    for name in ["helper.ready", "installed.ready"] { remove_file(&root.join(name))?; }
    let helper = root.join("updater.exe");
    if helper.exists() { package::plain_file(&helper)?; }
    fs::copy(std::env::current_exe().map_err(|e| e.to_string())?, &helper).map_err(|e| format!("Cannot prepare the restart helper: {e}"))?;
    if package::hash_file(&helper)? != plan.helper_hash { return Err("Restart helper verification failed.".into()); }
    write_json(&root.join("plan.json"), &plan.value())?;
    let mut child = Command::new(helper).creation_flags(0x08000000).arg("--apply-update").arg(root.join("plan.json"))
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("Cannot start the update helper: {e}"))?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(12) {
        if fs::read_to_string(root.join("helper.ready")).ok().as_deref() == Some(plan.nonce.as_str()) { return Ok(()); }
        if child.try_wait().map_err(|e| e.to_string())?.is_some() { return Err("The restart helper could not prepare the update. The current app has been kept open.".into()); }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill(); let _ = child.wait();
    Err("The restart helper did not respond. The current app has been kept open.".into())
}

pub fn startup() -> Result<Startup, String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().and_then(|s| s.to_str()) == Some("--apply-update") {
        if args.len() != 2 { return Err("Invalid restart helper arguments.".into()); }
        let path = PathBuf::from(&args[1]).canonicalize().map_err(|e| e.to_string())?;
        let root = path.parent().ok_or("Invalid update plan.")?;
        if path.file_name().and_then(|s| s.to_str()) != Some("plan.json") || root.file_name().and_then(|s| s.to_str()) != Some(".420p-updater")
            || package::workspace(root.parent().ok_or("Invalid installation folder.")?)? != root
            || std::env::current_exe().map_err(|e| e.to_string())?.canonicalize().map_err(|e| e.to_string())? != root.join("updater.exe") { return Err("The update helper was launched from an unexpected location.".into()); }
        let result = apply(root);
        if let Err(error) = result { write_result(root, &format!("The update could not finish: {error}. Recovery files were retained locally.")); }
        return Ok(Startup::Exit);
    }
    let install = package::install_directory()?;
    let root_path = install.join(".420p-updater");
    let instance = InstanceGuard { _handle: native::instance(&install, false)? };
    let acknowledge = if args.first().and_then(|s| s.to_str()) == Some("--update-ready") {
        if args.len() != 2 { return Err("Invalid update restart arguments.".into()); }
        let value = args[1].to_str().filter(|s| valid_nonce(s)).ok_or("Invalid update acknowledgement.")?.to_owned();
        Some(value)
    } else { None };
    if !root_path.exists() {
        if acknowledge.is_some() { return Err("The update restart metadata is missing.".into()); }
        return Ok(Startup::Run { acknowledge: None, message: None, instance });
    }
    let root = package::workspace(&install)?;
    if let Some(token) = acknowledge {
        let plan = Plan::read(&root)?;
        if plan.nonce != token || version(&plan.prepared.version)? != version(env!("CARGO_PKG_VERSION"))? || !check_lock(&root)? { return Err("The update restart could not be verified.".into()); }
        package::verify_files(&install, &plan.prepared.files)?;
        return Ok(Startup::Run { acknowledge: Some(token), message: Some(format!("Installed version {}. Finishing the restart check…", env!("CARGO_PKG_VERSION"))), instance });
    }
    if check_lock(&root)? { return Err("An app update is in progress. Wait for it to finish before opening another window.".into()); }
    clear_empty_intent(&root)?;
    if root.join("pending-backup").exists() {
        let previous = Plan::read(&root)?;
        if fs::read_to_string(root.join("installed.ready")).ok().as_deref() == Some(previous.nonce.as_str())
            && version(&previous.prepared.version)? == version(env!("CARGO_PKG_VERSION"))? {
            package::verify_files(&install, &previous.prepared.files)?;
            let message = match finalise(&root) {
                Ok(()) => format!("Updated to version {}. The previous version is kept locally for recovery.", env!("CARGO_PKG_VERSION")),
                Err(_) => "The update is installed. Rotation of the local rollback is still pending; both recovery sets have been retained.".into(),
            };
            return Ok(Startup::Run { acknowledge: None, message: Some(message), instance });
        }
        // A previous helper stopped mid-transaction. Recovery runs outside the
        // installed executable, after this process has released its file locks.
        Journal::read(&root.join("pending-backup"))?;
        let mut plan = Plan::read(&root)?;
        plan.parent = native::current()?; plan.nonce = nonce(); plan.recovery = true;
        plan.helper_hash = package::hash_file(&std::env::current_exe().map_err(|e| e.to_string())?)?;
        launch_helper(&root, plan)?;
        return Ok(Startup::Exit);
    }
    let message = if root.join("result.json").exists() { read_json(&root.join("result.json")).ok().and_then(|value| value["message"].as_str().map(|s| s.chars().take(1500).collect())) } else { None };
    Ok(Startup::Run { acknowledge: None, message, instance })
}

pub fn acknowledge(token: &str) -> Result<(), String> {
    let install = package::install_directory()?;
    let root = package::workspace(&install)?;
    let plan = Plan::read(&root)?;
    if plan.nonce != token || !valid_nonce(token) { return Err("The update restart token changed.".into()); }
    fs::write(root.join("installed.ready"), token).map_err(|e| e.to_string())
}

fn apply(root: &Path) -> Result<(), String> {
    let plan = Plan::read(root)?;
    let own_image = std::env::current_exe().map_err(|e| e.to_string())?;
    if package::hash_file(&own_image)? != plan.helper_hash { return Err("The restart helper changed after verification.".into()); }
    if check_lock(root)? { return Err("Another restart helper is already running.".into()); }
    let parent = native::watch(plan.parent)?;
    let own = native::current()?;
    let lock_path = root.join("install.lock.json");
    let lock = fs::OpenOptions::new().write(true).create_new(true).open(&lock_path).map_err(|e| e.to_string())?;
    drop(lock);
    write_json(&lock_path, &serde_json::json!({"pid":own.pid,"created":own.created}))?;
    let mut parent_closed = false;
    let operation = (|| {
        if !plan.recovery {
            if version(&plan.prepared.version)? <= version(&plan.source_version)? { return Err("An update cannot downgrade or reinstall this version.".into()); }
            if package::hash_file(&plan.prepared.install.join(APP_EXE))? != plan.source_hash { return Err("The installed executable changed before restart.".into()); }
            verify_executable(&plan.prepared.install.join(APP_EXE), &plan.source_version)?;
            package::verify_files(&root.join("staging/payload"), &plan.prepared.files)?;
            verify_executable(&root.join("staging/payload").join(APP_EXE), &plan.prepared.version)?;
        }
        fs::write(root.join("helper.ready"), &plan.nonce).map_err(|e| e.to_string())?;
        parent.wait(Duration::from_secs(120))?;
        parent_closed = true;
        // Every normal app window holds this named object's lifetime. Creating
        // it exclusively after the parent exits detects other open windows in
        // this Windows session, before any application file can be moved.
        let _exclusive = native::instance(&plan.prepared.install, true)?;
        if plan.recovery {
            restore_pending(root, &plan.prepared.install)?;
            write_result(root, "An interrupted update was recovered. The previous version has been restored.");
            return Ok(false);
        }
        let installed = install_files(root, &plan.prepared).and_then(|_| verify_backup(&root.join("pending-backup")));
        if let Err(error) = installed {
            if root.join("pending-backup/journal.json").exists() { restore_pending(root, &plan.prepared.install)?; }
            else if root.join("pending-backup").exists() { package::remove_owned(root, "pending-backup")?; }
            write_result(root, &format!("The update was not installed; the previous version was restored. {error}"));
            return Ok(false);
        }
        let mut child = match Command::new(plan.prepared.install.join(APP_EXE)).arg("--update-ready").arg(&plan.nonce).current_dir(&plan.prepared.install)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(child) => child,
            Err(error) => {
                restore_pending(root, &plan.prepared.install)?;
                write_result(root, &format!("The updated app could not start. The previous version was restored. {error}"));
                return Ok(false);
            }
        };
        let start = Instant::now();
        let mut accepted = false;
        while start.elapsed() < Duration::from_secs(60) {
            match child.try_wait() { Ok(None) => {}, Ok(Some(_)) | Err(_) => break }
            if fs::read_to_string(root.join("installed.ready")).ok().as_deref() == Some(plan.nonce.as_str()) { accepted = true; break; }
            std::thread::sleep(Duration::from_millis(100));
        }
        if !accepted {
            let _ = child.kill(); let _ = child.wait();
            restore_pending(root, &plan.prepared.install)?;
            write_result(root, "The updated app did not complete its restart check. The previous version was restored.");
            return Ok(false);
        }
        // The new app checked every installed file before showing its window.
        // Once acknowledged, cleanup failure must never terminate that window
        // or revert work the user may already have started in it.
        let message = match finalise(root) {
            Ok(()) => format!("Updated to version {}. The previous version is kept locally for recovery.", plan.prepared.version),
            Err(_) => "The update is installed. Rotation of the local rollback is still pending; both recovery sets have been retained.".into(),
        };
        write_result(root, &message);
        let _ = package::remove_owned(root, "staging");
        Ok(true)
    })();
    let _ = remove_file(&lock_path);
    match operation {
        Ok(true) => Ok(()),
        Ok(false) => {
            Command::new(plan.prepared.install.join(APP_EXE)).current_dir(&plan.prepared.install).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
                .map_err(|e| format!("The previous version was restored but could not be reopened: {e}"))?;
            Ok(())
        }
        Err(error) => {
            if parent_closed && !root.join("pending-backup").exists() && package::hash_file(&plan.prepared.install.join(APP_EXE)).ok().as_deref() == Some(plan.source_hash.as_str()) {
                write_result(root, &format!("The update was not installed. {error}"));
                let _ = Command::new(plan.prepared.install.join(APP_EXE)).current_dir(&plan.prepared.install).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
            }
            Err(error)
        }
    }
}

fn install_files(root: &Path, prepared: &Prepared) -> Result<(), String> {
    let payload = root.join("staging/payload");
    package::verify_files(&payload, &prepared.files)?;
    if root.join("pending-backup").exists() { return Err("An earlier update still needs recovery.".into()); }
    let mut journal = Journal { source_version: {
        let plan = Plan::read(root)?; plan.source_version
    }, records: Vec::new() };
    for file in &prepared.files {
        let target = package::checked_target(&prepared.install, &file.path, true)?;
        let old = if target.exists() { Some((fs::metadata(&target).map_err(|e| e.to_string())?.len(), package::hash_file(&target)?)) } else { None };
        journal.records.push(Previous { file: file.clone(), old });
    }
    // The entire intent is durable before the first original file is moved.
    fs::create_dir(root.join("pending-backup")).map_err(|e| e.to_string())?;
    write_json(&root.join("pending-backup/journal.json"), &journal.value())?;
    for record in &journal.records {
        let target = package::checked_target(&prepared.install, &record.file.path, true)?;
        let source = package::checked_target(&payload, &record.file.path, false)?;
        if let Some((bytes, hash)) = &record.old {
            if fs::metadata(&target).map_err(|e| e.to_string())?.len() != *bytes || package::hash_file(&target)? != *hash { return Err("An installed file changed during the update.".into()); }
            let backup = package::checked_target(&root.join("pending-backup"), &record.file.path, true)?;
            if backup.exists() { return Err("An update backup file already exists.".into()); }
            fs::rename(&target, backup).map_err(|e| format!("An app file is still in use or cannot be replaced: {e}"))?;
        } else if target.exists() { return Err("A file appeared in the update destination.".into()); }
        fs::rename(source, target).map_err(|e| format!("Cannot install an app file: {e}"))?;
    }
    package::verify_files(&prepared.install, &prepared.files)
}

fn verify_backup(backup: &Path) -> Result<(), String> {
    let journal = Journal::read(backup)?;
    for record in journal.records {
        if let Some((bytes, hash)) = record.old {
            let file = package::checked_target(backup, &record.file.path, false)?;
            if fs::metadata(&file).map_err(|e| e.to_string())?.len() != bytes || package::hash_file(&file)? != hash { return Err("The local rollback could not be verified.".into()); }
        }
    }
    Ok(())
}

fn finalise(root: &Path) -> Result<(), String> {
    verify_backup(&root.join("pending-backup"))?;
    package::remove_owned(root, "rollback")?;
    fs::rename(root.join("pending-backup"), root.join("rollback")).map_err(|e| e.to_string())
}

fn restore_pending(root: &Path, install: &Path) -> Result<(), String> {
    let backup = root.join("pending-backup");
    let journal = Journal::read(&backup)?;
    let mut errors = Vec::new();
    for record in journal.records.iter().rev() {
        let restore = (|| -> Result<(), String> {
            let target = package::checked_target(install, &record.file.path, true)?;
            let saved = package::checked_target(&backup, &record.file.path, true)?;
            if saved.exists() {
                package::plain_file(&saved)?;
                let (bytes, hash) = record.old.as_ref().ok_or("Unexpected recovery file.")?;
                if fs::metadata(&saved).map_err(|e| e.to_string())?.len() != *bytes || package::hash_file(&saved)? != *hash { return Err("An original recovery file was changed.".into()); }
                if target.exists() {
                    if package::hash_file(&target)? != record.file.sha256 { return Err("A destination changed after installation; it was preserved for manual recovery.".into()); }
                    fs::remove_file(&target).map_err(|e| e.to_string())?;
                }
                fs::rename(saved, target).map_err(|e| e.to_string())?;
            } else if let Some((bytes, hash)) = &record.old {
                if !target.exists() || fs::metadata(&target).map_err(|e| e.to_string())?.len() != *bytes || package::hash_file(&target)? != *hash { return Err("An original app file needs manual recovery.".into()); }
            } else if target.exists() {
                if package::hash_file(&target)? != record.file.sha256 { return Err("A newly installed file was changed; it was preserved for manual recovery.".into()); }
                fs::remove_file(target).map_err(|e| e.to_string())?;
            }
            Ok(())
        })();
        if let Err(error) = restore { errors.push(error); }
    }
    if !errors.is_empty() { return Err(format!("Rollback needs attention. Recovery files remain in {}. {}", backup.display(), errors[0])); }
    package::remove_owned(root, "pending-backup")?;
    let _ = package::remove_owned(root, "staging");
    Ok(())
}

pub fn verify_executable(path: &Path, expected: &str) -> Result<(), String> {
    package::plain_file(path)?;
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut header = [0u8; 64];
    file.read_exact(&mut header).map_err(|_| "The update executable is incomplete.")?;
    if &header[..2] != b"MZ" { return Err("The update is not a Windows executable.".into()); }
    use std::io::{Seek, SeekFrom};
    let offset = u32::from_le_bytes(header[60..64].try_into().unwrap());
    file.seek(SeekFrom::Start(offset as u64)).map_err(|e| e.to_string())?;
    let mut pe = [0u8; 6]; file.read_exact(&mut pe).map_err(|_| "The update has an invalid PE header.")?;
    if &pe[..4] != b"PE\0\0" || u16::from_le_bytes([pe[4], pe[5]]) != 0x8664 { return Err("The update must be a Windows x64 application.".into()); }
    if native::file_version(path)? != version(expected)? { return Err("The update executable's version differs from the GitHub release.".into()); }
    Ok(())
}

pub fn show_error(message: &str) { native::message(message); }

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;
    fn fixture() -> (PathBuf, PathBuf, Prepared) {
        let install = std::env::temp_dir().join(format!("420p-update-install-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&install).unwrap();
        let install = install.canonicalize().unwrap();
        let root = package::workspace(&install).unwrap();
        fs::create_dir_all(root.join("staging/payload/sdk")).unwrap(); fs::create_dir(install.join("sdk")).unwrap();
        fs::write(install.join(APP_EXE), b"previous application").unwrap(); fs::write(install.join("sdk/component.dll"), b"previous component").unwrap();
        fs::write(install.join("label.420p"), b"synthetic user work").unwrap();
        fs::write(root.join("staging/payload").join(APP_EXE), b"next application").unwrap(); fs::write(root.join("staging/payload/sdk/component.dll"), b"next component").unwrap();
        fs::write(root.join("staging/payload/sdk/additional.dll"), b"new component").unwrap();
        let files = [APP_EXE, "sdk/component.dll", "sdk/additional.dll"].into_iter().map(|name| {
            let path = root.join("staging/payload").join(name); FileRecord { path: name.into(), bytes: fs::metadata(&path).unwrap().len(), sha256: package::hash_file(&path).unwrap() }
        }).collect();
        let prepared = Prepared { version: "9.0.0".into(), install: install.clone(), files };
        let plan = Plan { prepared: prepared.clone(), source_version: "1.0.0".into(), source_hash: package::hash_file(&install.join(APP_EXE)).unwrap(), parent: native::current().unwrap(), nonce: "a".repeat(32), helper_hash: "b".repeat(64), recovery: false };
        write_json(&root.join("plan.json"), &plan.value()).unwrap();
        (install, root, prepared)
    }
    fn cleanup(install: PathBuf) {
        assert_eq!(install.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
        assert!(install.file_name().unwrap().to_str().unwrap().starts_with("420p-update-install-")); fs::remove_dir_all(install).unwrap();
    }
    fn original(install: &Path) {
        assert_eq!(fs::read(install.join(APP_EXE)).unwrap(), b"previous application");
        assert_eq!(fs::read(install.join("sdk/component.dll")).unwrap(), b"previous component");
        assert_eq!(fs::read(install.join("label.420p")).unwrap(), b"synthetic user work");
    }
    #[test] fn managed_install_can_be_restored_without_touching_user_projects() {
        let (install, root, prepared) = fixture();
        install_files(&root, &prepared).unwrap(); package::verify_files(&install, &prepared.files).unwrap(); verify_backup(&root.join("pending-backup")).unwrap();
        assert_eq!(fs::read(install.join("label.420p")).unwrap(), b"synthetic user work");
        restore_pending(&root, &install).unwrap(); original(&install);
        assert!(!install.join("sdk/additional.dll").exists()); assert!(!root.join("pending-backup").exists()); cleanup(install);
    }
    #[test] fn real_windows_file_lock_after_partial_install_restores_previous_files() {
        let (install, root, prepared) = fixture();
        fs::create_dir(root.join("rollback")).unwrap(); fs::write(root.join("rollback/retained.txt"), b"known good").unwrap();
        let lock = fs::OpenOptions::new().read(true).share_mode(1).open(install.join("sdk/component.dll")).unwrap();
        assert!(install_files(&root, &prepared).is_err());
        // The executable had already moved, but the locked component had not.
        assert_eq!(fs::read(install.join(APP_EXE)).unwrap(), b"next application");
        restore_pending(&root, &install).unwrap(); original(&install);
        assert_eq!(fs::read(root.join("rollback/retained.txt")).unwrap(), b"known good"); drop(lock); cleanup(install);
    }
    #[test] fn missing_or_changed_staged_files_never_modify_the_current_installation() {
        let (install, root, prepared) = fixture();
        fs::write(root.join("staging/payload/sdk/component.dll"), b"tampered").unwrap();
        assert!(install_files(&root, &prepared).is_err()); original(&install); assert!(!root.join("pending-backup").exists()); cleanup(install);
    }
    #[test] fn journal_recovers_a_crash_between_original_move_and_replacement_move() {
        let (install, root, prepared) = fixture();
        let records = prepared.files.iter().map(|file| {
            let path = install.join(&file.path);
            let old = path.exists().then(|| (fs::metadata(&path).unwrap().len(), package::hash_file(&path).unwrap()));
            Previous { file: file.clone(), old }
        }).collect();
        fs::create_dir(root.join("pending-backup")).unwrap();
        write_json(&root.join("pending-backup/journal.json"), &Journal { source_version: "1.0.0".into(), records }.value()).unwrap();
        fs::rename(install.join(APP_EXE), root.join("pending-backup").join(APP_EXE)).unwrap();
        restore_pending(&root, &install).unwrap(); original(&install); cleanup(install);
    }
    #[test] fn external_changes_are_preserved_when_rollback_cannot_safely_replace_them() {
        let (install, root, prepared) = fixture(); install_files(&root, &prepared).unwrap();
        fs::write(install.join(APP_EXE), b"externally modified application").unwrap();
        assert!(restore_pending(&root, &install).unwrap_err().contains("Recovery files remain"));
        assert_eq!(fs::read(install.join(APP_EXE)).unwrap(), b"externally modified application");
        assert_eq!(fs::read(root.join("pending-backup").join(APP_EXE)).unwrap(), b"previous application");
        assert_eq!(fs::read(install.join("label.420p")).unwrap(), b"synthetic user work"); cleanup(install);
    }
    #[test] fn rollback_rotation_keeps_the_new_recovery_point_and_removes_only_the_old_set() {
        let (install, root, prepared) = fixture();
        fs::create_dir(root.join("rollback")).unwrap(); fs::write(root.join("rollback/previous-set.txt"), b"old recovery set").unwrap();
        install_files(&root, &prepared).unwrap(); finalise(&root).unwrap();
        assert!(!root.join("pending-backup").exists()); assert!(!root.join("rollback/previous-set.txt").exists());
        assert_eq!(fs::read(root.join("rollback").join(APP_EXE)).unwrap(), b"previous application");
        package::verify_files(&install, &prepared.files).unwrap(); assert_eq!(fs::read(install.join("label.420p")).unwrap(), b"synthetic user work"); cleanup(install);
    }
    #[test] fn damaged_recovery_files_prevent_rotation_of_the_known_good_set() {
        let (install, root, prepared) = fixture(); fs::create_dir(root.join("rollback")).unwrap(); fs::write(root.join("rollback/keep.txt"), b"keep").unwrap();
        install_files(&root, &prepared).unwrap(); fs::write(root.join("pending-backup").join(APP_EXE), b"changed backup").unwrap();
        assert!(finalise(&root).is_err()); assert_eq!(fs::read(root.join("rollback/keep.txt")).unwrap(), b"keep"); cleanup(install);
    }
    #[test] fn process_identity_rejects_pid_reuse_and_waits_for_the_exact_process() {
        let current = native::current().unwrap(); assert!(native::alive(current).unwrap());
        assert!(!native::alive(ProcessIdentity { created: current.created + 1, ..current }).unwrap());
        assert!(native::watch(ProcessIdentity { created: current.created + 1, ..current }).is_err());
        assert!(native::watch(current).unwrap().wait(Duration::ZERO).is_err());
    }
    #[test] fn other_app_windows_block_installation_until_their_guard_is_released() {
        let (install, _, _) = fixture();
        let first = native::instance(&install, false).unwrap(); let second = native::instance(&install, false).unwrap();
        assert!(native::instance(&install, true).is_err()); drop(first);
        assert!(native::instance(&install, true).is_err()); drop(second);
        let exclusive = native::instance(&install, true).unwrap(); drop(exclusive); cleanup(install);
    }
    #[test] fn concurrent_downloads_are_serialised_separately_from_editor_windows() {
        let (install, _, _) = fixture(); let editor = native::instance(&install, false).unwrap();
        let download = download_guard(&install).unwrap(); assert!(download_guard(&install).is_err());
        drop(download); let retry = download_guard(&install).unwrap(); drop(retry); drop(editor); cleanup(install);
    }
    #[test] fn live_updater_lock_blocks_a_second_installer_and_stale_identity_is_cleared() {
        let (install, root, _) = fixture(); let current = native::current().unwrap();
        write_json(&root.join("install.lock.json"), &serde_json::json!({"pid":current.pid,"created":current.created})).unwrap(); assert!(check_lock(&root).unwrap());
        write_json(&root.join("install.lock.json"), &serde_json::json!({"pid":current.pid,"created":current.created+1})).unwrap(); assert!(!check_lock(&root).unwrap());
        assert!(!root.join("install.lock.json").exists()); cleanup(install);
    }
    #[test] fn invalid_version_protocol_and_pe_metadata_are_rejected() {
        for value in ["1.2", "1.2.3-beta", "1.02.3", "65536.0.0", "../1.2.3", "1.2.3.4.5"] { assert!(version(value).is_err(), "{value}"); }
        assert_eq!(version("1.2.3").unwrap(), version("1.2.3.0").unwrap());
        let (install, root, _) = fixture(); assert!(verify_executable(&install.join(APP_EXE), "1.2.3").is_err());
        let mut value = read_json(&root.join("plan.json")).unwrap(); value["files"][0]["path"] = serde_json::json!("../outside.exe"); write_json(&root.join("plan.json"), &value).unwrap();
        assert!(Plan::read(&root).is_err()); cleanup(install);
    }
    #[test] fn an_interruption_before_the_journal_does_not_block_the_unchanged_app() {
        let (install, root, _) = fixture(); fs::create_dir(root.join("pending-backup")).unwrap();
        clear_empty_intent(&root).unwrap(); original(&install); assert!(!root.join("pending-backup").exists()); cleanup(install);
    }
    #[test] fn missing_recovery_metadata_never_causes_unknown_backups_to_be_deleted() {
        let (install, root, _) = fixture(); fs::create_dir(root.join("pending-backup")).unwrap(); fs::write(root.join("pending-backup/original.dll"), b"keep").unwrap();
        assert!(clear_empty_intent(&root).is_err()); assert_eq!(fs::read(root.join("pending-backup/original.dll")).unwrap(), b"keep"); original(&install); cleanup(install);
    }
}

mod native {
    use super::*;
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    type Handle = *mut c_void;
    #[repr(C)] #[derive(Default)] struct FileTime { low: u32, high: u32 }
    #[link(name="kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn GetProcessTimes(process: Handle, created: *mut FileTime, exit: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
        fn WaitForSingleObject(handle: Handle, timeout: u32) -> u32;
        fn CloseHandle(handle: Handle) -> i32;
        fn CreateMutexW(attributes: *const c_void, owner: i32, name: *const u16) -> Handle;
        fn GetLastError() -> u32;
    }
    fn created(handle: Handle) -> Result<u64, String> {
        let (mut a, mut b, mut c, mut d) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
        // SAFETY: The process handle is live and all output buffers are valid.
        if unsafe { GetProcessTimes(handle, &mut a, &mut b, &mut c, &mut d) } == 0 { return Err(std::io::Error::last_os_error().to_string()); }
        Ok(((a.high as u64) << 32) | a.low as u64)
    }
    pub(super) fn current() -> Result<ProcessIdentity, String> {
        // SAFETY: GetCurrentProcess returns a process-owned pseudo-handle.
        Ok(ProcessIdentity { pid: std::process::id(), created: created(unsafe { GetCurrentProcess() })? })
    }
    pub(super) struct Watch(Handle);
    impl Drop for Watch { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
    pub(super) struct Instance(Handle);
    impl Drop for Instance { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
    pub(super) fn instance(install: &Path, exclusive: bool) -> Result<Instance, String> {
        named_guard(install, "windows", exclusive)
    }
    pub(super) fn named_guard(install: &Path, kind: &str, exclusive: bool) -> Result<Instance, String> {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(install.to_string_lossy().to_lowercase().as_bytes());
        let suffix: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        let name = wide(std::ffi::OsStr::new(&format!("Local\\420PLabelStudio-{kind}-{suffix}")));
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        let error = unsafe { GetLastError() };
        if handle.is_null() { return Err(format!("Cannot reserve the app's update guard: {}", std::io::Error::from_raw_os_error(error as i32))); }
        let guard = Instance(handle);
        if exclusive && error == 183 {
            return Err(if kind == "download" { "Another window is already preparing an update." } else { "Close every other 420P Label Studio window, including tray windows, before installing an update." }.into());
        }
        Ok(guard)
    }
    pub(super) fn watch(identity: ProcessIdentity) -> Result<Watch, String> {
        // SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION. Creation time closes
        // the PID-reuse race; this exact kernel handle is retained while waiting.
        let handle = unsafe { OpenProcess(0x00100000 | 0x1000, 0, identity.pid) };
        if handle.is_null() { return Err(std::io::Error::last_os_error().to_string()); }
        let watch = Watch(handle);
        if created(handle)? != identity.created { return Err("The parent process identity changed.".into()); }
        Ok(watch)
    }
    pub(super) fn alive(identity: ProcessIdentity) -> Result<bool, String> {
        let handle = unsafe { OpenProcess(0x00100000 | 0x1000, 0, identity.pid) };
        if handle.is_null() {
            let error = std::io::Error::last_os_error();
            return if error.raw_os_error() == Some(87) { Ok(false) } else { Err(format!("Cannot inspect the updater process: {error}")) };
        }
        let watch = Watch(handle);
        if created(watch.0)? != identity.created { return Ok(false); }
        match unsafe { WaitForSingleObject(watch.0, 0) } { 0 => Ok(false), 258 => Ok(true), _ => Err("Cannot inspect the updater process.".into()) }
    }
    impl Watch {
        pub(super) fn wait(&self, timeout: Duration) -> Result<(), String> {
            match unsafe { WaitForSingleObject(self.0, timeout.as_millis().min(u32::MAX as u128) as u32) } {
                0 => Ok(()), 258 => Err("The app did not close in time; no update was installed.".into()), _ => Err("Cannot wait for the app to close.".into()),
            }
        }
    }
    fn wide(path: &std::ffi::OsStr) -> Vec<u16> { path.encode_wide().chain(Some(0)).collect() }
    #[link(name="version")]
    unsafe extern "system" {
        fn GetFileVersionInfoSizeW(filename: *const u16, handle: *mut u32) -> u32;
        fn GetFileVersionInfoW(filename: *const u16, handle: u32, length: u32, data: *mut c_void) -> i32;
        fn VerQueryValueW(block: *const c_void, sub: *const u16, value: *mut *mut c_void, length: *mut u32) -> i32;
    }
    pub(super) fn file_version(path: &Path) -> Result<[u32; 4], String> {
        let path = wide(path.as_os_str()); let mut ignored = 0;
        let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut ignored) };
        if size == 0 || size > 1024 * 1024 { return Err("The update executable has no valid version information.".into()); }
        let mut data = vec![0u8; size as usize];
        if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 { return Err("Cannot read the update executable's version.".into()); }
        let query = [b'\\' as u16, 0]; let mut pointer = std::ptr::null_mut(); let mut length = 0;
        if unsafe { VerQueryValueW(data.as_ptr().cast(), query.as_ptr(), &mut pointer, &mut length) } == 0 || pointer.is_null() || length < 52 { return Err("Invalid Windows version metadata.".into()); }
        // VS_FIXEDFILEINFO's signature, structure version, fileVersionMS/LS.
        let fields: [u32; 4] = std::array::from_fn(|index| unsafe { std::ptr::read_unaligned(pointer.cast::<u32>().add(index)) });
        if fields[0] != 0xFEEF04BD { return Err("Invalid Windows version signature.".into()); }
        Ok([fields[2] >> 16, fields[2] & 0xffff, fields[3] >> 16, fields[3] & 0xffff])
    }
    pub(super) fn message(message: &str) {
        #[link(name="user32")] unsafe extern "system" { fn MessageBoxW(window: Handle, text: *const u16, caption: *const u16, kind: u32) -> i32; }
        let text = wide(std::ffi::OsStr::new(message)); let title = wide(std::ffi::OsStr::new(crate::branding::APP_NAME));
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10); }
    }
}
