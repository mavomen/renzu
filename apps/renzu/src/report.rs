//! Experiment reports for `renzu load-test` and `renzu report`.
//!
//! Every load-test run produces a local JSON report keyed by a unique run ID,
//! written to `./reports/` (overridable via the `RENZU_REPORTS_DIR`
//! environment variable) so it can be inspected without a running CLI
//! process. Reports follow PRD §FR-07 and ARCHITECTURE §3.6.

use crate::loadgen::{CompletionStatus, LoadTestResult, Workload};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Reports directory: `RENZU_REPORTS_DIR` or `./reports`.
const REPORTS_DIR_ENV: &str = "RENZU_REPORTS_DIR";
const REPORTS_DIR_DEFAULT: &str = "reports";

/// Unique run identifier: `<scenario>-<epoch-ms>`.
pub fn generate_run_id(scenario: &str) -> String {
    let epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{scenario}-{epoch_ms}")
}

/// Resolve the reports directory.
pub fn reports_dir() -> PathBuf {
    std::env::var(REPORTS_DIR_ENV)
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(REPORTS_DIR_DEFAULT))
}

/// Workload configuration recorded in a report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadReport {
    pub scenario: String,
    pub duration_secs: u64,
    pub kind: String,
    pub value: u64,
    pub url: String,
}

/// Request success/failure counts recorded in a report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountsReport {
    pub total: u64,
    pub successful: u64,
    pub failed: u64,
}

/// Latency statistics recorded in a report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyReport {
    pub count: usize,
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

/// Diagnostic report for a single experiment run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub run_id: String,
    pub scenario: String,
    pub start_time: String,
    pub duration_secs: u64,
    pub completion_status: CompletionStatus,
    pub workload: WorkloadReport,
    pub counts: CountsReport,
    pub latency: LatencyReport,
    pub throughput_rps: f64,
    pub telemetry_references: Vec<String>,
    pub warnings: Vec<String>,
}

/// Current wall-clock time as an RFC 3339 UTC string (`2026-11-01T16:31:56Z`).
pub fn now_rfc3339() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let mut remaining = duration.as_secs();
    let sec = (remaining % 60) as u8;
    remaining /= 60;
    let min = (remaining % 60) as u8;
    remaining /= 60;
    let hour = (remaining % 24) as u8;
    let mut days = (remaining / 24) as u32;

    let mut year = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let month = month_from_day_of_year(days, is_leap_year(year));
    let day = (days - day_of_year_start(month, is_leap_year(year)) + 1) as u8;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && !year.is_multiple_of(100) || year.is_multiple_of(400)
}

const CUMULATIVE_DAYS: [u32; 13] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365];

fn month_from_day_of_year(day_of_year: u32, leap: bool) -> u32 {
    let mut month = 1;
    let mut next = CUMULATIVE_DAYS[1];
    if leap && day_of_year > 59 {
        next += 1;
    }
    while month < 12 && day_of_year >= next {
        month += 1;
        next = CUMULATIVE_DAYS[month as usize];
        if leap && month > 2 {
            next += 1;
        }
    }
    month
}

/// First day-of-year (0-indexed) of the given month (1-indexed).
fn day_of_year_start(month: u32, leap: bool) -> u32 {
    let mut start = CUMULATIVE_DAYS[(month - 1) as usize];
    if leap && month > 2 {
        start += 1;
    }
    start
}

/// Build a [`RunReport`] from a completed load-test result.
pub fn build_report(
    run_id: &str,
    result: &LoadTestResult,
    completion_status: CompletionStatus,
    start_time: String,
) -> RunReport {
    let scenario = result.config.scenario.clone();
    let duration_secs = result.config.duration.as_secs();
    let workload = match &result.config.workload {
        Workload::Rate { rps } => WorkloadReport {
            scenario: scenario.clone(),
            duration_secs,
            kind: "rate".to_string(),
            value: *rps,
            url: result.config.url.clone(),
        },
        Workload::Concurrency { n } => WorkloadReport {
            scenario: scenario.clone(),
            duration_secs,
            kind: "concurrency".to_string(),
            value: *n,
            url: result.config.url.clone(),
        },
    };
    let counts = CountsReport {
        total: result.total,
        successful: result.successful,
        failed: result.failed,
    };
    let latency = LatencyReport {
        count: result.latency.count,
        min_ms: result.latency.min_ms,
        max_ms: result.latency.max_ms,
        mean_ms: result.latency.mean_ms,
        p50_ms: result.latency.p50_ms,
        p95_ms: result.latency.p95_ms,
        p99_ms: result.latency.p99_ms,
    };
    let mut warnings = Vec::new();
    if completion_status == CompletionStatus::Incomplete {
        warnings.push(
            "experiment marked incomplete: workload did not finish or fault cleanup failed"
                .to_string(),
        );
    }
    // telemetry_references is empty in F-018: loadgen does not collect trace IDs.
    // Missing telemetry is reported explicitly as a warning, never as healthy.
    warnings.push("missing telemetry: trace IDs are not collected in F-018".to_string());
    RunReport {
        run_id: run_id.to_string(),
        scenario,
        start_time,
        duration_secs,
        completion_status,
        workload,
        counts,
        latency,
        throughput_rps: result.throughput_rps,
        telemetry_references: Vec::new(),
        warnings,
    }
}

/// Write a report as JSON to `<reports_dir>/<run-id>.json`.
///
/// Creates the reports directory if it does not exist.
///
/// # Errors
/// Returns an [`std::io::Error`] if the directory cannot be created or the
/// file cannot be written.
pub fn write_report(report: &RunReport) -> std::io::Result<()> {
    let dir = reports_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", report.run_id));
    let json = serde_json::to_string_pretty(report)
        .map_err(|e| std::io::Error::other(format!("serialize report: {e}")))?;
    fs::write(path, json)
}

/// Read a report from `<reports_dir>/<run-id>.json`.
///
/// # Errors
/// Returns an [`std::io::Error`] if the file does not exist or cannot be
/// read/parsed.
pub fn read_report(run_id: &str) -> std::io::Result<RunReport> {
    let path = reports_dir().join(format!("{run_id}.json"));
    let json = fs::read_to_string(&path)?;
    serde_json::from_str(&json).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("parse report {path:?}: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadgen::{LatencyStats, LoadTestConfig, LoadTestResult, Workload};
    use std::time::Duration;

    fn sample_config() -> LoadTestConfig {
        LoadTestConfig {
            scenario: "baseline".to_string(),
            duration: Duration::from_secs(10),
            workload: Workload::Rate { rps: 20 },
            url: "http://127.0.0.1:8080/health".to_string(),
        }
    }

    fn sample_result() -> LoadTestResult {
        LoadTestResult {
            config: sample_config(),
            run_id: String::new(),
            completion_status: CompletionStatus::Complete,
            total: 100,
            successful: 95,
            failed: 5,
            throughput_rps: 10.0,
            latency: LatencyStats {
                count: 100,
                min_ms: 1.0,
                max_ms: 50.0,
                mean_ms: 10.0,
                p50_ms: 9.0,
                p95_ms: 20.0,
                p99_ms: 40.0,
            },
        }
    }

    #[test]
    fn generate_run_id_format() {
        let id = generate_run_id("baseline");
        assert!(id.starts_with("baseline-"));
        assert!(id.split('-').next_back().unwrap().parse::<u128>().is_ok());
    }

    #[test]
    fn generate_run_id_unique_per_call() {
        let a = generate_run_id("baseline");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let b = generate_run_id("baseline");
        assert_ne!(a, b);
    }

    #[test]
    fn build_report_matches_result() {
        let result = sample_result();
        let report = build_report(
            "baseline-123",
            &result,
            CompletionStatus::Complete,
            "2026-01-01T00:00:00Z".to_string(),
        );
        assert_eq!(report.run_id, "baseline-123");
        assert_eq!(report.scenario, "baseline");
        assert_eq!(report.completion_status, CompletionStatus::Complete);
        assert_eq!(report.counts.total, 100);
        assert_eq!(report.counts.successful, 95);
        assert_eq!(report.counts.failed, 5);
        assert_eq!(report.workload.kind, "rate");
        assert_eq!(report.workload.value, 20);
        assert_eq!(report.latency.p95_ms, 20.0);
        assert!(report.telemetry_references.is_empty());
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn build_report_incomplete_has_warning() {
        let result = sample_result();
        let report = build_report(
            "baseline-123",
            &result,
            CompletionStatus::Incomplete,
            "2026-01-01T00:00:00Z".to_string(),
        );
        assert_eq!(report.completion_status, CompletionStatus::Incomplete);
        assert!(report.warnings.iter().any(|w| w.contains("incomplete")));
    }

    #[test]
    fn build_report_concurrency_workload() {
        let mut config = sample_config();
        config.workload = Workload::Concurrency { n: 8 };
        let result = LoadTestResult {
            config,
            run_id: String::new(),
            completion_status: CompletionStatus::Complete,
            total: 50,
            successful: 50,
            failed: 0,
            throughput_rps: 5.0,
            latency: LatencyStats {
                count: 50,
                min_ms: 2.0,
                max_ms: 30.0,
                mean_ms: 12.0,
                p50_ms: 11.0,
                p95_ms: 25.0,
                p99_ms: 28.0,
            },
        };
        let report = build_report(
            "conc-1",
            &result,
            CompletionStatus::Complete,
            "t".to_string(),
        );
        assert_eq!(report.workload.kind, "concurrency");
        assert_eq!(report.workload.value, 8);
        assert_eq!(report.counts.total, 50);
    }

    #[test]
    fn round_trip_uses_temp_dir() {
        let dir = std::env::temp_dir().join(format!("renzu-report-test-{}", std::process::id()));
        std::env::set_var(REPORTS_DIR_ENV, &dir);
        let result = sample_result();
        let report = build_report(
            "rt-1",
            &result,
            CompletionStatus::Complete,
            "2026-01-01T00:00:00Z".to_string(),
        );
        write_report(&report).expect("write report");
        let read_back = read_report("rt-1").expect("read report");
        assert_eq!(read_back.run_id, report.run_id);
        assert_eq!(read_back.scenario, report.scenario);
        assert_eq!(read_back.completion_status, report.completion_status);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_unknown_run_id_errors() {
        let dir = std::env::temp_dir().join(format!("renzu-report-test-{}", std::process::id()));
        std::env::set_var(REPORTS_DIR_ENV, &dir);
        let err = read_report("does-not-exist").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        let _ = fs::remove_dir_all(&dir);
    }
}
