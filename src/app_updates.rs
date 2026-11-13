//! Public, unauthenticated GitHub release checks and verified asset metadata.
use std::{fs, io::Read, os::windows::process::CommandExt, path::PathBuf, process::{Command, Stdio}};

const MAX_RESPONSE_BYTES: usize = 1_048_576;
const MAX_NOTES_CHARS: usize = 8_000;
const MAX_PREFERENCES_BYTES: u64 = 4_096;
pub const DEFAULT_REPOSITORY: &str = "cantwalkwontrun/420p-label-studio";
const PREVIOUS_OWNER_REPOSITORY: &str = "noswimmingplease/420p-label-studio";
const RENAMED_LEGACY_REPOSITORY: &str = "cantwalkwontrun/dymo-420p-rust";
const LEGACY_REPOSITORY: &str = "noswimmingplease/dymo-420p-rust";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub newer: bool,
    pub package: Option<crate::update_package::Package>,
}

pub fn normalise_repository(input: &str) -> Result<String, String> {
    let input = input.trim();
    let input = input.strip_prefix("https://github.com/").unwrap_or(input).trim_end_matches('/');
    let input = input.strip_suffix(".git").unwrap_or(input);
    let (owner, repository) = input.split_once('/').ok_or("Enter a public GitHub repository as owner/repository or its https://github.com URL.")?;
    let owner_valid = !owner.is_empty() && owner.len() <= 39 && !owner.starts_with('-') && !owner.ends_with('-')
        && owner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    let repository_valid = !repository.is_empty() && repository.len() <= 100 && !matches!(repository, "." | "..")
        && repository.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !owner_valid || !repository_valid { return Err("Use a valid public GitHub owner/repository without extra paths, queries or credentials.".into()); }
    let repository = format!("{owner}/{repository}");
    // Migrate this project's known rename without trusting arbitrary redirects.
    if [LEGACY_REPOSITORY, PREVIOUS_OWNER_REPOSITORY, RENAMED_LEGACY_REPOSITORY].iter().any(|previous| repository.eq_ignore_ascii_case(previous)) {
        Ok(DEFAULT_REPOSITORY.to_owned())
    } else {
        Ok(repository)
    }
}

fn preferences_path() -> Result<PathBuf, String> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or("Windows local application data folder is unavailable.")?;
    Ok(PathBuf::from(local).join("Dymo420P").join("app-updates.json"))
}

pub fn load_repository() -> Result<String, String> {
    read_repository(&preferences_path()?)
}

fn read_repository(path: &std::path::Path) -> Result<String, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(DEFAULT_REPOSITORY.to_owned()),
        Err(error) => return Err(format!("Cannot read app update preferences: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(MAX_PREFERENCES_BYTES + 1).read_to_end(&mut bytes).map_err(|e| format!("Cannot read app update preferences: {e}"))?;
    if bytes.len() > MAX_PREFERENCES_BYTES as usize { return Err("App update preferences exceed the 4 KiB limit.".into()); }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| format!("Invalid app update preferences: {e}"))?;
    let fields = value.as_object().ok_or("Invalid app update preferences.")?;
    if fields.keys().any(|key| key != "version" && key != "repository") || value["version"].as_u64() != Some(1) {
        return Err("Unsupported app update preferences; the file was not changed.".into());
    }
    let repository = value["repository"].as_str().ok_or("The saved app update repository is missing.")?;
    if repository.is_empty() { Ok(String::new()) } else { normalise_repository(repository) }
}

pub fn save_repository(input: &str) -> Result<(), String> {
    let repository = if input.trim().is_empty() { String::new() } else { normalise_repository(input)? };
    let path = preferences_path()?;
    fs::create_dir_all(path.parent().ok_or("The app update preferences folder is unavailable.")?).map_err(|e| format!("Cannot create app update preferences folder: {e}"))?;
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({"version": 1, "repository": repository})).map_err(|e| e.to_string())?;
    crate::project_io::write_project(&path, &bytes, path.exists())
}

pub fn check(input: &str) -> Result<Release, String> {
    let repository = normalise_repository(input)?;
    let root = std::env::var_os("SystemRoot").ok_or("Windows directory is unavailable.")?;
    let curl = PathBuf::from(root).join("System32").join("curl.exe");
    let url = format!("https://api.github.com/repos/{repository}/releases/latest");
    // --disable must be first: user curl configuration must not inject tokens
    // or other credentials into this deliberately unauthenticated request.
    let child = Command::new(curl).creation_flags(0x08000000)
        .args(["--disable", "--silent", "--show-error", "--connect-timeout", "4", "--max-time", "12", "--max-filesize", "1048576", "--proto", "=https",
            "--header", "Accept: application/vnd.github+json", "--user-agent", "Dymo420P-App-Update-Check",
            "--write-out", "\n%{http_code}", "--url", &url])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("Cannot start the Windows update-check client: {e}"))?;
    let output = crate::sdk::wait_child(child, std::time::Duration::from_secs(15))
        .map_err(|_| "The GitHub app update check could not finish within its resource limits.".to_owned())?;
    if !output.status.success() { return Err("Could not read GitHub release information. Check the connection and try again.".into()); }
    if output.stdout.len() > MAX_RESPONSE_BYTES + 4 { return Err("GitHub release information exceeded the 1 MiB limit.".into()); }
    let split = output.stdout.iter().rposition(|b| *b == b'\n').ok_or("GitHub did not return an HTTP status.")?;
    let status = std::str::from_utf8(&output.stdout[split + 1..]).ok().and_then(|v| v.parse::<u16>().ok()).ok_or("GitHub returned an invalid HTTP status.")?;
    parse_release(&repository, status, &output.stdout[..split], env!("CARGO_PKG_VERSION"))
}

fn parse_release(repository: &str, status: u16, body: &[u8], current: &str) -> Result<Release, String> {
    if body.len() > MAX_RESPONSE_BYTES { return Err("GitHub release information exceeded the 1 MiB limit.".into()); }
    match status {
        200 => {}
        404 => return Err("No public stable release was found. The repository may be private, missing or have no published releases.".into()),
        401 | 403 | 429 => return Err("GitHub denied this unauthenticated check or its rate limit was reached. Try again later.".into()),
        300..=399 => return Err("GitHub redirected this repository. Check its current owner/repository in Settings.".into()),
        _ => return Err(format!("GitHub release check failed (HTTP {status}).")),
    }
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| "GitHub returned invalid release information.")?;
    if value["draft"].as_bool() != Some(false) || value["prerelease"].as_bool() != Some(false) {
        return Err("GitHub did not return a published stable release.".into());
    }
    let tag = value["tag_name"].as_str().ok_or("The GitHub release tag is missing.")?;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    let latest = version_parts(version).ok_or("The latest release tag is not a supported numeric version (for example v1.2.3).")?;
    let current = version_parts(current).ok_or("This application's version cannot be compared safely.")?;
    let url = value["html_url"].as_str().ok_or("The GitHub release page is missing.")?;
    let prefix = format!("https://github.com/{repository}/releases/tag/");
    if !url.get(..prefix.len()).is_some_and(|part| part.eq_ignore_ascii_case(&prefix)) || url.get(prefix.len()..) != Some(tag) {
        return Err("The release page does not belong to the configured repository and tag.".into());
    }
    let notes = value["body"].as_str().unwrap_or("");
    let mut trimmed: String = notes.chars().take(MAX_NOTES_CHARS).collect();
    if notes.chars().count() > MAX_NOTES_CHARS { trimmed.push_str("\n… Open the release page for the remaining notes."); }
    Ok(Release { version: version.to_owned(), notes: trimmed, url: url.to_owned(), newer: latest > current,
        package: crate::update_package::package(repository, version, tag, &value) })
}

fn version_parts(version: &str) -> Option<[u32; 4]> {
    let parts: Vec<_> = version.split('.').collect();
    if !(3..=4).contains(&parts.len()) { return None; }
    let mut result = [0; 4];
    for (index, part) in parts.into_iter().enumerate() {
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) || !part.bytes().all(|b| b.is_ascii_digit()) { return None; }
        result[index] = part.parse().ok()?;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn new_install_uses_project_repository_and_preserves_explicit_preferences() {
        let path = std::env::temp_dir().join(format!("dymo-update-default-{}-{}.json", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        assert_eq!(read_repository(&path).unwrap(), DEFAULT_REPOSITORY);
        for (repository, expected) in [("", ""), ("example/another-app", "example/another-app"),
            (LEGACY_REPOSITORY, DEFAULT_REPOSITORY), (DEFAULT_REPOSITORY, DEFAULT_REPOSITORY)] {
            let bytes = serde_json::to_vec(&serde_json::json!({"version":1,"repository":repository})).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert_eq!(read_repository(&path).unwrap(), expected);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        fs::remove_file(path).unwrap();
    }
    fn response(tag: &str, url: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"draft": false, "prerelease": false, "tag_name": tag, "html_url": url, "body": "Synthetic release notes."})).unwrap()
    }
    #[test] fn known_repository_rename_migrates_saved_values_and_checks_the_new_release_page() {
        for input in [LEGACY_REPOSITORY, PREVIOUS_OWNER_REPOSITORY, RENAMED_LEGACY_REPOSITORY, "NOSWIMMINGPLEASE/420P-LABEL-STUDIO", "NOSWIMMINGPLEASE/DYMO-420P-RUST",
            " https://github.com/noswimmingplease/dymo-420p-rust.git/ "] {
            let repository = normalise_repository(input).unwrap();
            assert_eq!(repository, DEFAULT_REPOSITORY);
            let url = format!("https://github.com/{DEFAULT_REPOSITORY}/releases/tag/v0.1.3");
            let release = parse_release(&repository, 200, &response("v0.1.3", &url), "0.1.2").unwrap();
            assert!(release.newer);
            assert_eq!(release.url, url);
        }
        for input in ["example/dymo-420p-rust", "example/another-app"] {
            assert_eq!(normalise_repository(input).unwrap(), input);
        }
        assert!(normalise_repository("noswimmingplease/dymo-420p-rust/extra").is_err());
        assert!(normalise_repository("noswimmingplease/dymo-420p-rust?token=secret").is_err());
    }
    #[test] fn repository_normalisation_rejects_extra_paths_and_credentials() {
        assert_eq!(normalise_repository(" https://github.com/example-owner/label-app.git/ ").unwrap(), "example-owner/label-app");
        assert_eq!(normalise_repository("example-owner/label_app").unwrap(), "example-owner/label_app");
        for input in ["", "owner", "owner/repo/extra", "https://example.invalid/owner/repo", "https://github.com/user:secret@owner/repo", "owner/repo?token=secret", "../repo", "owner/..", "-owner/repo", "owner/répo"] { assert!(normalise_repository(input).is_err(), "{input}"); }
    }
    #[test] fn stable_release_comparison_and_page_validation() {
        let repo = "example-owner/label-app";
        let release = parse_release(repo, 200, &response("v1.2.3", "https://github.com/example-owner/label-app/releases/tag/v1.2.3"), "1.2.2").unwrap();
        assert!(release.newer); assert_eq!(release.version, "1.2.3");
        assert!(!parse_release(repo, 200, &response("v1.2.3.0", "https://github.com/example-owner/label-app/releases/tag/v1.2.3.0"), "1.2.3").unwrap().newer);
        for url in ["https://example.invalid/file.exe", "https://github.com/other/repo/releases/tag/v1.2.3", "https://github.com/example-owner/label-app/releases/tag/v1.2.3?redirect=1", "https://github.com/example-owner/label-app/releases/tag/v1.2.2"] { assert!(parse_release(repo, 200, &response("v1.2.3", url), "1.2.2").is_err()); }
    }
    #[test] fn unavailable_or_unstable_releases_never_report_up_to_date() {
        for status in [301, 401, 403, 404, 429, 500] { assert!(parse_release("example/repo", status, b"{}", "0.1.0").is_err()); }
        assert!(parse_release("example/repo", 200, b"not json", "0.1.0").is_err());
        for tag in ["v1.2.3-beta", "v1.2.3+metadata", "v01.2.3", "release-latest", "1.2", "1.2.3.4.5"] { assert!(version_parts(tag.strip_prefix('v').unwrap_or(tag)).is_none()); }
        let mut value: serde_json::Value = serde_json::from_slice(&response("v1.0.0", "https://github.com/example/repo/releases/tag/v1.0.0")).unwrap();
        value["prerelease"] = serde_json::json!(true);
        assert!(parse_release("example/repo", 200, &serde_json::to_vec(&value).unwrap(), "0.1.0").is_err());
    }
    #[test] fn preferences_and_notes_are_bounded() {
        let path = std::env::temp_dir().join(format!("dymo-update-preferences-{}.json", std::process::id()));
        fs::write(&path, serde_json::to_vec(&serde_json::json!({"version":1,"repository":"example/repo"})).unwrap()).unwrap();
        assert_eq!(read_repository(&path).unwrap(), "example/repo");
        fs::write(&path, vec![b' '; MAX_PREFERENCES_BYTES as usize + 1]).unwrap();
        assert!(read_repository(&path).is_err()); fs::remove_file(&path).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&response("v1.0.0", "https://github.com/example/repo/releases/tag/v1.0.0")).unwrap();
        value["body"] = serde_json::json!("é".repeat(MAX_NOTES_CHARS + 1));
        let release = parse_release("example/repo", 200, &serde_json::to_vec(&value).unwrap(), "0.1.0").unwrap();
        assert!(release.notes.ends_with("remaining notes.")); assert!(release.notes.len() < 2 * MAX_NOTES_CHARS + 100);
        assert!(parse_release("example/repo", 200, &vec![b' '; MAX_RESPONSE_BYTES + 1], "0.1.0").is_err());
    }
}
