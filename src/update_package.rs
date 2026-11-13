//! Bounded, cancellable downloads and checked extraction of official releases.
use std::{collections::HashSet, fs, io::{Read, Write}, os::windows::{fs::MetadataExt, process::CommandExt}, path::{Path, PathBuf}, process::{Command, Stdio}, sync::atomic::{AtomicBool, Ordering}};
use sha2::{Digest, Sha256};

pub const APP_EXE: &str = "label-studio-420p.exe";
pub const MAX_ARCHIVE: u64 = 100 * 1024 * 1024;
pub const MAX_EXPANDED: u64 = 512 * 1024 * 1024;
pub const MAX_FILES: usize = 4096;
const OWNER: &[u8] = b"420p-label-studio updater v1\n";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset { pub name: String, pub url: String, pub bytes: u64, pub sha256: String }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package { pub version: String, pub archive: Asset, pub checksums: Asset }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRecord { pub path: String, pub bytes: u64, pub sha256: String }
#[derive(Clone, Debug)]
pub struct Prepared { pub version: String, pub install: PathBuf, pub files: Vec<FileRecord> }
pub enum DownloadEvent { Progress(String, u64, u64), Finished(Result<Prepared, String>) }

impl Prepared {
    fn save(&self, stage: &Path) -> Result<(), String> {
        let value = serde_json::json!({"schema":1,"version":self.version,"files":self.files.iter().map(|file| serde_json::json!({"path":file.path,"bytes":file.bytes,"sha256":file.sha256})).collect::<Vec<_>>()});
        crate::project_io::write_project(&stage.join("prepared.json"), &serde_json::to_vec(&value).map_err(|e| e.to_string())?, false)
    }
}

pub fn resume(install: &Path) -> Result<Option<Prepared>, String> {
    let root_path = install.join(".420p-updater");
    if !root_path.exists() { return Ok(None); }
    let root = workspace(install)?;
    let path = root.join("staging/prepared.json");
    if !path.exists() { return Ok(None); }
    plain_directory(&root.join("staging"))?; plain_file(&path)?;
    let mut bytes = Vec::new(); fs::File::open(&path).map_err(|e| e.to_string())?.take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 { return Err("The downloaded update metadata is too large.".into()); }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value["schema"].as_u64() != Some(1) { return Err("Unsupported downloaded update metadata.".into()); }
    let version = value["version"].as_str().ok_or("Missing downloaded update version.")?.to_owned();
    if crate::updater::version(&version)? <= crate::updater::version(env!("CARGO_PKG_VERSION"))? { return Ok(None); }
    let files = value["files"].as_array().filter(|v| v.len() <= MAX_FILES).ok_or("Invalid downloaded update file list.")?.iter().map(|v| Ok(FileRecord {
        path: v["path"].as_str().ok_or("Invalid downloaded update path.")?.into(), bytes: v["bytes"].as_u64().ok_or("Invalid downloaded update size.")?, sha256: v["sha256"].as_str().ok_or("Invalid downloaded update digest.")?.into()
    })).collect::<Result<Vec<_>, String>>()?;
    verify_files(&root.join("staging/payload"), &files)?;
    crate::updater::verify_executable(&root.join("staging/payload").join(APP_EXE), &version)?;
    Ok(Some(Prepared { version, install: install.canonicalize().map_err(|e| e.to_string())?, files }))
}

pub fn package(repository: &str, version: &str, tag: &str, value: &serde_json::Value) -> Option<Package> {
    if !repository.eq_ignore_ascii_case(crate::app_updates::DEFAULT_REPOSITORY) { return None; }
    let assets = value["assets"].as_array()?;
    let archive_name = format!("420p-label-studio-v{version}-windows-x64.zip");
    let asset = |name: &str, limit: u64| -> Option<Asset> {
        let matches: Vec<_> = assets.iter().filter(|a| a["name"].as_str() == Some(name)).collect();
        if matches.len() != 1 { return None; }
        let a = matches[0];
        if a["state"].as_str()? != "uploaded" { return None; }
        let bytes = a["size"].as_u64()?;
        if bytes == 0 || bytes > limit { return None; }
        let sha256 = a["digest"].as_str()?.strip_prefix("sha256:")?.to_ascii_lowercase();
        if !valid_hash(&sha256) { return None; }
        let url = format!("https://github.com/{}/releases/download/{tag}/{name}", crate::app_updates::DEFAULT_REPOSITORY);
        if a["browser_download_url"].as_str()? != url { return None; }
        Some(Asset { name: name.into(), url, bytes, sha256 })
    };
    Some(Package { version: version.into(), archive: asset(&archive_name, MAX_ARCHIVE)?, checksums: asset("SHA256SUMS.txt", 16 * 1024)? })
}

pub fn valid_hash(value: &str) -> bool { value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()) }

pub fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("Cannot read update file: {e}"))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| format!("Cannot verify update file: {e}"))?;
        if count == 0 { break; }
        hash.update(&buffer[..count]);
    }
    use std::fmt::Write as _;
    let mut result = String::with_capacity(64);
    for byte in hash.finalize() { write!(&mut result, "{byte:02x}").expect("writing into a String cannot fail"); }
    Ok(result)
}

pub fn install_directory() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("Cannot find the application folder.")?
        .canonicalize().map_err(|e| e.to_string())
}

pub fn workspace(install: &Path) -> Result<PathBuf, String> {
    let canonical = install.canonicalize().map_err(|e| e.to_string())?;
    let root = canonical.join(".420p-updater");
    match fs::create_dir(&root) {
        Ok(()) => { fs::write(root.join("owner.txt"), OWNER).map_err(|e| e.to_string())?; }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            plain_directory(&root)?;
            let marker = root.join("owner.txt");
            plain_file(&marker)?;
            if fs::read(marker).map_err(|e| e.to_string())? != OWNER { return Err("The reserved update folder is already in use by something else.".into()); }
        }
        Err(error) => return Err(format!("The application folder is not writable. Install the app in a folder you can change: {error}")),
    }
    Ok(root)
}

pub fn plain_directory(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.file_attributes() & 0x400 != 0 { return Err("Update paths must be ordinary directories, without links or junctions.".into()); }
    Ok(())
}

pub fn plain_file(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_attributes() & 0x400 != 0 { return Err("Update files must be ordinary files, without links.".into()); }
    Ok(())
}

/// Only fixed, app-owned subdirectories are removable. Resolve the exact target
/// before recursion, and reject reparse points throughout the tree.
pub fn remove_owned(root: &Path, name: &str) -> Result<(), String> {
    if !matches!(name, "staging" | "rollback" | "pending-backup") { return Err("Invalid update cleanup target.".into()); }
    if workspace(root.parent().ok_or("Invalid update folder.")?)? != root { return Err("Invalid update workspace.".into()); }
    let target = root.join(name);
    if !target.try_exists().map_err(|e| e.to_string())? { return Ok(()); }
    plain_directory(&target)?;
    if target.canonicalize().map_err(|e| e.to_string())?.parent() != Some(root.canonicalize().map_err(|e| e.to_string())?.as_path()) {
        return Err("Update cleanup target escaped its workspace.".into());
    }
    fn remove_tree(path: &Path) -> Result<(), String> {
        plain_directory(path)?;
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let meta = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            if meta.file_attributes() & 0x400 != 0 { return Err("A link was found in update recovery files; nothing outside the update folder will be removed.".into()); }
            if meta.is_dir() { remove_tree(&entry.path())?; }
            else if meta.is_file() { fs::remove_file(entry.path()).map_err(|e| e.to_string())?; }
            else { return Err("An unsupported update recovery file was found.".into()); }
        }
        fs::remove_dir(path).map_err(|e| e.to_string())
    }
    remove_tree(&target)
}

pub fn relative_path(name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.len() > 220 || !name.is_ascii() || name.contains('\\') || name.starts_with('/') {
        return Err("The update contains an invalid file path.".into());
    }
    for part in name.split('/') {
        if part.is_empty() || matches!(part, "." | "..") || part.ends_with(['.', ' '])
            || part.bytes().any(|b| b < 32 || b"<>:\"|?*".contains(&b)) {
            return Err("The update contains an unsafe Windows file path.".into());
        }
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
            || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && matches!(stem.as_bytes()[3], b'1'..=b'9')) {
            return Err("The update contains a reserved Windows file name.".into());
        }
    }
    let root = name.split('/').next().unwrap_or("");
    let permitted = if name.contains('/') {
        (root == "sdk" && matches!(Path::new(name).extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase().as_str(), "exe" | "dll" | "json" | "xml" | "dymo"))
            || (root == "licences" && !matches!(Path::new(name).extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase().as_str(), "exe" | "dll" | "ps1" | "cmd" | "bat" | "vbs" | "lnk" | "msi"))
    } else { matches!(name, APP_EXE | "README.md" | "COPYRIGHT.txt" | "THIRD-PARTY-NOTICES.txt") };
    if !permitted { return Err("The update contains files outside the application's managed folders.".into()); }
    Ok(name.split('/').collect())
}

pub fn checked_target(base: &Path, name: &str, create_parents: bool) -> Result<PathBuf, String> {
    let relative = relative_path(name)?;
    plain_directory(base)?;
    let mut current = base.to_owned();
    let parts: Vec<_> = relative.components().collect();
    for part in &parts[..parts.len() - 1] {
        current.push(part.as_os_str());
        if !current.try_exists().map_err(|e| e.to_string())? && create_parents { fs::create_dir(&current).map_err(|e| e.to_string())?; }
        plain_directory(&current)?;
    }
    let result = base.join(relative);
    match fs::symlink_metadata(&result) {
        Ok(_) => plain_file(&result)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    Ok(result)
}

pub fn verify_files(base: &Path, files: &[FileRecord]) -> Result<(), String> {
    validate_records(files)?;
    for record in files {
        let path = checked_target(base, &record.path, false)?;
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() != record.bytes || hash_file(&path)? != record.sha256 {
            return Err("An update file failed verification; the installed app was not accepted.".into());
        }
    }
    Ok(())
}

pub fn validate_records(files: &[FileRecord]) -> Result<(), String> {
    if files.is_empty() || files.len() > MAX_FILES { return Err("The update has an invalid number of files.".into()); }
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for record in files {
        relative_path(&record.path)?;
        if !valid_hash(&record.sha256) || !seen.insert(record.path.to_ascii_lowercase()) { return Err("The update contains invalid hashes or duplicate file names.".into()); }
        total = total.checked_add(record.bytes).ok_or("Update size overflow.")?;
    }
    if total > MAX_EXPANDED { return Err("The expanded update exceeds the size limit.".into()); }
    Ok(())
}

fn cancelled(cancel: &AtomicBool) -> Result<(), String> { if cancel.load(Ordering::Acquire) { Err("Update download cancelled.".into()) } else { Ok(()) } }

fn download(asset: &Asset, path: &Path, cancel: &AtomicBool, progress: &impl Fn(String, u64, u64)) -> Result<(), String> {
    cancelled(cancel)?;
    let curl = PathBuf::from(std::env::var_os("SystemRoot").ok_or("Windows directory is unavailable.")?).join("System32/curl.exe");
    let mut child = Command::new(curl).creation_flags(0x08000000)
        .args(["--disable", "--silent", "--show-error", "--fail", "--location", "--max-redirs", "5", "--connect-timeout", "5", "--max-time", "180", "--proto", "=https", "--proto-redir", "=https", "--max-filesize", &asset.bytes.to_string(), "--user-agent", "420P-Label-Studio-Updater", "--output"])
        .arg(path).args(["--url", &asset.url]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("Cannot start the update download: {e}"))?;
    let start = std::time::Instant::now();
    loop {
        let received = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if cancel.load(Ordering::Acquire) || start.elapsed() > std::time::Duration::from_secs(190) || received > asset.bytes {
            let _ = child.kill(); let _ = child.wait();
            cancelled(cancel)?;
            return Err("The update download exceeded its time or size limit.".into());
        }
        progress(format!("Downloading {}", asset.name), received, asset.bytes);
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Err("The update download failed. Check the connection and try again.".into()),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(150)),
            Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error.to_string()); }
        }
    }
    cancelled(cancel)?;
    if fs::metadata(path).map_err(|e| e.to_string())?.len() != asset.bytes || hash_file(path)? != asset.sha256 {
        return Err("The downloaded update did not match GitHub's SHA-256 digest. Nothing was installed.".into());
    }
    Ok(())
}

fn verify_checksum(text: &str, archive: &Asset) -> Result<(), String> {
    let mut found = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty() && !line.starts_with('#')) {
        let mut parts = line.split_whitespace();
        let hash = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("").trim_start_matches('*');
        if name == archive.name { found.push((hash, parts.next().is_none())); }
    }
    if found.len() != 1 || !found[0].1 || !found[0].0.eq_ignore_ascii_case(&archive.sha256) {
        return Err("The release checksum file does not match its Windows package. Nothing was installed.".into());
    }
    Ok(())
}

fn extract(archive: &Path, payload: &Path, cancel: &AtomicBool) -> Result<Vec<FileRecord>, String> {
    cancelled(cancel)?;
    let file = fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("Invalid update ZIP: {e}"))?;
    if zip.len() == 0 || zip.len() > MAX_FILES * 2 { return Err("The update ZIP contains too many entries.".into()); }
    fs::create_dir(payload).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for index in 0..zip.len() {
        cancelled(cancel)?;
        let mut entry = zip.by_index(index).map_err(|e| format!("Cannot read update ZIP: {e}"))?;
        let name = std::str::from_utf8(entry.name_raw()).map_err(|_| "The update contains invalid file names.")?.to_owned();
        if entry.is_dir() {
            let dir = name.strip_suffix('/').ok_or("Invalid update directory.")?;
            // Directory entries are unnecessary; validate their components using
            // an allowed child before ignoring them. Parents are created below.
            relative_path(&format!("{dir}/directory-check.txt"))?;
            continue;
        }
        let path = relative_path(&name)?;
        if entry.unix_mode().is_some_and(|mode| mode & 0o170000 != 0 && mode & 0o170000 != 0o100000) {
            return Err("The update ZIP contains a link or special file.".into());
        }
        if !matches!(entry.compression(), zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated) {
            return Err("The update ZIP uses unsupported compression.".into());
        }
        if !seen.insert(name.to_ascii_lowercase()) { return Err("The update ZIP contains duplicate Windows file names.".into()); }
        total = total.checked_add(entry.size()).ok_or("Update size overflow.")?;
        if total > MAX_EXPANDED || files.len() >= MAX_FILES { return Err("The expanded update exceeds its limits.".into()); }
        let target = checked_target(payload, &name, true)?;
        if target != payload.join(path) { return Err("Invalid extracted update path.".into()); }
        let mut output = fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| e.to_string())?;
        let copied = std::io::copy(&mut (&mut entry).take(MAX_EXPANDED + 1), &mut output).map_err(|e| format!("Update extraction failed: {e}"))?;
        if copied != entry.size() { return Err("An extracted update file has the wrong size.".into()); }
        output.flush().and_then(|_| output.sync_all()).map_err(|e| e.to_string())?;
        files.push(FileRecord { path: name, bytes: copied, sha256: hash_file(&target)? });
    }
    validate_records(&files)?;
    for name in [APP_EXE, "sdk/DymoBridge.exe", "licences/index.json", "COPYRIGHT.txt", "THIRD-PARTY-NOTICES.txt"] {
        if !files.iter().any(|file| file.path == name) { return Err("The update package is incomplete.".into()); }
    }
    Ok(files)
}

pub fn prepare(package: &Package, install: &Path, cancel: &AtomicBool, progress: impl Fn(String, u64, u64)) -> Result<Prepared, String> {
    let _download_guard = crate::updater::download_guard(install)?;
    let root = workspace(install)?;
    if root.join("install.lock.json").exists() || root.join("pending-backup").exists() { return Err("An installation or recovery is already pending. Reopen the app before downloading another update.".into()); }
    remove_owned(&root, "staging")?;
    let stage = root.join("staging");
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let result = (|| {
        download(&package.checksums, &stage.join("checksums.txt"), cancel, &progress)?;
        let text = fs::read_to_string(stage.join("checksums.txt")).map_err(|e| e.to_string())?;
        verify_checksum(&text, &package.archive)?;
        download(&package.archive, &stage.join("package.zip"), cancel, &progress)?;
        progress("Verifying and extracting the update…".into(), 0, 0);
        let files = extract(&stage.join("package.zip"), &stage.join("payload"), cancel)?;
        crate::updater::verify_executable(&stage.join("payload").join(APP_EXE), &package.version)?;
        cancelled(cancel)?;
        let prepared = Prepared { version: package.version.clone(), install: install.canonicalize().map_err(|e| e.to_string())?, files };
        prepared.save(&stage)?;
        Ok(prepared)
    })();
    if result.is_err() { let _ = remove_owned(&root, "staging"); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let path = std::env::temp_dir().join(format!("420p-update-package-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&path).unwrap(); path
    }
    fn cleanup(path: PathBuf) {
        assert_eq!(path.canonicalize().unwrap().parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
        assert!(path.file_name().unwrap().to_str().unwrap().starts_with("420p-update-package-"));
        fs::remove_dir_all(path).unwrap();
    }
    fn assets() -> serde_json::Value {
        let repo = crate::app_updates::DEFAULT_REPOSITORY;
        serde_json::json!({"assets":[
            {"name":"420p-label-studio-v1.2.3-windows-x64.zip","state":"uploaded","size":100,"digest":format!("sha256:{}", "a".repeat(64)),"browser_download_url":format!("https://github.com/{repo}/releases/download/v1.2.3/420p-label-studio-v1.2.3-windows-x64.zip")},
            {"name":"SHA256SUMS.txt","state":"uploaded","size":100,"digest":format!("sha256:{}", "b".repeat(64)),"browser_download_url":format!("https://github.com/{repo}/releases/download/v1.2.3/SHA256SUMS.txt")}
        ]})
    }
    fn zip_file(path: &Path, names: &[(&str, &[u8])]) {
        let mut archive = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, bytes) in names {
            archive.start_file(*name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
    }
    #[test] fn only_official_uploaded_bounded_digest_pinned_assets_are_installable() {
        let repo = crate::app_updates::DEFAULT_REPOSITORY;
        assert!(package(repo, "1.2.3", "v1.2.3", &assets()).is_some());
        assert!(package("example/another-app", "1.2.3", "v1.2.3", &assets()).is_none());
        for key in ["digest", "browser_download_url", "state"] {
            let mut value = assets(); value["assets"][0][key] = serde_json::json!("invalid");
            assert!(package(repo, "1.2.3", "v1.2.3", &value).is_none());
        }
        for size in [0, MAX_ARCHIVE + 1] {
            let mut value = assets(); value["assets"][0]["size"] = serde_json::json!(size);
            assert!(package(repo, "1.2.3", "v1.2.3", &value).is_none());
        }
        let mut value = assets(); let duplicate = value["assets"][0].clone(); value["assets"].as_array_mut().unwrap().push(duplicate);
        assert!(package(repo, "1.2.3", "v1.2.3", &value).is_none());
        assert!(package(repo, "1.2.3", "v1.2.3", &serde_json::json!({})).is_none());
    }
    #[test] fn checksum_requires_one_exact_archive_match() {
        let asset = package(crate::app_updates::DEFAULT_REPOSITORY, "1.2.3", "v1.2.3", &assets()).unwrap().archive;
        let line = format!("{}  {}\n", asset.sha256, asset.name);
        assert!(verify_checksum(&format!("# Language: en-GB\n{line}"), &asset).is_ok());
        assert!(verify_checksum(&format!("{line}{line}"), &asset).is_err());
        assert!(verify_checksum(&line.replace('a', "c"), &asset).is_err());
        assert!(verify_checksum(&format!("{} extra", line.trim()), &asset).is_err());
        assert!(verify_checksum("", &asset).is_err());
    }
    #[test] fn windows_traversal_streams_reserved_names_and_private_files_are_rejected() {
        for name in ["../outside.exe", "/outside.exe", "C:/outside.exe", "sdk/../../outside.exe", "sdk\\outside.dll", "sdk/file.dll:stream", "sdk/CON.dll", "sdk/com1.dll", "sdk/LPT9.json", "sdk/a./b.dll", "sdk/a /b.dll", "sdk//b.dll", "sdk/./b.dll", "sdk/evil.ps1", "licences/evil.exe", "app-updates.json", ".420p-updater/plan.json", "label.420p", "sdk/label.420p"] {
            assert!(relative_path(name).is_err(), "{name}");
        }
        for name in [APP_EXE, "sdk/templates/9mm.dymo", "sdk/runtime.dll", "licences/font/Apache-2.0", "licences/index.json"] { assert!(relative_path(name).is_ok(), "{name}"); }
    }
    #[test] fn expanded_size_duplicate_names_and_modified_files_are_rejected() {
        let dir = fixture(); fs::write(dir.join(APP_EXE), b"original").unwrap();
        let file = FileRecord { path: APP_EXE.into(), bytes: 8, sha256: hash_file(&dir.join(APP_EXE)).unwrap() };
        assert!(verify_files(&dir, std::slice::from_ref(&file)).is_ok());
        fs::write(dir.join(APP_EXE), b"modified").unwrap();
        assert!(verify_files(&dir, std::slice::from_ref(&file)).is_err());
        assert!(validate_records(&[file.clone(), file.clone()]).is_err());
        let huge = FileRecord { bytes: MAX_EXPANDED + 1, ..file };
        assert!(validate_records(&[huge]).is_err()); cleanup(dir);
    }
    #[test] fn extraction_rejects_partial_corrupt_duplicate_and_escaping_archives() {
        for bad in ["../outside.exe", "sdk/a.dll:stream", "sdk/NUL.dll"] {
            let dir = fixture(); zip_file(&dir.join("test.zip"), &[(bad, b"bad")]);
            assert!(extract(&dir.join("test.zip"), &dir.join("payload"), &AtomicBool::new(false)).is_err());
            assert!(!dir.join("outside.exe").exists()); cleanup(dir);
        }
        let dir = fixture();
        zip_file(&dir.join("test.zip"), &[("sdk/A.dll", b"one"), ("sdk/a.dll", b"two")]);
        assert!(extract(&dir.join("test.zip"), &dir.join("payload"), &AtomicBool::new(false)).is_err());
        fs::write(dir.join("broken.zip"), b"not a ZIP").unwrap();
        assert!(extract(&dir.join("broken.zip"), &dir.join("other"), &AtomicBool::new(false)).is_err()); cleanup(dir);
    }
    #[test] fn complete_archive_round_trips_and_cancel_does_not_extract() {
        let dir = fixture();
        zip_file(&dir.join("test.zip"), &[(APP_EXE, b"application"), ("sdk/DymoBridge.exe", b"bridge"), ("licences/index.json", b"{}"), ("COPYRIGHT.txt", b"Language: en-GB"), ("THIRD-PARTY-NOTICES.txt", b"Language: en-GB")]);
        let files = extract(&dir.join("test.zip"), &dir.join("payload"), &AtomicBool::new(false)).unwrap();
        assert_eq!(files.len(), 5); verify_files(&dir.join("payload"), &files).unwrap();
        assert!(extract(&dir.join("test.zip"), &dir.join("cancelled"), &AtomicBool::new(true)).is_err());
        assert!(!dir.join("cancelled").exists()); cleanup(dir);
    }
    #[test] fn workspace_cleanup_is_scoped_and_never_touches_user_files() {
        let dir = fixture(); fs::write(dir.join("label.420p"), b"synthetic user label").unwrap();
        let root = workspace(&dir).unwrap(); fs::create_dir(root.join("staging")).unwrap(); fs::write(root.join("staging/partial.zip"), b"partial").unwrap();
        assert!(remove_owned(&root, "..").is_err()); remove_owned(&root, "staging").unwrap();
        assert_eq!(fs::read(dir.join("label.420p")).unwrap(), b"synthetic user label"); cleanup(dir);
    }
    #[test] fn an_existing_unowned_workspace_is_preserved() {
        let dir = fixture(); fs::create_dir(dir.join(".420p-updater")).unwrap(); fs::write(dir.join(".420p-updater/user.txt"), b"keep").unwrap();
        assert!(workspace(&dir).is_err()); assert_eq!(fs::read(dir.join(".420p-updater/user.txt")).unwrap(), b"keep"); cleanup(dir);
    }
    #[test] fn junctions_cannot_redirect_extraction_or_cleanup() {
        let dir = fixture(); let outside = fixture(); fs::write(outside.join("keep.dll"), b"keep").unwrap();
        let root = workspace(&dir).unwrap(); fs::create_dir(root.join("staging")).unwrap();
        let link = root.join("staging/sdk");
        let status = Command::new("cmd.exe").creation_flags(0x08000000).args(["/c", "mklink", "/J"]).arg(&link).arg(&outside).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap();
        assert!(status.success(), "creating a directory junction is required for this security regression test");
        assert!(checked_target(&root.join("staging"), "sdk/keep.dll", true).is_err());
        assert!(remove_owned(&root, "staging").is_err());
        assert_eq!(fs::read(outside.join("keep.dll")).unwrap(), b"keep");
        fs::remove_dir(link).unwrap(); cleanup(dir); cleanup(outside);
    }
}
