//! Performance comparison of two saved experiment reports.
//!
//! Reads two [`RunReport`]s via [`crate::report::read_report`], computes
//! absolute and percentage deltas between them, flags incompatible runs
//! (scenario mismatch or workload kind/value mismatch), and prints a
//! human-readable side-by-side table to stdout.
//!
//! Percentage deltas are omitted (`n/a`) when the baseline value is zero,
//! since a percentage delta is undefined in that case.

use crate::report::RunReport;
use std::io;

/// Result of comparing two experiment reports.
#[derive(Debug)]
pub struct Comparison {
    /// Baseline (left) report.
    pub left: RunReport,
    /// Compared (right) report.
    pub right: RunReport,
    /// Whether the two runs are structurally compatible.
    pub compatible: bool,
    /// Human-readable reason for incompatibility, when `compatible` is false.
    pub incompatibility_reason: Option<String>,
    /// Per-metric deltas (right minus left).
    pub deltas: Deltas,
}

/// Absolute and percentage delta for a single metric.
#[derive(Debug, Clone)]
pub struct NumericDelta {
    pub baseline: f64,
    pub compared: f64,
    pub absolute: f64,
    pub percent: Option<f64>,
}

/// All computed deltas between two reports.
#[derive(Debug)]
pub struct Deltas {
    pub throughput_rps: NumericDelta,
    pub total: NumericDelta,
    pub successful: NumericDelta,
    pub failed: NumericDelta,
    pub min_ms: NumericDelta,
    pub max_ms: NumericDelta,
    pub mean_ms: NumericDelta,
    pub p50_ms: NumericDelta,
    pub p95_ms: NumericDelta,
    pub p99_ms: NumericDelta,
}

/// Read both reports and compare them.
///
/// Unknown or unreadable run IDs surface as [`std::io::Error`]; the caller is
/// responsible for converting them to [`crate::CliError`] messages.
pub fn compare(left_run_id: &str, right_run_id: &str) -> io::Result<Comparison> {
    let left = crate::report::read_report(left_run_id)?;
    let right = crate::report::read_report(right_run_id)?;
    Ok(compare_reports(&left, &right))
}

/// Pure comparison logic: build deltas and the compatibility flag from two
/// already-loaded reports.
pub fn compare_reports(left: &RunReport, right: &RunReport) -> Comparison {
    let compatible = reports_compatible(left, right);
    let incompatibility_reason = if compatible {
        None
    } else {
        Some(incompatibility_reason_text(left, right))
    };
    let deltas = Deltas {
        throughput_rps: delta(left.throughput_rps, right.throughput_rps),
        total: delta(left.counts.total as f64, right.counts.total as f64),
        successful: delta(
            left.counts.successful as f64,
            right.counts.successful as f64,
        ),
        failed: delta(left.counts.failed as f64, right.counts.failed as f64),
        min_ms: delta(left.latency.min_ms, right.latency.min_ms),
        max_ms: delta(left.latency.max_ms, right.latency.max_ms),
        mean_ms: delta(left.latency.mean_ms, right.latency.mean_ms),
        p50_ms: delta(left.latency.p50_ms, right.latency.p50_ms),
        p95_ms: delta(left.latency.p95_ms, right.latency.p95_ms),
        p99_ms: delta(left.latency.p99_ms, right.latency.p99_ms),
    };
    Comparison {
        left: left.clone(),
        right: right.clone(),
        compatible,
        incompatibility_reason,
        deltas,
    }
}

/// Whether two reports are structurally comparable.
///
/// A comparison is compatible when scenario name, workload kind, and workload
/// value all match. Environment and measurement conditions are not checked.
fn reports_compatible(left: &RunReport, right: &RunReport) -> bool {
    left.scenario == right.scenario
        && left.workload.kind == right.workload.kind
        && left.workload.value == right.workload.value
}

/// Human-readable reason the two reports are incompatible.
fn incompatibility_reason_text(left: &RunReport, right: &RunReport) -> String {
    let mut reasons = Vec::new();
    if left.scenario != right.scenario {
        reasons.push(format!(
            "scenario differs: {} vs {}",
            left.scenario, right.scenario
        ));
    }
    if left.workload.kind != right.workload.kind || left.workload.value != right.workload.value {
        reasons.push(format!(
            "workload differs: {} {} vs {} {}",
            left.workload.kind, left.workload.value, right.workload.kind, right.workload.value
        ));
    }
    reasons.join("; ")
}

/// Compute the delta (compared minus baseline) for a numeric metric.
///
/// `percent` is [`None`] when the baseline is zero, since a percentage delta
/// is undefined in that case.
fn delta(baseline: f64, compared: f64) -> NumericDelta {
    let absolute = compared - baseline;
    let percent = if baseline == 0.0 {
        None
    } else {
        Some(absolute / baseline * 100.0)
    };
    NumericDelta {
        baseline,
        compared,
        absolute,
        percent,
    }
}

/// Print a human-readable comparison table to stdout.
pub fn print_comparison(comparison: &Comparison) {
    let mut out = String::new();
    out.push_str("Performance comparison\n");
    out.push_str("======================\n");
    out.push_str(&format!(
        "\nRun A: {} ({})\nRun B: {} ({})\n",
        comparison.left.run_id,
        comparison.left.scenario,
        comparison.right.run_id,
        comparison.right.scenario,
    ));
    if !comparison.compatible {
        out.push_str(&format!(
            "\nWARNING: incompatible runs — {}\n",
            comparison.incompatibility_reason.as_deref().unwrap_or("")
        ));
    }
    out.push('\n');
    out.push_str("Metric                 Run A     Run B     Abs delta  % delta\n");
    out.push_str("------------------------------------------------------------\n");

    let d = &comparison.deltas;

    push_row(&mut out, "Throughput (req/s)", &d.throughput_rps, false);
    push_row(&mut out, "Total requests", &d.total, true);
    push_row(&mut out, "Successful", &d.successful, true);
    push_row(&mut out, "Failed", &d.failed, true);
    push_row(&mut out, "Latency min (ms)", &d.min_ms, false);
    push_row(&mut out, "Latency max (ms)", &d.max_ms, false);
    push_row(&mut out, "Latency mean (ms)", &d.mean_ms, false);
    push_row(&mut out, "Latency p50 (ms)", &d.p50_ms, false);
    push_row(&mut out, "Latency p95 (ms)", &d.p95_ms, false);
    push_row(&mut out, "Latency p99 (ms)", &d.p99_ms, false);

    print!("{out}");
}

fn push_row(out: &mut String, label: &str, delta: &NumericDelta, is_count: bool) {
    out.push_str(&format!(
        "{:<22} {:>8} {:>8} {:>10} {:>8}\n",
        label,
        format_value(delta.baseline, is_count),
        format_value(delta.compared, is_count),
        format_abs(delta.absolute, is_count),
        format_percent(delta.percent),
    ));
}

fn format_value(value: f64, is_count: bool) -> String {
    if is_count {
        format!("{:.0}", value)
    } else {
        format!("{:.2}", value)
    }
}

fn format_abs(delta: f64, is_count: bool) -> String {
    let sign = if delta > 0.0 {
        "+"
    } else if delta < 0.0 {
        "-"
    } else {
        ""
    };
    let mag = delta.abs();
    if is_count {
        format!("{sign}{mag:.0}")
    } else {
        format!("{sign}{mag:.2}")
    }
}

fn format_percent(percent: Option<f64>) -> String {
    match percent {
        Some(p) => {
            let sign = if p > 0.0 {
                "+"
            } else if p < 0.0 {
                "-"
            } else {
                ""
            };
            format!("{sign}{:.2}%", p.abs())
        }
        None => "n/a".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadgen::CompletionStatus;
    use crate::report::{CountsReport, LatencyReport, WorkloadReport};

    fn test_report(
        scenario: &str,
        kind: &str,
        value: u64,
        throughput_rps: f64,
        total: u64,
    ) -> RunReport {
        RunReport {
            run_id: "test-run".to_string(),
            scenario: scenario.to_string(),
            start_time: "2026-01-01T00:00:00Z".to_string(),
            duration_secs: 10,
            completion_status: CompletionStatus::Complete,
            workload: WorkloadReport {
                scenario: scenario.to_string(),
                duration_secs: 10,
                kind: kind.to_string(),
                value,
                url: "http://127.0.0.1:8080/health".to_string(),
            },
            counts: CountsReport {
                total,
                successful: total,
                failed: 0,
            },
            latency: LatencyReport {
                count: total as usize,
                min_ms: 1.0,
                max_ms: 10.0,
                mean_ms: 5.0,
                p50_ms: 5.0,
                p95_ms: 9.0,
                p99_ms: 10.0,
            },
            throughput_rps,
            telemetry_references: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn test_delta_computation() {
        let left = test_report("baseline", "rate", 20, 10.0, 100);
        let right = test_report("baseline", "rate", 20, 12.0, 120);
        let comparison = compare_reports(&left, &right);
        assert!(comparison.compatible);
        assert_eq!(comparison.deltas.throughput_rps.absolute, 2.0);
        assert!((comparison.deltas.throughput_rps.percent.unwrap() - 20.0).abs() < f64::EPSILON);
        assert_eq!(comparison.deltas.total.absolute, 20.0);
        assert!((comparison.deltas.total.percent.unwrap() - 20.0).abs() < f64::EPSILON);
        assert_eq!(comparison.deltas.failed.absolute, 0.0);
    }

    #[test]
    fn percent_delta_omitted_when_baseline_zero() {
        let left = test_report("baseline", "rate", 20, 0.0, 0);
        let right = test_report("baseline", "rate", 20, 5.0, 10);
        let comparison = compare_reports(&left, &right);
        assert_eq!(comparison.deltas.throughput_rps.percent, None);
        assert_eq!(comparison.deltas.throughput_rps.absolute, 5.0);
        assert_eq!(comparison.deltas.total.percent, None);
    }

    #[test]
    fn negative_deltas() {
        let left = test_report("baseline", "rate", 20, 20.0, 200);
        let right = test_report("baseline", "rate", 20, 10.0, 100);
        let comparison = compare_reports(&left, &right);
        assert_eq!(comparison.deltas.throughput_rps.absolute, -10.0);
        assert!((comparison.deltas.throughput_rps.percent.unwrap() - -50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compatible_identical_reports() {
        let left = test_report("baseline", "rate", 20, 10.0, 100);
        let right = test_report("baseline", "rate", 20, 10.0, 100);
        let comparison = compare_reports(&left, &right);
        assert!(comparison.compatible);
        assert!(comparison.incompatibility_reason.is_none());
    }

    #[test]
    fn incompatible_scenario_mismatch() {
        let left = test_report("baseline", "rate", 20, 10.0, 100);
        let right = test_report("faulted", "rate", 20, 10.0, 100);
        let comparison = compare_reports(&left, &right);
        assert!(!comparison.compatible);
        let reason = comparison.incompatibility_reason.unwrap();
        assert!(reason.contains("scenario differs"));
    }

    #[test]
    fn incompatible_workload_kind_mismatch() {
        let left = test_report("baseline", "rate", 20, 10.0, 100);
        let right = test_report("baseline", "concurrency", 10, 10.0, 100);
        let comparison = compare_reports(&left, &right);
        assert!(!comparison.compatible);
        let reason = comparison.incompatibility_reason.unwrap();
        assert!(reason.contains("workload differs"));
    }

    #[test]
    fn incompatible_workload_value_mismatch() {
        let left = test_report("baseline", "rate", 20, 10.0, 100);
        let right = test_report("baseline", "rate", 50, 10.0, 100);
        let comparison = compare_reports(&left, &right);
        assert!(!comparison.compatible);
        let reason = comparison.incompatibility_reason.unwrap();
        assert!(reason.contains("workload differs"));
    }
}
