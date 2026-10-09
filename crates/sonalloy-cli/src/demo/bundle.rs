use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sonalloy_core::definition::InstrumentDefinition;
use sonalloy_core::{Diagnostic, DiagnosticCode};

use crate::command::render_bundle_demo;
use crate::output::CliFailure;

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_RENAME_INFO, FILE_RENAME_INFO_0,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileRenameInfoEx, OPEN_EXISTING,
    SetFileInformationByHandle,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RenderSettings {
    pub(crate) sample_rate: u32,
    pub(crate) block_size: usize,
    pub(crate) tail_seconds: f64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format_version: u32,
    demo: String,
    render_settings: RenderSettings,
    render: Option<RenderFiles>,
    files: Vec<ManifestFile>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderFiles {
    mix: String,
    stems: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    path: String,
    sha256: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PackReport {
    status: &'static str,
    command: &'static str,
    output: String,
    format_version: u32,
    part_count: usize,
    file_count: usize,
    render_included: bool,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

pub(crate) fn pack(
    input: &Path,
    output: &Path,
    settings: RenderSettings,
    with_render: bool,
) -> Result<PackReport, CliFailure> {
    reject_existing_output(output)?;
    if settings.sample_rate == 0
        || settings.block_size == 0
        || !settings.tail_seconds.is_finite()
        || settings.tail_seconds < 0.0
    {
        return Err(failure(
            DiagnosticCode::ValueOutOfRange,
            "invalid render settings",
            "render_settings",
            "sample rate and block size must be positive; tail must be finite and non-negative",
        ));
    }
    let source = super::load(input, settings.sample_rate, settings.block_size)?;
    validate_part_paths(&source.definition)?;
    // Preserve omitted fields as well as every value outside the relocated references.
    let mut definition: serde_json::Value = read_json(input)?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = tempfile::Builder::new()
        .prefix(".sonalloy-bundle-")
        .tempdir_in(parent)
        .map_err(|error| io_failure(output, "could not create bundle staging directory", error))?;
    let root = temporary.path();
    fs::create_dir(root.join("patterns"))
        .map_err(|error| io_failure(root, "could not create patterns directory", error))?;
    relocate_parts(input, root, &source, &mut definition)?;
    drop(source);
    write_json(&root.join("demo.json"), &definition)?;
    let relocated = super::load(
        &root.join("demo.json"),
        settings.sample_rate,
        settings.block_size,
    )?;
    let render = if with_render {
        render_bundle_demo(
            root.join("demo.json"),
            root.join("render/mix.wav"),
            root.join("render/stems"),
            settings.sample_rate,
            settings.block_size,
            settings.tail_seconds,
        )?;
        Some(RenderFiles {
            mix: "render/mix.wav".to_owned(),
            stems: relocated
                .parts
                .iter()
                .map(|part| {
                    (
                        part.definition.id.clone(),
                        format!("render/stems/{}.wav", part.definition.id),
                    )
                })
                .collect(),
        })
    } else {
        None
    };
    let manifest = Manifest {
        format_version: 1,
        demo: "demo.json".to_owned(),
        render_settings: settings,
        render,
        files: list_files(root)?,
    };
    write_json(&root.join("bundle.json"), &manifest)?;
    verify_manifest(root, &relocated.definition)?;
    commit_directory(root, output).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            failure(
                DiagnosticCode::BundleOutputExists,
                "bundle output already exists",
                output.to_string_lossy(),
                error,
            )
        } else {
            io_failure(output, "could not commit bundle directory", error)
        }
    })?;
    Ok(PackReport {
        status: "ok",
        command: "demo pack",
        output: output.to_string_lossy().into_owned(),
        format_version: 1,
        part_count: relocated.parts.len(),
        file_count: manifest.files.len(),
        render_included: with_render,
        diagnostics: relocated.diagnostics,
    })
}

fn portable_path_key(path: &str) -> String {
    path.split('/')
        .map(|component| component.trim_end_matches('.').to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("/")
}

fn validate_part_paths(definition: &super::DemoDefinition) -> Result<(), CliFailure> {
    let mut names = HashSet::new();
    for (index, part) in definition.parts.iter().enumerate() {
        // Windows drops trailing periods from file names, which would detach the
        // directory from the path every reference inside the bundle points at.
        if part.id.ends_with('.') {
            return Err(failure(
                DiagnosticCode::ValueOutOfRange,
                "bundle part id must not end with a period",
                format!("parts[{index}].id"),
                &part.id,
            ));
        }
        if !names.insert(portable_path_key(&part.id)) {
            return Err(failure(
                DiagnosticCode::IdDuplicated,
                "bundle instrument directory names collide",
                format!("parts[{index}].id"),
                &part.id,
            ));
        }
    }
    Ok(())
}

fn relocate_parts(
    input: &Path,
    root: &Path,
    source: &super::LoadedDemo,
    definition: &mut serde_json::Value,
) -> Result<(), CliFailure> {
    let base = input.parent().unwrap_or_else(|| Path::new("."));
    let mut hashes = HashMap::<PathBuf, String>::new();
    for (index, part) in source.parts.iter().enumerate() {
        let id = &part.definition.id;
        let instrument_path = super::resolve_reference_path(base, &part.definition.instrument);
        let mut instrument: InstrumentDefinition = read_json(&instrument_path)?;
        let destination = root.join("instruments").join(id);
        fs::create_dir_all(&destination).map_err(|error| {
            io_failure(&destination, "could not create instrument directory", error)
        })?;
        relocate_assets(&mut instrument, &instrument_path, &destination, &mut hashes).map_err(
            |mut error| {
                for diagnostic in &mut error.diagnostics {
                    diagnostic.path = Some(format!(
                        "parts[{index}].instrument.{}",
                        diagnostic.path.as_deref().unwrap_or("assets")
                    ));
                    diagnostic.detail = Some(format!(
                        "part {id}: {}",
                        diagnostic.detail.as_deref().unwrap_or("")
                    ));
                }
                error
            },
        )?;
        write_json(&destination.join("definition.json"), &instrument)?;
        let pattern = format!("patterns/{id}.json");
        let source_pattern = super::resolve_reference_path(base, &part.definition.pattern);
        fs::copy(&source_pattern, root.join(&pattern))
            .map_err(|error| io_failure(&source_pattern, "could not copy pattern", error))?;
        definition["parts"][index]["instrument"] =
            format!("instruments/{id}/definition.json").into();
        definition["parts"][index]["pattern"] = pattern.into();
    }
    Ok(())
}

/// Copies every asset referenced by one instrument into its bundle directory.
///
/// `hashes` carries the SHA-256 of each source across the whole bundle: samplers
/// reference the same file from many zones, and each reference would otherwise copy
/// and hash the whole file again before the duplicate is noticed.
fn relocate_assets(
    definition: &mut InstrumentDefinition,
    source: &Path,
    destination: &Path,
    hashes: &mut HashMap<PathBuf, String>,
) -> Result<(), CliFailure> {
    let base = source.parent().unwrap_or_else(|| Path::new("."));
    let assets = destination.join("assets");
    fs::create_dir_all(&assets)
        .map_err(|error| io_failure(&assets, "could not create assets directory", error))?;
    let mut stored = HashMap::<String, String>::new();
    definition.try_for_each_asset_mut(|field, reference| {
        let path = super::resolve_reference_path(base, Path::new(&reference.path));
        let mut staged = None;
        let hash = if let Some(hash) = hashes.get(&path) {
            hash.clone()
        } else {
            let file = stage_asset(&assets, field, &path)?;
            let hash = hash_file(file.path())?;
            hashes.insert(path.clone(), hash.clone());
            staged = Some(file);
            hash
        };
        if let Some(expected) = &reference.sha256
            && !expected.eq_ignore_ascii_case(&hash)
        {
            return Err(failure(
                DiagnosticCode::AssetHashMismatch,
                "asset sha256 does not match",
                field,
                format!("{}: expected {expected}, actual {hash}", path.display()),
            ));
        }
        let relative = if let Some(relative) = stored.get(&hash) {
            relative.clone()
        } else {
            let name = asset_name(&hash, &path, field)?;
            let file = if let Some(file) = staged {
                file
            } else {
                stage_asset(&assets, field, &path)?
            };
            file.persist_noclobber(assets.join(&name))
                .map_err(|error| io_failure(&path, "could not store asset", error))?;
            let relative = format!("assets/{name}");
            stored.insert(hash.clone(), relative.clone());
            relative
        };
        reference.path = relative;
        reference.sha256 = Some(hash);
        Ok(())
    })
}

/// Copies one source asset next to its instrument so that it can be hashed, and later
/// persisted under its content-addressed name.
fn stage_asset(
    assets: &Path,
    field: &str,
    path: &Path,
) -> Result<tempfile::NamedTempFile, CliFailure> {
    let metadata = fs::metadata(path).map_err(|error| {
        failure(
            DiagnosticCode::AssetNotFound,
            "could not inspect asset",
            field,
            format!("{}: {error}", path.display()),
        )
    })?;
    if !metadata.is_file() {
        return Err(failure(
            DiagnosticCode::AssetNotFound,
            "asset is not a regular file",
            field,
            path.display(),
        ));
    }
    let mut input = File::open(path).map_err(|error| {
        failure(
            DiagnosticCode::AssetNotFound,
            "could not open asset",
            field,
            format!("{}: {error}", path.display()),
        )
    })?;
    let mut staged = tempfile::NamedTempFile::new_in(assets)
        .map_err(|error| io_failure(assets, "could not stage asset", error))?;
    std::io::copy(&mut input, &mut staged).map_err(|error| {
        failure(
            DiagnosticCode::DefinitionError,
            "could not copy asset",
            field,
            format!("{}: {error}", path.display()),
        )
    })?;
    staged
        .flush()
        .map_err(|error| io_failure(staged.path(), "could not flush asset", error))?;
    Ok(staged)
}

fn asset_name(hash: &str, path: &Path, field: &str) -> Result<String, CliFailure> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("");
    if !extension.is_empty()
        && (!valid_path(extension)
            || extension.contains(['/', '<', '>', '"', '|', '?', '*'])
            || extension.chars().any(char::is_control)
            || extension.ends_with(' '))
    {
        return Err(failure(
            DiagnosticCode::DefinitionError,
            "unsafe asset extension",
            field,
            path.display(),
        ));
    }
    Ok(if extension.is_empty() {
        hash.to_owned()
    } else {
        format!("{hash}.{}", extension.to_ascii_lowercase())
    })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, CliFailure> {
    let file = File::open(path)
        .map_err(|error| io_failure(path, "could not read bundle source", error))?;
    serde_json::from_reader(file).map_err(|error| {
        failure(
            DiagnosticCode::JsonInvalid,
            "could not parse bundle JSON",
            path.to_string_lossy(),
            error,
        )
    })
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), CliFailure> {
    let mut file = File::create(path)
        .map_err(|error| io_failure(path, "could not create bundle file", error))?;
    serde_json::to_writer_pretty(&mut file, value)
        .map_err(|error| io_failure(path, "could not write bundle JSON", error))?;
    file.write_all(b"\n")
        .map_err(|error| io_failure(path, "could not finish bundle JSON", error))
}

fn hash_file(path: &Path) -> Result<String, CliFailure> {
    use std::fmt::Write as _;
    let mut file =
        File::open(path).map_err(|error| io_failure(path, "could not hash bundle file", error))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 8 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| io_failure(path, "could not hash bundle file", error))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let mut digest = String::with_capacity(64);
    for byte in hash.finalize() {
        write!(digest, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(digest)
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

fn list_files(root: &Path) -> Result<Vec<ManifestFile>, CliFailure> {
    fn walk(
        root: &Path,
        directory: &Path,
        files: &mut Vec<ManifestFile>,
        names: &mut HashSet<String>,
    ) -> Result<(), CliFailure> {
        for entry in fs::read_dir(directory)
            .map_err(|error| io_failure(directory, "could not list bundle", error))?
        {
            let entry =
                entry.map_err(|error| io_failure(directory, "could not list bundle", error))?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("entry is under root")
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if !valid_path(&relative) || !names.insert(portable_path_key(&relative)) {
                return Err(failure(
                    DiagnosticCode::DefinitionError,
                    "unsafe or duplicate bundle path",
                    &relative,
                    "paths must be safe and unique ignoring case",
                ));
            }
            let kind = entry
                .file_type()
                .map_err(|error| io_failure(&path, "could not inspect bundle file", error))?;
            if kind.is_dir() {
                walk(root, &path, files, names)?;
            } else if kind.is_file() {
                if relative != "bundle.json" {
                    files.push(ManifestFile {
                        path: relative,
                        sha256: hash_file(&path)?,
                    });
                }
            } else {
                return Err(failure(
                    DiagnosticCode::DefinitionError,
                    "bundle contains a symbolic link or special file",
                    &relative,
                    path.display(),
                ));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, root, &mut files, &mut HashSet::new())?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn verify_manifest(root: &Path, demo: &super::DemoDefinition) -> Result<(), CliFailure> {
    let manifest: Manifest = read_json(&root.join("bundle.json"))?;
    let valid_settings = manifest.render_settings.sample_rate > 0
        && manifest.render_settings.block_size > 0
        && manifest.render_settings.tail_seconds.is_finite()
        && manifest.render_settings.tail_seconds >= 0.0;
    let files = list_files(root)?;
    if manifest.format_version != 1
        || manifest.demo != "demo.json"
        || !valid_settings
        || manifest.files != files
    {
        return Err(failure(
            DiagnosticCode::DefinitionError,
            "bundle manifest does not match files",
            "bundle.json",
            "version, settings, paths, order, or SHA-256 mismatch",
        ));
    }
    let expected_render = demo
        .parts
        .iter()
        .map(|part| (part.id.clone(), format!("render/stems/{}.wav", part.id)))
        .collect::<BTreeMap<_, _>>();
    if let Some(render) = manifest.render {
        if render.mix != "render/mix.wav" || render.stems != expected_render {
            return Err(failure(
                DiagnosticCode::DefinitionError,
                "invalid bundle render references",
                "bundle.json.render",
                "mix and every part stem are required",
            ));
        }
        for path in std::iter::once(&render.mix).chain(render.stems.values()) {
            if !files.iter().any(|file| &file.path == path) {
                return Err(failure(
                    DiagnosticCode::DefinitionError,
                    "missing bundle render file",
                    path,
                    "render reference is absent from files",
                ));
            }
        }
    } else if files.iter().any(|file| file.path.starts_with("render/")) {
        return Err(failure(
            DiagnosticCode::DefinitionError,
            "unexpected bundle render file",
            "bundle.json.render",
            "render must describe included WAVs",
        ));
    }
    Ok(())
}

fn reject_existing_output(output: &Path) -> Result<(), CliFailure> {
    match fs::symlink_metadata(output) {
        Ok(_) => Err(failure(
            DiagnosticCode::BundleOutputExists,
            "bundle output already exists",
            output.to_string_lossy(),
            "choose a new output directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_failure(output, "could not inspect bundle output", error)),
    }
}

/// Commits the staged bundle by renaming it, without ever taking over a name that
/// something else owns.
#[cfg(unix)]
fn commit_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(Into::into)
}

/// Commits the staged bundle by renaming it, without ever taking over a name that
/// something else owns.
#[cfg(windows)]
fn commit_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    let source = OwnedHandle::open_directory(source)?;
    let request = rename_request(destination)?;
    let size = byte_length(request.len())?;
    // SAFETY: `source` is a directory handle opened for renaming and `request` is a
    // `FILE_RENAME_INFO` header followed by the file name it announces.
    let renamed = unsafe {
        SetFileInformationByHandle(source.0, FileRenameInfoEx, request.as_ptr().cast(), size)
    };
    if renamed == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Encodes `FileRenameInformationEx` without `FILE_RENAME_FLAG_REPLACE_IF_EXISTS`, so
/// the kernel rejects the rename as soon as any file or directory already owns the
/// destination name. That is the guarantee `fs::rename` and `MoveFileEx` lack: both
/// replace an existing empty directory instead of failing. The information class
/// needs Windows 10 1709 or newer.
#[cfg(windows)]
fn rename_request(destination: &Path) -> std::io::Result<Vec<u8>> {
    let name: Vec<u8> = std::path::absolute(destination)?
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_ne_bytes)
        .collect();
    let header = FILE_RENAME_INFO {
        Anonymous: FILE_RENAME_INFO_0 { Flags: 0 },
        RootDirectory: std::ptr::null_mut(),
        FileNameLength: byte_length(name.len())?,
        FileName: [0],
    };
    let minimum = std::mem::size_of::<FILE_RENAME_INFO>();
    let mut request = Vec::with_capacity(minimum + name.len());
    // SAFETY: `header` is a live `FILE_RENAME_INFO` and the prefix stops at `FileName`.
    request.extend_from_slice(unsafe {
        std::slice::from_raw_parts(
            std::ptr::from_ref(&header).cast::<u8>(),
            std::mem::offset_of!(FILE_RENAME_INFO, FileName),
        )
    });
    request.extend_from_slice(&name);
    // The structure is padded to its own size after the name it carries.
    request.resize(request.len().max(minimum), 0);
    Ok(request)
}

#[cfg(windows)]
fn byte_length(bytes: usize) -> std::io::Result<u32> {
    u32::try_from(bytes)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path is too long"))
}

/// A directory handle that `SetFileInformationByHandle` can rename.
#[cfg(windows)]
struct OwnedHandle(HANDLE);

#[cfg(windows)]
impl OwnedHandle {
    fn open_directory(path: &Path) -> std::io::Result<Self> {
        // `CreateFileW` needs a null-terminated wide string.
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: `path` is null-terminated and the remaining arguments request an
        // existing directory opened only for renaming.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                DELETE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self(handle))
    }
}

#[cfg(windows)]
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the handle is opened once and closed once, by this drop.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn io_failure(path: &Path, message: &str, error: impl std::fmt::Display) -> CliFailure {
    failure(
        DiagnosticCode::DefinitionError,
        message,
        path.to_string_lossy(),
        error,
    )
}

fn failure(
    code: DiagnosticCode,
    message: &str,
    path: impl Into<String>,
    detail: impl std::fmt::Display,
) -> CliFailure {
    CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(code, message)
                .with_path(path)
                .with_detail(detail.to_string()),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn manifest_rejects_corruption_unsafe_paths_and_unlisted_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let demo: super::super::DemoDefinition = serde_json::from_value(json!({
            "schema_version": 1,
            "parts": [{"id":"lead","instrument":"instruments/lead/definition.json","pattern":"patterns/lead.json"}]
        })).unwrap();
        write_json(&root.join("demo.json"), &demo).unwrap();
        let manifest = Manifest {
            format_version: 1,
            demo: "demo.json".to_owned(),
            render_settings: RenderSettings {
                sample_rate: 48000,
                block_size: 257,
                tail_seconds: 1.0,
            },
            render: None,
            files: list_files(root).unwrap(),
        };
        let original = serde_json::to_value(&manifest).unwrap();
        write_json(&root.join("bundle.json"), &original).unwrap();
        assert!(verify_manifest(root, &demo).is_ok());

        for case in [
            "version",
            "duplicate",
            "hash",
            "unsafe",
            "missing",
            "unlisted",
            "render",
        ] {
            let mut changed = original.clone();
            match case {
                "version" => changed["format_version"] = json!(2),
                "duplicate" => {
                    let file = changed["files"][0].clone();
                    changed["files"].as_array_mut().unwrap().push(file);
                }
                "hash" => changed["files"][0]["sha256"] = json!("0".repeat(64)),
                "unsafe" => changed["files"][0]["path"] = json!("../demo.json"),
                "missing" => changed["files"] = json!([]),
                "unlisted" => fs::write(root.join("unlisted.txt"), b"extra").unwrap(),
                "render" => changed["render"] = json!({"mix":"render/mix.wav","stems":{}}),
                _ => unreachable!(),
            }
            write_json(&root.join("bundle.json"), &changed).unwrap();

            assert!(verify_manifest(root, &demo).is_err(), "{case}");

            if case == "unlisted" {
                fs::remove_file(root.join("unlisted.txt")).unwrap();
            }
        }
        write_json(&root.join("bundle.json"), &original).unwrap();
        fs::write(root.join("demo.json"), b"changed").unwrap();
        assert!(verify_manifest(root, &demo).is_err());
    }

    #[test]
    fn committing_a_bundle_never_replaces_an_existing_destination() {
        let parent = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(parent.path()).unwrap();
        fs::write(staging.path().join("demo.json"), b"staged").unwrap();
        let destination = parent.path().join("bundle");
        // The destination may appear after the initial CLI check.
        fs::create_dir(&destination).unwrap();

        assert!(commit_directory(staging.path(), &destination).is_err());

        assert!(destination.is_dir());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        assert_eq!(
            fs::read(staging.path().join("demo.json")).unwrap(),
            b"staged"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bundle_file_inventory_rejects_symbolic_links() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("source.wav"), b"audio").unwrap();
        std::os::unix::fs::symlink("source.wav", directory.path().join("linked.wav")).unwrap();

        assert!(list_files(directory.path()).is_err());
    }
}
