//! Self-update from the latest GitHub release.

use sha2::{Digest, Sha256};
use std::fmt;
use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const USER_BIN_DIR: &str = ".local/bin";
const REQUEST_TIMEOUT_SECONDS: u64 = 120;

/// A single downloadable asset attached to a GitHub release.
struct ReleaseAsset {
    name: String,
    download_url: String,
}

/// A self-update failure.
struct UpdateError(String);

impl fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub(super) fn run() -> ExitCode {
    println!("Current version: {VERSION}");
    match update() {
        Ok(Some(tag)) => {
            println!("Update completed: {VERSION} -> {tag}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

/// Downloads and installs the latest release. `Ok(None)` means already up to date.
fn update() -> Result<Option<String>, UpdateError> {
    ensure_updatable_install()?;

    let agent = new_agent();
    print!("Checking for updates... ");
    flush_stdout();
    let (tag, assets) = fetch_latest_release(&agent)?;
    if is_up_to_date(VERSION, &tag) {
        println!("already up to date.");
        return Ok(None);
    }
    println!("found {tag}");

    let triple = target_triple(std::env::consts::ARCH, std::env::consts::OS);
    let archive_url = resolve_asset_url(&assets, &triple).ok_or_else(|| {
        UpdateError(format!(
            "no binary found for {triple} in the latest release ({tag})"
        ))
    })?;
    let checksum_url = resolve_checksum_url(&assets).ok_or_else(|| {
        UpdateError(format!(
            "no SHA256SUMS.txt found in the latest release ({tag})"
        ))
    })?;

    let archive = download_archive(&agent, archive_url)?;
    verify_archive_checksum(&agent, archive_url, checksum_url, &archive)?;

    let destination = staged_binary_path()?;
    extract_binary(&archive, &destination)?;
    install_binary(&destination)?;
    Ok(Some(tag))
}

/// True when the latest release tag matches the running version.
fn is_up_to_date(current_version: &str, latest_tag: &str) -> bool {
    latest_tag == format!("v{current_version}")
}

/// The release asset target triple for the running platform.
fn target_triple(arch: &str, os: &str) -> String {
    match os {
        "linux" => format!("{arch}-unknown-linux-gnu"),
        "macos" => format!("{arch}-apple-darwin"),
        "windows" => format!("{arch}-pc-windows-msvc"),
        other => format!("{arch}-{other}"),
    }
}

/// The `owner/repo` path taken from the package repository URL.
fn repository_path() -> &'static str {
    const PREFIX: &str = "https://github.com/";
    match env!("CARGO_PKG_REPOSITORY").strip_prefix(PREFIX) {
        Some(path) => path,
        None => env!("CARGO_PKG_REPOSITORY"),
    }
}

/// The installed binary name, e.g. `sonalloy` or `sonalloy.exe`.
fn binary_name() -> String {
    format!("sonalloy{}", std::env::consts::EXE_SUFFIX)
}

/// The user-local binary that self-update keeps up to date.
fn user_binary_path() -> Result<PathBuf, UpdateError> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| UpdateError("HOME directory could not be resolved".into()))?;
    Ok(home.join(USER_BIN_DIR).join(binary_name()))
}

/// The directory that holds the user-local binary.
fn install_dir() -> Result<PathBuf, UpdateError> {
    user_binary_path()?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| UpdateError("could not determine the install directory".into()))
}

/// The staging path a downloaded binary is extracted to before the swap.
fn staged_binary_path() -> Result<PathBuf, UpdateError> {
    Ok(install_dir()?.join(format!(".{}.new", binary_name())))
}

/// Verifies that the running binary is the user-local install self-update can replace.
fn ensure_updatable_install() -> Result<(), UpdateError> {
    let expected = user_binary_path()?;
    let expected_metadata = std::fs::symlink_metadata(&expected).map_err(|error| {
        UpdateError(format!(
            "self-update requires a user-local install at {}: {error}",
            expected.display()
        ))
    })?;
    if expected_metadata.file_type().is_symlink() {
        return Err(UpdateError(format!(
            "self-update requires {} to be a regular file, not a symlink. Reinstall Sonalloy with:\n  \
             curl -fsSL https://raw.githubusercontent.com/endo-ly/sonalloy/main/scripts/install.sh | bash",
            expected.display()
        )));
    }

    let current = std::env::current_exe()
        .map_err(|error| UpdateError(format!("failed to locate the running binary: {error}")))?;
    let current = current.canonicalize().unwrap_or(current);
    let expected_dir = expected
        .parent()
        .ok_or_else(|| UpdateError("could not determine the install directory".into()))?;

    if current.file_name() == Some(std::ffi::OsStr::new(binary_name().as_str()))
        && current.parent() == Some(expected_dir)
    {
        return Ok(());
    }

    Err(UpdateError(format!(
        "self-update requires a user-local install at {}. Current binary is {}. Reinstall Sonalloy with:\n  \
         curl -fsSL https://raw.githubusercontent.com/endo-ly/sonalloy/main/scripts/install.sh | bash",
        expected.display(),
        current.display()
    )))
}

fn new_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECONDS))
        .build()
}

fn get(agent: &ureq::Agent, url: &str) -> Result<ureq::Response, UpdateError> {
    let user_agent = format!("sonalloy/{VERSION}");
    match agent.get(url).set("User-Agent", &user_agent).call() {
        Ok(response) => Ok(response),
        Err(ureq::Error::Status(status, _)) => Err(UpdateError(format!(
            "request to {url} failed: HTTP {status}"
        ))),
        Err(ureq::Error::Transport(transport)) => {
            Err(UpdateError(format!("request to {url} failed: {transport}")))
        }
    }
}

/// Fetches the tag name and assets of the latest GitHub release.
fn fetch_latest_release(agent: &ureq::Agent) -> Result<(String, Vec<ReleaseAsset>), UpdateError> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        repository_path()
    );
    let json: serde_json::Value = get(agent, &url)?
        .into_json()
        .map_err(|error| UpdateError(format!("failed to parse the release response: {error}")))?;

    let tag = json["tag_name"]
        .as_str()
        .ok_or_else(|| UpdateError("missing 'tag_name' in the release response".into()))?
        .to_string();
    let assets = json["assets"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| {
                    let name = value["name"].as_str()?;
                    let download_url = value["browser_download_url"].as_str()?;
                    Some(ReleaseAsset {
                        name: name.to_owned(),
                        download_url: download_url.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok((tag, assets))
}

/// Finds the tar.gz archive URL matching the given target triple.
fn resolve_asset_url<'a>(assets: &'a [ReleaseAsset], triple: &str) -> Option<&'a str> {
    assets.iter().find_map(|asset| {
        (asset.name.contains(triple) && asset.name.ends_with(".tar.gz"))
            .then_some(asset.download_url.as_str())
    })
}

/// Finds the SHA256SUMS.txt URL.
fn resolve_checksum_url(assets: &[ReleaseAsset]) -> Option<&str> {
    assets
        .iter()
        .find_map(|asset| (asset.name == "SHA256SUMS.txt").then_some(asset.download_url.as_str()))
}

/// Splits a `sha256sum` manifest line into the file name and digest.
fn parse_checksum_line(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.split_whitespace();
    let digest = parts.next()?;
    let name = parts.next()?.trim_start_matches('*');
    let name = name.strip_prefix("./").unwrap_or(name);
    (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some((name, digest))
}

/// Downloads the archive with a progress report on stderr.
fn download_archive(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, UpdateError> {
    let response = get(agent, url)?;
    let total = response
        .header("Content-Length")
        .and_then(|length| length.parse().ok());
    let mut reader = response.into_reader();

    let mut bytes = Vec::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut downloaded: usize = 0;
    let mut reported_percent: usize = 0;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| UpdateError(format!("download failed: {error}")))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        downloaded += read;
        if let Some(total) = total.filter(|total| *total > 0) {
            let percent = downloaded.saturating_mul(100) / total;
            if percent >= reported_percent + 5 || percent >= 100 {
                eprint!(
                    "\r  {} / {} ({}%)  ",
                    format_size(downloaded),
                    format_size(total),
                    percent
                );
                flush_stderr();
                reported_percent = percent;
            }
        }
    }
    eprintln!();
    Ok(bytes)
}

/// Verifies the downloaded archive against SHA256SUMS.txt.
fn verify_archive_checksum(
    agent: &ureq::Agent,
    archive_url: &str,
    checksum_url: &str,
    bytes: &[u8],
) -> Result<(), UpdateError> {
    eprint!("  Verifying checksum... ");
    flush_stderr();
    let manifest = get(agent, checksum_url)?
        .into_string()
        .map_err(|error| UpdateError(format!("failed to read SHA256SUMS.txt: {error}")))?;

    let archive_name = archive_url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| UpdateError("failed to derive the archive file name".into()))?;
    let expected = manifest
        .lines()
        .filter_map(parse_checksum_line)
        .find_map(|(name, digest)| (name == archive_name).then_some(digest))
        .ok_or_else(|| {
            UpdateError(format!(
                "SHA256SUMS.txt does not contain a checksum for {archive_name}"
            ))
        })?;

    let digest = Sha256::digest(bytes);
    let mut actual = String::with_capacity(digest.len() * 2);
    for byte in &digest {
        write!(actual, "{byte:02x}").expect("writing to a String cannot fail");
    }
    if actual != expected {
        return Err(UpdateError(format!(
            "checksum mismatch for {archive_name}: expected {expected}, got {actual}"
        )));
    }
    eprintln!("ok");
    Ok(())
}

/// Extracts the binary from a tar.gz archive to the staging path.
fn extract_binary(archive_bytes: &[u8], destination: &Path) -> Result<(), UpdateError> {
    eprint!("  Extracting binary... ");
    flush_stderr();
    let result = extract_first_binary(archive_bytes, destination);
    if result.is_err() {
        let _ = std::fs::remove_file(destination);
    }
    result?;
    eprintln!("done");
    Ok(())
}

fn extract_first_binary(archive_bytes: &[u8], destination: &Path) -> Result<(), UpdateError> {
    let decoder = flate2::read::GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(decoder);
    let expected_name = binary_name();

    let entries = archive
        .entries()
        .map_err(|error| UpdateError(format!("failed to read the archive entries: {error}")))?;
    for entry in entries {
        let mut entry = entry
            .map_err(|error| UpdateError(format!("error reading the archive entry: {error}")))?;
        let is_binary = entry
            .path()
            .map_err(|error| UpdateError(format!("error reading the archive entry: {error}")))?
            .file_name()
            .and_then(|name| name.to_str())
            == Some(expected_name.as_str());
        if is_binary {
            return entry
                .unpack(destination)
                .map(|_| ())
                .map_err(|error| UpdateError(format!("failed to extract the binary: {error}")));
        }
    }
    Err(UpdateError(format!(
        "could not find '{expected_name}' in the downloaded archive"
    )))
}

/// Atomically replaces the running binary with the staged one.
///
/// On success the old binary is kept as `.sonalloy.old` in the same directory.
/// On failure the original binary is restored.
fn install_binary(staged: &Path) -> Result<(), UpdateError> {
    let current_exe = std::env::current_exe()
        .map_err(|error| UpdateError(format!("failed to locate the running binary: {error}")))?;
    let current_exe = current_exe.canonicalize().unwrap_or(current_exe);
    let exe_dir = current_exe
        .parent()
        .ok_or_else(|| UpdateError("could not determine the binary directory".into()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(staged, std::fs::Permissions::from_mode(0o755)).map_err(
            |error| {
                UpdateError(format!(
                    "failed to set permissions on the new binary: {error}"
                ))
            },
        )?;
    }

    let backup = exe_dir.join(format!(".{}.old", binary_name()));
    std::fs::rename(&current_exe, &backup).map_err(|error| {
        UpdateError(format!("failed to move the current binary aside: {error}"))
    })?;

    if let Err(error) = std::fs::rename(staged, &current_exe) {
        return match std::fs::rename(&backup, &current_exe) {
            Ok(()) => Err(UpdateError(format!(
                "failed to install the new binary (rolled back): {error}"
            ))),
            Err(rollback_error) => Err(UpdateError(format!(
                "failed to install the new binary and rollback failed: install error: {error}; rollback error: {rollback_error}"
            ))),
        };
    }
    Ok(())
}

fn format_size(bytes: usize) -> String {
    const MB: usize = 1024 * 1024;
    const KB: usize = 1024;
    if bytes >= MB {
        let whole = bytes / MB;
        let tenths = bytes % MB * 10 / MB;
        format!("{whole}.{tenths} MB")
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

fn flush_stdout() {
    let _ = std::io::stdout().flush();
}

fn flush_stderr() {
    let _ = std::io::stderr().flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_asset_url_matches_triple_and_archive_suffix() {
        let assets = vec![
            ReleaseAsset {
                name: "SHA256SUMS.txt".into(),
                download_url: "https://example.com/SHA256SUMS.txt".into(),
            },
            ReleaseAsset {
                name: "sonalloy-2026.9.2-x86_64-unknown-linux-gnu.tar.gz".into(),
                download_url: "https://example.com/linux.tar.gz".into(),
            },
        ];

        let resolved = resolve_asset_url(&assets, "x86_64-unknown-linux-gnu");

        assert_eq!(resolved, Some("https://example.com/linux.tar.gz"));
        assert_eq!(resolve_asset_url(&assets, "aarch64-apple-darwin"), None);
    }

    #[test]
    fn parse_checksum_line_reads_sha256sum_format() {
        let digest = "a".repeat(64);

        assert_eq!(
            parse_checksum_line(&format!("{digest}  ./sonalloy.tar.gz")),
            Some(("sonalloy.tar.gz", digest.as_str()))
        );
        assert_eq!(
            parse_checksum_line(&format!("{digest} *sonalloy.tar.gz")),
            Some(("sonalloy.tar.gz", digest.as_str()))
        );
        assert_eq!(parse_checksum_line("short sonalloy.tar.gz"), None);
    }

    #[test]
    fn target_triple_matches_release_asset_names() {
        assert_eq!(target_triple("x86_64", "linux"), "x86_64-unknown-linux-gnu");
        assert_eq!(target_triple("aarch64", "macos"), "aarch64-apple-darwin");
        assert_eq!(target_triple("x86_64", "windows"), "x86_64-pc-windows-msvc");
    }

    #[test]
    fn up_to_date_compares_tagged_latest_release() {
        assert!(is_up_to_date("2026.9.2", "v2026.9.2"));
        assert!(!is_up_to_date("2026.9.2", "v2026.8.30"));
    }
}
