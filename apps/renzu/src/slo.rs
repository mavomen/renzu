//! Service-Level Objective (SLO) and error-budget reporting for saved
//! experiment reports.
//!
//! Reads [`RunReport`]s via [`crate::report::read_report`] and evaluates SLO
//! compliance against configurable availability and latency targets. The
//! error budget is reported as `target − observed`: a positive value means
//! headroom remains, a negative value means the budget has been consumed.
//!
//! # Time window
//!
//! Each SLO evaluation is scoped to a single saved `RunReport`. The run's
//! own duration is the measurement window; no cross-run aggregation is
//! performed.
//!
//! # Availability SLO
//!
//! `availability = successful / total` as a fraction in [0, 1].
//! Error budget remaining = `availability_target − availability`.
//!
//! # Latency SLO
//!
//! `p95_ms` from the report's latency statistics.
//! Error budget remaining = `latency_target_ms − p95_ms`.

use crate::report::RunReport;
use std::io;

/// Result of evaluating SLOs for a single experiment run.
#[derive(Debug, Clone)]
pub struct SloResult {
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
    /// Availability error-budget remaining (`target − observed`).
    /// Positive means headroom remains; negative means over budget.
    pub availability_budget_remaining: f64,
    /// p95 latency in milliseconds.
    pub latency_p95_ms: f64,
    /// Latency target in milliseconds.
    pub latency_target_ms: f64,
    /// Whether latency met the target.
    pub latency_met: bool,
    /// Latency error-budget remaining (`target − observed`).
    /// Positive means headroom remains; negative means over budget.
    pub latency_budget_remaining: f64,
    /// True when there are no requests and SLOs cannot be evaluated.
    pub insufficient_data: bool,
}

/// Compute availability and latency SLOs against the given targets.
///
/// When `total` is zero, `insufficient_data` is set and both SLOs are
/// reported as not met with zero budget remaining.
pub fn compute_slo(
    run_id: &str,
    report: &RunReport,
    availability_target: f64,
    latency_target_ms: f64,
) -> SloResult {
    let total = report.counts.total;
    let insufficient_data = total == 0;

    let (availability, availability_met, availability_budget_remaining) = if insufficient_data {
        (0.0, false, 0.0)
    } else {
        let availability = report.counts.successful as f64 / total as f64;
        let met = availability >= availability_target;
        let budget = availability_target - availability;
        (availability, met, budget)
    };

    let (latency_met, latency_budget_remaining) = if insufficient_data {
        (false, 0.0)
    } else {
        let met = report.latency.p95_ms <= latency_target_ms;
        let budget = latency_target_ms - report.latency.p95_ms;
        (met, budget)
    };

    SloResult {
        run_id: run_id.to_string(),
        total_requests: total,
        availability,
        availability_target,
        availability_met,
        availability_budget_remaining,
        latency_p95_ms: report.latency.p95_ms,
        latency_target_ms,
        latency_met,
        latency_budget_remaining,
        insufficient_data,
    }
}

/// Read a saved report and compute SLOs against the given targets.
pub fn compute_slo_for_run(
    run_id: &str,
    availability_target: f64,
    latency_target_ms: f64,
) -> io::Result<SloResult> {
    let report = crate::report::read_report(run_id)?;
    Ok(compute_slo(
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
    fn all_successful_run_meets_slo_with_positive_budget() {
        let report = make_report(100, 100, 100.0);
        let slo = compute_slo("rt-1", &report, 0.99, 500.0);
        assert!(!slo.insufficient_data);
        assert_eq!(slo.availability, 1.0);
        assert!(slo.availability_met);
        assert_eq!(slo.availability_budget_remaining, 0.99 - 1.0);
        assert_eq!(slo.latency_p95_ms, 100.0);
        assert!(slo.latency_met);
        assert_eq!(slo.latency_budget_remaining, 500.0 - 100.0);
    }

    #[test]
    fn partial_failures_below_target_consume_budget() {
        let report = make_report(100, 98, 100.0);
        let slo = compute_slo("rt-1", &report, 0.99, 500.0);
        assert_eq!(slo.availability, 0.98);
        assert!(!slo.availability_met);
        assert_eq!(slo.availability_budget_remaining, 0.99 - 0.98);
        assert!(slo.latency_met);
        assert_eq!(slo.latency_budget_remaining, 500.0 - 100.0);
    }

    #[test]
    fn zero_requests_is_insufficient_data() {
        let report = make_report(0, 0, 100.0);
        let slo = compute_slo("rt-1", &report, 0.99, 500.0);
        assert!(slo.insufficient_data);
        assert_eq!(slo.availability, 0.0);
        assert!(!slo.availability_met);
        assert_eq!(slo.availability_budget_remaining, 0.0);
        assert_eq!(slo.latency_p95_ms, 100.0);
        assert!(!slo.latency_met);
        assert_eq!(slo.latency_budget_remaining, 0.0);
    }

    #[test]
    fn latency_p95_over_target_consumes_budget() {
        let report = make_report(100, 100, 1000.0);
        let slo = compute_slo("rt-1", &report, 0.99, 500.0);
        assert_eq!(slo.latency_p95_ms, 1000.0);
        assert!(!slo.latency_met);
        assert_eq!(slo.latency_budget_remaining, 500.0 - 1000.0);
    }

    #[test]
    fn latency_p95_exactly_at_target_is_met_with_zero_budget() {
        let report = make_report(100, 100, 500.0);
        let slo = compute_slo("rt-1", &report, 0.99, 500.0);
        assert!(slo.latency_met);
        assert_eq!(slo.latency_budget_remaining, 0.0);
    }

    #[test]
    fn custom_targets_are_respected() {
        let report = make_report(100, 99, 200.0);
        let slo = compute_slo("rt-1", &report, 0.98, 150.0);
        assert!(slo.availability_met); // 0.99 >= 0.98
        assert_eq!(slo.availability_budget_remaining, 0.98 - 0.99);
        assert!(!slo.latency_met); // 200 > 150
        assert_eq!(slo.latency_budget_remaining, 150.0 - 200.0);
    }
}
