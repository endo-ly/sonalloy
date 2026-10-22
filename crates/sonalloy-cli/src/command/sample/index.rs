use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sonalloy_core::{AudioAnalysis, AudioFileMetadata, Diagnostic, read_audio_file_metadata};

use super::{IndexArgs, analyze_file, diagnostic, failure, normalized, path_text, walk};
use crate::output::CliFailure;

pub(super) const ANALYSIS_REVISION: u32 = 1;
const FORMAT: &str = "sonalloy.sample-index";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModifiedTime {
    seconds: i64,
    nanoseconds: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub(super) relative_path: String,
    pub(super) size_bytes: u64,
    pub(super) modified_time: ModifiedTime,
    pub(super) audio: Option<AudioFileMetadata>,
    pub(super) analysis_revision: Option<u32>,
    pub(super) analysis: Option<AudioAnalysis>,
    pub(super) diagnostics: Vec<Diagnostic>,
}

impl Record {
    pub(super) fn analysis_status(&self) -> &'static str {
        if !self.diagnostics.is_empty() {
            "failed"
        } else if self.analysis.is_none() {
            "unanalyzed"
        } else if self.analysis_revision != Some(ANALYSIS_REVISION) {
            "stale"
        } else {
            "analyzed"
        }
    }

    pub(super) fn matches_file(&self, path: &Path) -> bool {
        fingerprint(path)
            .is_ok_and(|(size, modified)| size == self.size_bytes && modified == self.modified_time)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Index {
    pub(super) format: String,
    pub(super) source_root: PathBuf,
    pub(super) files: Vec<Record>,
}

pub(super) fn read(path: &Path) -> Result<Index, CliFailure> {
    let bytes = std::fs::read(path).map_err(|error| failure(path, error.to_string()))?;
    let index: Index = serde_json::from_slice(&bytes)
        .map_err(|error| failure(path, format!("invalid sample index: {error}")))?;
    if index.format != FORMAT || !index.source_root.is_absolute() {
        return Err(failure(path, "invalid sample index format or source_root"));
    }
    let mut seen = BTreeSet::new();
    for record in &index.files {
        let relative = Path::new(&record.relative_path);
        if record.relative_path.is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || !seen.insert(&record.relative_path)
            || record.modified_time.nanoseconds >= 1_000_000_000
        {
            return Err(failure(
                path,
                format!(
                    "invalid or duplicate relative_path or timestamp: {}",
                    record.relative_path
                ),
            ));
        }
        if let Some(audio) = &record.audio {
            if audio.sample_rate == 0
                || audio.channels == 0
                || audio
                    .duration_seconds
                    .is_some_and(|v| !v.is_finite() || v < 0.0)
            {
                return Err(failure(
                    path,
                    format!("invalid audio metadata: {}", record.relative_path),
                ));
            }
        }
        if let Some(analysis) = &record.analysis {
            if !analysis.finite
                || record.audio.as_ref().is_none_or(|audio| {
                    audio.sample_rate != analysis.sample_rate
                        || audio.channels != analysis.channels
                        || audio.frames.is_some_and(|frames| {
                            u64::try_from(analysis.frames).ok() != Some(frames)
                        })
                })
            {
                return Err(failure(
                    path,
                    format!(
                        "analysis does not match audio metadata: {}",
                        record.relative_path
                    ),
                ));
            }
        }
    }
    Ok(index)
}

#[derive(Serialize)]
pub(super) struct Report {
    status: &'static str,
    index: String,
    source_root: String,
    total_files: usize,
    added: usize,
    updated: usize,
    unchanged: usize,
    removed: usize,
    analyzed: usize,
    pub(super) failed: usize,
    diagnostics: Vec<Diagnostic>,
}

pub(super) fn run(args: &IndexArgs) -> Result<Report, CliFailure> {
    let root = normalized(&args.directory)?;
    if !root.is_dir() {
        return Err(failure(&root, "source_root must be a directory"));
    }
    let mut old = previous_records(&args.output, &root)?;
    let mut paths = BTreeSet::new();
    walk(&root, &mut paths)?;
    if args.output.exists() && paths.contains(&normalized(&args.output)?) {
        return Err(failure(
            &args.output,
            "index output must not replace a source WAV",
        ));
    }
    let mut report = Report {
        status: "ok",
        index: path_text(&args.output)?,
        source_root: path_text(&root)?,
        total_files: paths.len(),
        added: 0,
        updated: 0,
        unchanged: 0,
        removed: 0,
        analyzed: 0,
        failed: 0,
        diagnostics: Vec::new(),
    };
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let relative = path
            .strip_prefix(&root)
            .map_err(|error| failure(&path, error.to_string()))?;
        let relative_path = path_text(relative)?;
        let (size_bytes, modified_time) = fingerprint(&path)?;
        let previous = old.remove(&relative_path);
        let changed = previous.as_ref().is_none_or(|record| {
            record.size_bytes != size_bytes || record.modified_time != modified_time
        });
        if previous.is_none() {
            report.added += 1;
        } else if changed || args.force {
            report.updated += 1;
        } else {
            report.unchanged += 1;
        }
        let refresh = changed
            || args.force
            || previous
                .as_ref()
                .is_some_and(|record| record.audio.is_none());
        let mut record = if refresh {
            let (audio, diagnostics) = match read_audio_file_metadata(&path) {
                Ok(audio) => (Some(audio), Vec::new()),
                Err(error) => (None, vec![diagnostic(&path, error.to_string())]),
            };
            Record {
                relative_path,
                size_bytes,
                modified_time,
                audio,
                analysis_revision: None,
                analysis: None,
                diagnostics,
            }
        } else {
            previous.expect("unchanged record exists")
        };
        if args.analyze && analyze_record(&path, &mut record) {
            report.analyzed += 1;
        }
        // A concurrent writer must not attach measurements to a different file fingerprint.
        if fingerprint(&path)? != (record.size_bytes, record.modified_time.clone()) {
            return Err(failure(
                &path,
                "WAV changed while indexing; retry the index operation",
            ));
        }
        if !record.diagnostics.is_empty() {
            report.failed += 1;
            report.diagnostics.extend(record.diagnostics.clone());
        }
        files.push(record);
    }
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    report.removed = old.len();
    report.status = if report.failed == 0 {
        "ok"
    } else if report.failed == report.total_files {
        "error"
    } else {
        "partial"
    };
    save(
        &args.output,
        &Index {
            format: FORMAT.to_owned(),
            source_root: root,
            files,
        },
    )?;
    Ok(report)
}

fn previous_records(output: &Path, root: &Path) -> Result<BTreeMap<String, Record>, CliFailure> {
    let old = if output.exists() {
        let old = read(output)?;
        if normalized(&old.source_root)? != root {
            return Err(failure(output, "index belongs to a different source_root"));
        }
        old.files
    } else {
        Vec::new()
    };
    let old = old
        .into_iter()
        .map(|record| (record.relative_path.clone(), record))
        .collect();
    Ok(old)
}

fn analyze_record(path: &Path, record: &mut Record) -> bool {
    if record.analysis_status() != "analyzed" {
        record.analysis = None;
        record.analysis_revision = None;
        // Retry metadata as well when a previous attempt could not open the WAV.
        if record.audio.is_none() {
            record.audio = read_audio_file_metadata(path).ok();
        }
        match analyze_file(path, None) {
            Ok(analysis) => {
                let bits = record
                    .audio
                    .as_ref()
                    .and_then(|audio| audio.bits_per_sample);
                record.audio = Some(AudioFileMetadata {
                    sample_rate: analysis.sample_rate,
                    channels: analysis.channels,
                    bits_per_sample: bits,
                    frames: u64::try_from(analysis.frames).ok(),
                    duration_seconds: Some(analysis.duration_seconds),
                });
                record.analysis = Some(analysis);
                record.analysis_revision = Some(ANALYSIS_REVISION);
                record.diagnostics.clear();
                return true;
            }
            Err(error) => {
                record.diagnostics = vec![error];
            }
        }
    }
    false
}

fn fingerprint(path: &Path) -> Result<(u64, ModifiedTime), CliFailure> {
    let metadata = std::fs::metadata(path).map_err(|error| failure(path, error.to_string()))?;
    let time = metadata
        .modified()
        .map_err(|error| failure(path, error.to_string()))?;
    Ok((
        metadata.len(),
        modified_time(time).ok_or_else(|| failure(path, "modification time is out of range"))?,
    ))
}

fn modified_time(time: SystemTime) -> Option<ModifiedTime> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(time) => Some(ModifiedTime {
            seconds: i64::try_from(time.as_secs()).ok()?,
            nanoseconds: time.subsec_nanos(),
        }),
        Err(error) => {
            let time = error.duration();
            let seconds = i64::try_from(time.as_secs()).ok()?.checked_neg()?;
            let nanos = time.subsec_nanos();
            Some(ModifiedTime {
                seconds: if nanos == 0 {
                    seconds
                } else {
                    seconds.checked_sub(1)?
                },
                nanoseconds: if nanos == 0 { 0 } else { 1_000_000_000 - nanos },
            })
        }
    }
}

fn save(path: &Path, index: &Index) -> Result<(), CliFailure> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut pending = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| failure(path, error.to_string()))?;
    serde_json::to_writer_pretty(&mut pending, index)
        .map_err(|error| failure(path, error.to_string()))?;
    pending
        .write_all(b"\n")
        .and_then(|()| pending.as_file().sync_all())
        .map_err(|error| failure(path, error.to_string()))?;
    // Validate the persisted representation before replacing the old index.
    read(pending.path())?;
    pending
        .persist(path)
        .map_err(|error| failure(path, error.to_string()))?;
    Ok(())
}
