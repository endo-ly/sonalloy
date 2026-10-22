use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use clap::Args;
use serde::Serialize;
use sonalloy_core::{AudioAnalysis, AudioFileMetadata, Diagnostic};

use super::index::{self, Record};
use super::{failure, normalized, path_text};
use crate::output::CliFailure;

#[derive(Debug, Args)]
pub(in crate::command) struct SearchArgs {
    #[arg(value_name = "INDEX", required = true, num_args = 1..)]
    indexes: Vec<PathBuf>,
    #[arg(long)]
    query: Option<String>,
    #[arg(long, value_parser = super::super::parse_nonnegative_f64)]
    min_duration: Option<f64>,
    #[arg(long, value_parser = super::super::parse_nonnegative_f64)]
    max_duration: Option<f64>,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    sample_rate: Option<u32>,
    #[arg(long, value_parser = super::super::parse_positive_usize)]
    channels: Option<usize>,
    #[arg(long, allow_hyphen_values = true, value_parser = finite_number)]
    min_peak_dbfs: Option<f64>,
    #[arg(long, allow_hyphen_values = true, value_parser = finite_number)]
    max_peak_dbfs: Option<f64>,
    #[arg(long, allow_hyphen_values = true, value_parser = finite_number)]
    min_rms_dbfs: Option<f64>,
    #[arg(long, allow_hyphen_values = true, value_parser = finite_number)]
    max_rms_dbfs: Option<f64>,
    #[arg(long, value_parser = super::super::parse_nonnegative_f64)]
    min_centroid_hz: Option<f64>,
    #[arg(long, value_parser = super::super::parse_nonnegative_f64)]
    max_centroid_hz: Option<f64>,
    #[arg(long, default_value_t = 100)]
    limit: usize,
    #[arg(long, default_value_t = 0)]
    offset: usize,
    #[arg(long)]
    pub(super) json: bool,
}

fn finite_number(value: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| "expected a finite number".to_owned())
}

#[derive(Serialize)]
struct SearchResult {
    path: String,
    source_index: String,
    relative_path: String,
    audio: Option<AudioFileMetadata>,
    analysis_status: &'static str,
    analysis: Option<AudioAnalysis>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Serialize)]
pub(super) struct Report {
    status: &'static str,
    total_files: usize,
    evaluated: usize,
    total_matches: usize,
    returned: usize,
    unevaluated: usize,
    complete: bool,
    results: Vec<SearchResult>,
    diagnostics: Vec<Diagnostic>,
}

struct Candidate {
    path: PathBuf,
    source: PathBuf,
    record: Record,
}

pub(super) fn run(args: &SearchArgs) -> Result<Report, CliFailure> {
    for (name, min, max) in [
        ("duration", args.min_duration, args.max_duration),
        ("peak-dbfs", args.min_peak_dbfs, args.max_peak_dbfs),
        ("rms-dbfs", args.min_rms_dbfs, args.max_rms_dbfs),
        ("centroid-hz", args.min_centroid_hz, args.max_centroid_hz),
    ] {
        if min.zip(max).is_some_and(|(min, max)| min > max) {
            return Err(failure(
                Path::new(""),
                format!("--min-{name} must be less than or equal to --max-{name}"),
            ));
        }
    }
    let sources = args
        .indexes
        .iter()
        .map(|path| normalized(path))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut candidates: BTreeMap<PathBuf, Vec<Candidate>> = BTreeMap::new();
    for source in sources {
        let index = index::read(&source)?;
        for record in index.files {
            let path = index.source_root.join(&record.relative_path);
            // Missing files remain searchable using their last indexed absolute path.
            let path = if path.exists() {
                normalized(&path)?
            } else {
                path
            };
            let candidate = Candidate {
                path: path.clone(),
                source: source.clone(),
                record,
            };
            candidates.entry(path).or_default().push(candidate);
        }
    }
    let mut candidates = candidates
        .into_values()
        .map(merge)
        .collect::<Result<Vec<_>, _>>()?;
    candidates.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then_with(|| a.record.relative_path.cmp(&b.record.relative_path))
    });
    let mut report = Report {
        status: "ok",
        total_files: candidates.len(),
        evaluated: 0,
        total_matches: 0,
        returned: 0,
        unevaluated: 0,
        complete: true,
        results: Vec::new(),
        diagnostics: Vec::new(),
    };
    let query = args.query.as_ref().map(|q| q.to_lowercase());
    for candidate in candidates {
        let record = candidate.record;
        match matches(&record, args, query.as_deref()) {
            None => {
                report.unevaluated += 1;
            }
            Some(matched) => {
                report.evaluated += 1;
                if !matched {
                    continue;
                }
                let ordinal = report.total_matches;
                report.total_matches += 1;
                if ordinal >= args.offset && report.results.len() < args.limit {
                    report.results.push(SearchResult {
                        path: path_text(&candidate.path)?,
                        source_index: path_text(&candidate.source)?,
                        analysis_status: record.analysis_status(),
                        relative_path: record.relative_path,
                        audio: record.audio,
                        analysis: record.analysis,
                        diagnostics: record.diagnostics,
                    });
                }
            }
        }
    }
    report.complete = report.unevaluated == 0;
    report.returned = report.results.len();
    Ok(report)
}

fn merge(candidates: Vec<Candidate>) -> Result<Candidate, CliFailure> {
    let rank = |candidate: &Candidate| {
        (
            candidate.record.matches_file(&candidate.path),
            candidate.record.analysis_status() == "analyzed",
            candidate.record.audio.is_some(),
        )
    };
    let mut candidates = candidates.into_iter();
    let mut best = candidates.next().expect("one or more records per WAV");
    let mut best_rank = rank(&best);
    let mut conflict = None;
    for candidate in candidates {
        let candidate_rank = rank(&candidate);
        if candidate_rank > best_rank {
            best_rank = candidate_rank;
            best = candidate;
            conflict = None;
        } else if candidate_rank == best_rank && !equivalent(&best.record, &candidate.record) {
            conflict = Some(candidate.source);
        }
    }
    if let Some(source) = conflict {
        return Err(failure(
            &best.path,
            format!(
                "conflicting records in {} and {}",
                best.source.display(),
                source.display()
            ),
        ));
    }
    Ok(best)
}

fn equivalent(a: &Record, b: &Record) -> bool {
    a.size_bytes == b.size_bytes
        && a.modified_time == b.modified_time
        && a.audio == b.audio
        && a.analysis_revision == b.analysis_revision
        && a.analysis == b.analysis
        && a.diagnostics == b.diagnostics
}

fn matches(record: &Record, args: &SearchArgs, query: Option<&str>) -> Option<bool> {
    if query.is_some_and(|query| !record.relative_path.to_lowercase().contains(query)) {
        return Some(false);
    }
    let mut conditions = Vec::new();
    if let Some(rate) = args.sample_rate {
        conditions.push(record.audio.as_ref().map(|a| a.sample_rate == rate));
    }
    if let Some(channels) = args.channels {
        conditions.push(record.audio.as_ref().map(|a| a.channels == channels));
    }
    conditions.push(range(
        record.audio.as_ref().and_then(|a| a.duration_seconds),
        args.min_duration,
        args.max_duration,
    ));
    let analysis = (record.analysis_status() == "analyzed")
        .then_some(record.analysis.as_ref())
        .flatten();
    conditions.push(acoustic_range(
        analysis,
        |a| a.level.peak_dbfs,
        args.min_peak_dbfs,
        args.max_peak_dbfs,
    ));
    conditions.push(acoustic_range(
        analysis,
        |a| a.level.rms_dbfs,
        args.min_rms_dbfs,
        args.max_rms_dbfs,
    ));
    conditions.push(acoustic_range(
        analysis,
        |a| a.spectrum.spectral_centroid_hz,
        args.min_centroid_hz,
        args.max_centroid_hz,
    ));
    if conditions.contains(&Some(false)) {
        Some(false)
    } else if conditions.contains(&None) {
        None
    } else {
        Some(true)
    }
}

fn range(value: Option<f64>, min: Option<f64>, max: Option<f64>) -> Option<bool> {
    if min.is_none() && max.is_none() {
        return Some(true);
    }
    value.map(|value| within(value, min, max))
}

fn acoustic_range(
    analysis: Option<&AudioAnalysis>,
    metric: impl FnOnce(&AudioAnalysis) -> Option<f32>,
    min: Option<f64>,
    max: Option<f64>,
) -> Option<bool> {
    if min.is_none() && max.is_none() {
        return Some(true);
    }
    analysis
        .map(|analysis| metric(analysis).is_some_and(|value| within(f64::from(value), min, max)))
}

fn within(value: f64, min: Option<f64>, max: Option<f64>) -> bool {
    min.is_none_or(|min| value >= min) && max.is_none_or(|max| value <= max)
}
