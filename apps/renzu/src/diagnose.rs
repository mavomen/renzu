//! Explainable incident diagnosis for saved experiment reports.
//!
//! Reads [`RunReport`]s via [`crate::report::read_report`] and generates
//! diagnostic hypotheses using rule-based heuristics. No AI/ML model is
//! required; model integration is documented as future work.
//!
//! Each hypothesis is classified as an [`HypothesisKind::Observation`]
//! (directly measurable fact from the report) or an
//! [`HypothesisKind::Inference`] (interpretation of that fact), cites the
//! report fields that support it, and states its uncertainty explicitly.
//! The assistant never modifies the environment.

use crate::report::RunReport;
use std::io;

/// Threshold above which the error rate is considered elevated.
const ERROR_RATE_THRESHOLD: f64 = 0.05;

/// Threshold above which p95 latency is considered elevated (milliseconds).
const LATENCY_P95_THRESHOLD_MS: f64 = 500.0;

/// Threshold below which throughput is considered low (requests per second).
const THROUGHPUT_LOW_THRESHOLD_RPS: f64 = 1.0;

/// Kind of hypothesis: an observed fact or an inferred interpretation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum HypothesisKind {
    /// Directly measurable fact from the report.
    Observation,
    /// Interpretation of observed facts.
    Inference,
}

/// Confidence level for a hypothesis.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum Confidence {
    High,
    Medium,
}

impl std::fmt::Display for Confidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Confidence::High => write!(f, "high"),
            Confidence::Medium => write!(f, "medium"),
        }
    }
}

/// A single diagnostic hypothesis.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Hypothesis {
    /// Stable identifier for the hypothesis.
    pub id: String,
    /// Human-readable hypothesis text.
    pub text: String,
    /// Whether this is an observation or an inference.
    pub kind: HypothesisKind,
    /// Confidence level.
    pub confidence: Confidence,
    /// Report fields or values cited as supporting evidence.
    pub evidence: Vec<String>,
    /// Explicit uncertainty note, if any.
    pub uncertainty: Option<String>,
}

/// Result of diagnosing a single experiment run.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiagnoseResult {
    /// Run identifier.
    pub run_id: String,
    /// Scenario name.
    pub scenario: String,
    /// Generated hypotheses, ordered by confidence then id.
    pub hypotheses: Vec<Hypothesis>,
    /// True when there are no requests and no hypothesis can be formed.
    pub insufficient_data: bool,
}

/// Run the rule-based diagnostic heuristics over a [`RunReport`].
pub fn diagnose(report: &RunReport) -> DiagnoseResult {
    let mut hypotheses: Vec<Hypothesis> = Vec::new();

    let total = report.counts.total;
    let insufficient_data = total == 0;

    if insufficient_data {
        return DiagnoseResult {
            run_id: report.run_id.clone(),
            scenario: report.scenario.clone(),
            hypotheses,
            insufficient_data: true,
        };
    }

    let error_rate = report.counts.failed as f64 / total as f64;

    // High error rate inference.
    if error_rate > ERROR_RATE_THRESHOLD {
        hypotheses.push(Hypothesis {
            id: "elevated-error-rate".to_string(),
            text: format!(
                "Elevated error rate: {:.1}% of requests failed ({}/{})",
                error_rate * 100.0,
                report.counts.failed,
                total
            ),
            kind: HypothesisKind::Inference,
            confidence: if error_rate > 0.5 {
                Confidence::High
            } else {
                Confidence::Medium
            },
            evidence: vec![
                format!("counts.total = {}", total),
                format!("counts.failed = {}", report.counts.failed),
                format!("error rate = {:.1}%", error_rate * 100.0),
            ],
            uncertainty: Some(
                "Error rate alone does not identify the root cause; inspect \
                 telemetry_references for correlated traces and logs."
                    .to_string(),
            ),
        });
    } else {
        hypotheses.push(Hypothesis {
            id: "acceptable-error-rate".to_string(),
            text: format!(
                "Acceptable error rate: {:.1}% of requests failed ({}/{})",
                error_rate * 100.0,
                report.counts.failed,
                total
            ),
            kind: HypothesisKind::Observation,
            confidence: Confidence::High,
            evidence: vec![
                format!("counts.total = {}", total),
                format!("counts.failed = {}", report.counts.failed),
                format!("error rate = {:.1}%", error_rate * 100.0),
            ],
            uncertainty: None,
        });
    }

    // High p95 latency inference.
    if report.latency.p95_ms > LATENCY_P95_THRESHOLD_MS {
        hypotheses.push(Hypothesis {
            id: "elevated-latency".to_string(),
            text: format!(
                "Elevated p95 latency: {:.1} ms exceeds {} ms threshold",
                report.latency.p95_ms, LATENCY_P95_THRESHOLD_MS
            ),
            kind: HypothesisKind::Inference,
            confidence: Confidence::Medium,
            evidence: vec![
                format!("latency.p95_ms = {:.1}", report.latency.p95_ms),
                format!("latency.mean_ms = {:.1}", report.latency.mean_ms),
                format!("threshold = {} ms", LATENCY_P95_THRESHOLD_MS),
            ],
            uncertainty: Some(
                "p95 is an aggregate; individual slow spans are identified \
                 via telemetry_references, not from this statistic alone."
                    .to_string(),
            ),
        });
    } else {
        hypotheses.push(Hypothesis {
            id: "acceptable-latency".to_string(),
            text: format!(
                "Acceptable p95 latency: {:.1} ms within {} ms threshold",
                report.latency.p95_ms, LATENCY_P95_THRESHOLD_MS
            ),
            kind: HypothesisKind::Observation,
            confidence: Confidence::High,
            evidence: vec![
                format!("latency.p95_ms = {:.1}", report.latency.p95_ms),
                format!("threshold = {} ms", LATENCY_P95_THRESHOLD_MS),
            ],
            uncertainty: None,
        });
    }

    // Low throughput observation.
    if report.throughput_rps < THROUGHPUT_LOW_THRESHOLD_RPS && total > 0 {
        hypotheses.push(Hypothesis {
            id: "low-throughput".to_string(),
            text: format!(
                "Low throughput: {:.1} requests/second over {} seconds",
                report.throughput_rps, report.duration_secs
            ),
            kind: HypothesisKind::Observation,
            confidence: Confidence::Medium,
            evidence: vec![
                format!("throughput_rps = {:.1}", report.throughput_rps),
                format!("duration_secs = {}", report.duration_secs),
                format!("counts.total = {}", total),
            ],
            uncertainty: Some(
                "Throughput depends on the configured workload shape; low \
                 volume may be intentional."
                    .to_string(),
            ),
        });
    }

    // Incomplete run observation.
    if report.completion_status != crate::loadgen::CompletionStatus::Complete {
        hypotheses.push(Hypothesis {
            id: "incomplete-run".to_string(),
            text: format!(
                "Run did not complete: status {:?}",
                report.completion_status
            ),
            kind: HypothesisKind::Observation,
            confidence: Confidence::High,
            evidence: vec![format!(
                "completion_status = {:?}",
                report.completion_status
            )],
            uncertainty: Some("Incomplete runs may have partial or missing telemetry.".to_string()),
        });
    }

    // Warnings observation.
    if !report.warnings.is_empty() {
        hypotheses.push(Hypothesis {
            id: "warnings-present".to_string(),
            text: format!(
                "{} warning(s) recorded during the run",
                report.warnings.len()
            ),
            kind: HypothesisKind::Observation,
            confidence: Confidence::Medium,
            evidence: report.warnings.clone(),
            uncertainty: None,
        });
    }

    // Telemetry references observation.
    if !report.telemetry_references.is_empty() {
        hypotheses.push(Hypothesis {
            id: "telemetry-available".to_string(),
            text: format!(
                "{} telemetry reference(s) available for trace/log lookup",
                report.telemetry_references.len()
            ),
            kind: HypothesisKind::Observation,
            confidence: Confidence::High,
            evidence: report.telemetry_references.clone(),
            uncertainty: None,
        });
    }

    // Sort by confidence (High first) then id for stable output.
    hypotheses.sort_by(|a, b| {
        let conf_order = |c: &Confidence| match c {
            Confidence::High => 0,
            Confidence::Medium => 1,
        };
        conf_order(&a.confidence)
            .cmp(&conf_order(&b.confidence))
            .then_with(|| a.id.cmp(&b.id))
    });

    DiagnoseResult {
        run_id: report.run_id.clone(),
        scenario: report.scenario.clone(),
        hypotheses,
        insufficient_data: false,
    }
}

/// Read a saved report and diagnose it.
pub fn diagnose_for_run(run_id: &str) -> io::Result<DiagnoseResult> {
    let report = crate::report::read_report(run_id)?;
    Ok(diagnose(&report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadgen::CompletionStatus;
    use crate::report::{CountsReport, LatencyReport, RunReport, WorkloadReport};

    fn make_report(
        total: u64,
        successful: u64,
        p95_ms: f64,
        throughput_rps: f64,
        completion_status: CompletionStatus,
    ) -> RunReport {
        RunReport {
            run_id: "rt-1".to_string(),
            scenario: "baseline".to_string(),
            start_time: "2026-11-24T10:00:00Z".to_string(),
            duration_secs: 60,
            completion_status,
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
                p99_ms: p99_ms(p95_ms),
            },
            throughput_rps,
            telemetry_references: vec![],
            warnings: vec![],
            schema_version: 1,
        }
    }

    fn p99_ms(p95_ms: f64) -> f64 {
        p95_ms * 1.2
    }

    #[test]
    fn zero_requests_is_insufficient_data() {
        let report = make_report(0, 0, 10.0, 0.0, CompletionStatus::Complete);
        let result = diagnose(&report);
        assert!(result.insufficient_data);
        assert!(result.hypotheses.is_empty());
    }

    #[test]
    fn all_successful_low_latency_yields_observations_only() {
        let report = make_report(100, 100, 50.0, 10.0, CompletionStatus::Complete);
        let result = diagnose(&report);
        assert!(!result.insufficient_data);
        let inferences: Vec<&Hypothesis> = result
            .hypotheses
            .iter()
            .filter(|h| h.kind == HypothesisKind::Inference)
            .collect();
        assert!(
            inferences.is_empty(),
            "no inferences expected for healthy run"
        );
        let obs_ids: Vec<&str> = result.hypotheses.iter().map(|h| h.id.as_str()).collect();
        assert!(
            obs_ids.contains(&"acceptable-error-rate"),
            "expected acceptable-error-rate observation"
        );
        assert!(
            obs_ids.contains(&"acceptable-latency"),
            "expected acceptable-latency observation"
        );
    }

    #[test]
    fn high_error_rate_produces_inference() {
        let report = make_report(100, 90, 50.0, 10.0, CompletionStatus::Complete);
        let result = diagnose(&report);
        let error_inference = result
            .hypotheses
            .iter()
            .find(|h| h.id == "elevated-error-rate");
        assert!(
            error_inference.is_some(),
            "expected elevated-error-rate inference"
        );
        let inf = error_inference.unwrap();
        assert_eq!(inf.kind, HypothesisKind::Inference);
        assert!(
            inf.evidence
                .iter()
                .any(|e| e.contains("counts.failed = 10")),
            "evidence should cite failed count"
        );
    }

    #[test]
    fn high_latency_produces_inference() {
        let report = make_report(100, 100, 800.0, 10.0, CompletionStatus::Complete);
        let result = diagnose(&report);
        let lat_inference = result
            .hypotheses
            .iter()
            .find(|h| h.id == "elevated-latency");
        assert!(
            lat_inference.is_some(),
            "expected elevated-latency inference"
        );
        let inf = lat_inference.unwrap();
        assert_eq!(inf.kind, HypothesisKind::Inference);
        assert!(
            inf.evidence
                .iter()
                .any(|e| e.contains("latency.p95_ms = 800.0")),
            "evidence should cite p95 latency"
        );
    }

    #[test]
    fn telemetry_references_produce_observation() {
        let mut report = make_report(100, 100, 50.0, 10.0, CompletionStatus::Complete);
        report.telemetry_references = vec![
            "http://jaeger/trace/abc".to_string(),
            "http://grafana/explore".to_string(),
        ];
        let result = diagnose(&report);
        let obs = result
            .hypotheses
            .iter()
            .find(|h| h.id == "telemetry-available");
        assert!(obs.is_some(), "expected telemetry-available observation");
        let obs = obs.unwrap();
        assert_eq!(obs.kind, HypothesisKind::Observation);
        assert_eq!(obs.evidence.len(), 2);
    }

    #[test]
    fn warnings_produce_observation() {
        let mut report = make_report(100, 100, 50.0, 10.0, CompletionStatus::Complete);
        report.warnings = vec!["partial telemetry".to_string()];
        let result = diagnose(&report);
        let obs = result
            .hypotheses
            .iter()
            .find(|h| h.id == "warnings-present");
        assert!(obs.is_some(), "expected warnings-present observation");
        let obs = obs.unwrap();
        assert_eq!(obs.evidence, vec!["partial telemetry".to_string()]);
    }

    #[test]
    fn incomplete_run_produces_observation() {
        let report = make_report(100, 95, 50.0, 10.0, CompletionStatus::Incomplete);
        let result = diagnose(&report);
        let obs = result.hypotheses.iter().find(|h| h.id == "incomplete-run");
        assert!(obs.is_some(), "expected incomplete-run observation");
        let obs = obs.unwrap();
        assert_eq!(obs.kind, HypothesisKind::Observation);
        assert!(
            obs.uncertainty.is_some(),
            "incomplete runs should note uncertainty"
        );
    }

    #[test]
    fn low_throughput_produces_observation() {
        let report = make_report(5, 5, 10.0, 0.1, CompletionStatus::Complete);
        let result = diagnose(&report);
        let obs = result.hypotheses.iter().find(|h| h.id == "low-throughput");
        assert!(obs.is_some(), "expected low-throughput observation");
        let obs = obs.unwrap();
        assert!(
            obs.uncertainty.is_some(),
            "low throughput should note uncertainty about intent"
        );
    }

    #[test]
    fn high_confidence_hypotheses_sort_first() {
        let report = make_report(100, 10, 800.0, 0.1, CompletionStatus::Complete);
        let result = diagnose(&report);
        let first = result.hypotheses.first().unwrap();
        assert_eq!(first.confidence, Confidence::High);
    }
}
