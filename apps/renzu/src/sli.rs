//! Service-Level Indicator (SLI) calculation for saved experiment reports.
//!
//! Reads [`RunReport`]s via [`crate::report::read_report`] and computes
//! availability and latency SLIs against configurable targets.
//!
//! # Availability SLI
//!
//! `availability = successful / total` as a fraction in [0, 1]. When `total`
//! is zero there is insufficient data and neither SLI is reported as met.
//!
//! # Latency SLI
//!
//! `p95_ms <= latency_target_ms`. When there is insufficient data, latency
//! compliance is reported as not met.

use crate::report::RunReport;
use std::io;

/// Result of computing SLIs for a single experiment run.
#[derive(Debug, Clone)]
pub struct SliResult {
    /// Run identifier.
    pub run_id: String,
    /// Total number of requests in the run.
    pub total_requests: u64,
    /// Computed availability as a fraction in [0, 1].
    pub availability: f64,
    /// Availability target fraction (e.g. `0.99` for 99%).
    pub availability_target: f64,
    /// Whether availability met the target.
    pub availability_met: bool,
    /// p95 latency in milliseconds.
    pub latency_p95_ms: f64,
    /// Latency target in milliseconds.
    pub latency_target_ms: f64,
    /// Whether latency met the target.
    pub latency_met: bool,
    /// True when there are no requests and SLIs cannot be evaluated.
    pub insufficient_data: bool,
}

/// Compute availability and latency SLIs for a [`RunReport`].
///
/// When `total` is zero, `insufficient_data` is set and both SLIs are
/// reported as not met.
pub fn compute_sli(
    run_id: &str,
    report: &RunReport,
    availability_target: f64,
    latency_target_ms: f64,
) -> SliResult {
    let total = report.counts.total;
    let insufficient_data = total == 0;

    let (availability, availability_met) = if insufficient_data {
        (0.0, false)
    } else {
        let availability = report.counts.successful as f64 / total as f64;
        (availability, availability >= availability_target)
    };

    let latency_met = if insufficient_data {
        false
    } else {
        report.latency.p95_ms <= latency_target_ms
    };

    SliResult {
        run_id: run_id.to_string(),
        total_requests: total,
        availability,
        availability_target,
        availability_met,
        latency_p95_ms: report.latency.p95_ms,
        latency_target_ms,
        latency_met,
        insufficient_data,
    }
}

/// Read a saved report and compute SLIs against the given targets.
pub fn compute_sli_for_run(
    run_id: &str,
    availability_target: f64,
    latency_target_ms: f64,
) -> io::Result<SliResult> {
    let report = crate::report::read_report(run_id)?;
    Ok(compute_sli(
        run_id,
        &report,
        availability_target,
        latency_target_ms,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadgen::CompletionStatus;
    use crate::report::{CountsReport, LatencyReport, RunReport, WorkloadReport};

    fn make_report(total: u64, successful: u64, p95_ms: f64) -> RunReport {
        RunReport {
            run_id: "rt-1".to_string(),
            scenario: "baseline".to_string(),
            start_time: "2026-11-24T00:00:00Z".to_string(),
            duration_secs: 60,
            completion_status: CompletionStatus::Complete,
            workload: WorkloadReport {
                scenario: "baseline".to_string(),
                duration_secs: 60,
                kind: "rate".to_string(),
                value: 100,
                url: "http://127.0.0.1:8080/health".to_string(),
            },
            counts: CountsReport {
                total,
                successful,
                failed: total - successful,
            },
            latency: LatencyReport {
                count: total as usize,
                min_ms: 1.0,
                max_ms: p95_ms,
                mean_ms: p95_ms / 2.0,
                p50_ms: p95_ms / 2.0,
                p95_ms,
                p99_ms: p95_ms * 1.2,
            },
            throughput_rps: total as f64 / 60.0,
            telemetry_references: vec![],
            warnings: vec![],
            schema_version: 1,
        }
    }

    #[test]
    fn all_successful_run_meets_availability_target() {
        let report = make_report(100, 100, 100.0);
        let sli = compute_sli("rt-1", &report, 0.99, 500.0);
        assert!(!sli.insufficient_data);
        assert_eq!(sli.availability, 1.0);
        assert!(sli.availability_met);
    }

    #[test]
    fn partial_failures_below_target_are_not_met() {
        let report = make_report(100, 98, 100.0);
        let sli = compute_sli("rt-1", &report, 0.99, 500.0);
        assert_eq!(sli.availability, 0.98);
        assert!(!sli.availability_met);
    }

    #[test]
    fn zero_requests_is_insufficient_data() {
        let report = make_report(0, 0, 100.0);
        let sli = compute_sli("rt-1", &report, 0.99, 500.0);
        assert!(sli.insufficient_data);
        assert_eq!(sli.availability, 0.0);
        assert!(!sli.availability_met);
        assert!(!sli.latency_met);
    }

    #[test]
    fn latency_p95_under_target_is_met() {
        let report = make_report(100, 100, 100.0);
        let sli = compute_sli("rt-1", &report, 0.99, 500.0);
        assert_eq!(sli.latency_p95_ms, 100.0);
        assert!(sli.latency_met);
    }

    #[test]
    fn latency_p95_over_target_is_not_met() {
        let report = make_report(100, 100, 1000.0);
        let sli = compute_sli("rt-1", &report, 0.99, 500.0);
        assert_eq!(sli.latency_p95_ms, 1000.0);
        assert!(!sli.latency_met);
    }

    #[test]
    fn latency_p95_exactly_at_target_is_met() {
        let report = make_report(100, 100, 500.0);
        let sli = compute_sli("rt-1", &report, 0.99, 500.0);
        assert!(sli.latency_met);
    }

    #[test]
    fn custom_targets_are_respected() {
        let report = make_report(100, 99, 200.0);
        let sli = compute_sli("rt-1", &report, 0.98, 150.0);
        assert!(sli.availability_met); // 0.99 >= 0.98
        assert!(!sli.latency_met); // 200 > 150
    }
}
