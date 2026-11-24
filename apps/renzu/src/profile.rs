//! Trace-to-profile correlation for saved experiment reports.
//!
//! Reads [`RunReport`]s via [`crate::report::read_report`] and correlates
//! `perf script` profile samples against a run's time window
//! (`start_time` + `duration_secs`).
//!
//! # Correlation model
//!
//! Correlation is by time-window overlap only. `perf script` sample
//! timestamps are relative to perf recording start; the caller supplies
//! `--perf-offset-seconds` to anchor perf's time base to wall-clock time.
//! Samples whose absolute timestamp falls within
//! [`start_time`, `start_time + duration_secs`] are attributed to the run.
//!
//! # Limitations
//!
//! This is temporal correlation, not per-span CPU attribution. A hot
//! function observed during a slow run does not imply that function caused
//! any particular span's slowness. Per-span attribution is explicitly not
//! supported and not claimed.

use crate::report::RunReport;
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

/// A parsed `perf script` sample line.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProfileSample {
    /// Process name that owned the sample.
    pub comm: String,
    /// Absolute timestamp of the sample, relative to perf recording start.
    pub timestamp_secs: f64,
    /// CPU on which the sample was taken.
    pub cpu: u32,
    /// Symbol / function name associated with the sample.
    pub function: String,
}

/// Result of correlating a `RunReport` with profile samples.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CorrelationResult {
    /// Run identifier.
    pub run_id: String,
    /// Run start time as RFC 3339.
    pub start_time: String,
    /// Run duration in seconds.
    pub duration_secs: u64,
    /// Number of samples falling within the run's time window.
    pub samples_in_window: u64,
    /// Top functions by sample count, sorted descending.
    pub top_functions: Vec<(String, u64)>,
}

/// Parse `perf script` output lines into [`ProfileSample`] entries.
///
/// Lines are expected in the default `perf script` format:
///   comm pid/tid timestamp cpu event symbol ...
///
/// Malformed lines are skipped silently.
pub fn parse_perf_script(lines: &[&str]) -> io::Result<Vec<ProfileSample>> {
    let mut samples = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let fields: Vec<&str> = trimmed.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }
        let comm = fields[0].to_string();
        // fields[1] is pid/tid, ignored.
        let timestamp_secs = match fields[2].parse::<f64>() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let cpu = match fields[3].parse::<u32>() {
            Ok(c) => c,
            Err(_) => continue,
        };
        // fields[4] is the event name; the symbol follows on later fields.
        let function = fields
            .get(5..)
            .map(|rest| rest.join(" "))
            .unwrap_or_else(|| "<unknown>".to_string());
        samples.push(ProfileSample {
            comm,
            timestamp_secs,
            cpu,
            function,
        });
    }
    Ok(samples)
}

/// Correlate profile samples with a single run's time window.
///
/// `offset_seconds` is the seconds from perf recording start to run start
/// (negative if perf started before the run). Each sample's `timestamp_secs`
/// (perf-relative) plus `offset_seconds` gives seconds into the run; samples
/// with `0 <= relative <= duration_secs` fall inside the window.
pub fn correlate_profile(
    run_id: &str,
    report: &RunReport,
    perf_samples: &[ProfileSample],
    offset_seconds: f64,
    top_n: usize,
) -> CorrelationResult {
    let duration = report.duration_secs as f64;

    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut samples_in_window: u64 = 0;

    for sample in perf_samples {
        let relative = sample.timestamp_secs + offset_seconds;
        if relative >= 0.0 && relative <= duration {
            samples_in_window += 1;
            *counts.entry(sample.function.clone()).or_insert(0) += 1;
        }
    }

    let mut top_functions: Vec<(String, u64)> = counts.into_iter().collect();
    top_functions.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    top_functions.truncate(top_n);

    CorrelationResult {
        run_id: run_id.to_string(),
        start_time: report.start_time.clone(),
        duration_secs: report.duration_secs,
        samples_in_window,
        top_functions,
    }
}

/// Read a saved report and correlate it with profile samples read from a file.
pub fn correlate_profile_for_run(
    run_id: &str,
    perf_script_path: &PathBuf,
    offset_seconds: f64,
    top_n: usize,
) -> io::Result<CorrelationResult> {
    let report = crate::report::read_report(run_id)?;
    let contents = std::fs::read_to_string(perf_script_path)?;
    let lines: Vec<&str> = contents.lines().collect();
    let samples = parse_perf_script(&lines)?;
    Ok(correlate_profile(
        run_id,
        &report,
        &samples,
        offset_seconds,
        top_n,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadgen::CompletionStatus;
    use crate::report::{CountsReport, LatencyReport, RunReport, WorkloadReport};

    fn make_report(start_time: &str, duration_secs: u64) -> RunReport {
        RunReport {
            run_id: "rt-1".to_string(),
            scenario: "baseline".to_string(),
            start_time: start_time.to_string(),
            duration_secs,
            completion_status: CompletionStatus::Complete,
            workload: WorkloadReport {
                scenario: "baseline".to_string(),
                duration_secs,
                kind: "rate".to_string(),
                value: 100,
                url: "http://127.0.0.1:8080/health".to_string(),
            },
            counts: CountsReport {
                total: 100,
                successful: 98,
                failed: 2,
            },
            latency: LatencyReport {
                count: 100,
                min_ms: 1.0,
                max_ms: 100.0,
                mean_ms: 50.0,
                p50_ms: 40.0,
                p95_ms: 90.0,
                p99_ms: 99.0,
            },
            throughput_rps: 1.67,
            telemetry_references: vec![],
            warnings: vec![],
            schema_version: 1,
        }
    }

    fn sample(ts: f64, function: &str) -> ProfileSample {
        ProfileSample {
            comm: "gateway".to_string(),
            timestamp_secs: ts,
            cpu: 0,
            function: function.to_string(),
        }
    }

    #[test]
    fn samples_inside_window_are_counted_and_sorted() {
        let report = make_report("2026-11-24T10:00:00Z", 60);
        let samples = vec![
            sample(10.0, "axum::handler"),
            sample(20.0, "axum::handler"),
            sample(30.0, "tokio::spawn"),
            sample(40.0, "tokio::spawn"),
            sample(50.0, "tokio::spawn"),
        ];
        // run starts at epoch ~1795485600; perf offset 0 means perf started at run start.
        let result = correlate_profile("rt-1", &report, &samples, 0.0, 10);
        assert_eq!(result.samples_in_window, 5);
        assert_eq!(result.top_functions.len(), 2);
        assert_eq!(result.top_functions[0], ("tokio::spawn".to_string(), 3));
        assert_eq!(result.top_functions[1], ("axum::handler".to_string(), 2));
    }

    #[test]
    fn samples_outside_window_are_excluded() {
        let report = make_report("2026-11-24T10:00:00Z", 60);
        // 300 seconds (5 min) before and after the run.
        let samples = vec![sample(-300.0, "old_fn"), sample(400.0, "new_fn")];
        let result = correlate_profile("rt-1", &report, &samples, 0.0, 10);
        assert_eq!(result.samples_in_window, 0);
        assert!(result.top_functions.is_empty());
    }

    #[test]
    fn boundary_samples_are_included() {
        let report = make_report("2026-11-24T10:00:00Z", 60);
        let samples = vec![sample(0.0, "start_fn"), sample(60.0, "end_fn")];
        let result = correlate_profile("rt-1", &report, &samples, 0.0, 10);
        assert_eq!(result.samples_in_window, 2);
        assert_eq!(result.top_functions[0], ("end_fn".to_string(), 1));
        assert_eq!(result.top_functions[1], ("start_fn".to_string(), 1));
    }

    #[test]
    fn negative_offset_shifts_samples_into_window() {
        let report = make_report("2026-11-24T10:00:00Z", 60);
        // perf started 100 seconds before the run; sample at 150s perf time
        // maps to 150 - 100 = 50s into the run (offset -100).
        let samples = vec![sample(150.0, "pre_fn")];
        let result = correlate_profile("rt-1", &report, &samples, -100.0, 10);
        assert_eq!(result.samples_in_window, 1);
        assert_eq!(result.top_functions[0], ("pre_fn".to_string(), 1));
    }

    #[test]
    fn empty_perf_script_yields_empty_result() {
        let report = make_report("2026-11-24T10:00:00Z", 60);
        let result = correlate_profile("rt-1", &report, &[], 0.0, 10);
        assert_eq!(result.samples_in_window, 0);
        assert!(result.top_functions.is_empty());
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let lines = vec![
            "garbage line with no numbers",
            "comm 123 notanumber cpu event fn",
            "comm 123 10.0 cpu event fn",
        ];
        let samples = parse_perf_script(&lines).unwrap();
        // Only the third line parses (timestamp 10.0 is a valid f64, but cpu "cpu" is not u32).
        assert_eq!(samples.len(), 0);
    }

    #[test]
    fn top_n_limit_is_respected() {
        let report = make_report("2026-11-24T10:00:00Z", 60);
        let samples = vec![
            sample(10.0, "a"),
            sample(11.0, "a"),
            sample(12.0, "b"),
            sample(13.0, "b"),
            sample(14.0, "b"),
            sample(15.0, "c"),
        ];
        let result = correlate_profile("rt-1", &report, &samples, 0.0, 2);
        assert_eq!(result.top_functions.len(), 2);
        assert_eq!(result.top_functions[0], ("b".to_string(), 3));
        assert_eq!(result.top_functions[1], ("a".to_string(), 2));
    }

    #[test]
    fn offset_applied_correctly_positive() {
        let report = make_report("2026-11-24T10:00:00Z", 60);
        // perf started 5 seconds after the run; sample at perf time 10s
        // maps to absolute run_time + 5 + 10 = run_start + 15.
        let samples = vec![sample(10.0, "late_fn")];
        let result = correlate_profile("rt-1", &report, &samples, 5.0, 10);
        assert_eq!(result.samples_in_window, 1);
    }

    #[test]
    fn parse_perf_script_skips_header_and_footer() {
        let lines = vec![
            "# perf script output header",
            "gateway 1234/5678 10.500 3 cpu-clock axum::handler",
            "# some comment",
            "",
            "orders 2345/6789 20.250 1 cpu-clock tokio::spawn",
        ];
        let samples = parse_perf_script(&lines).unwrap();
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].function, "axum::handler");
        assert_eq!(samples[1].function, "tokio::spawn");
    }
}
