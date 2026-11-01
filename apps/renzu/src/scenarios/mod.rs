//! Scenario system for fault injection and baseline scenarios.
//!
//! Provides built-in scenarios and an executor that can run them against
//! the local observability environment. Faults are injected via HTTP
//! fault-control endpoints on the gateway (bound to 127.0.0.1 by default)
//! and are always cleaned up after the scenario completes, even on error.

use std::time::Duration;

/// A runnable scenario: a named behavior with an optional injected fault.
#[derive(Debug, Clone)]
pub struct Scenario {
    /// Short, unique scenario name (used on the command line).
    pub name: String,
    /// Human-readable description of what the scenario does.
    pub description: String,
    /// The fault to inject, or [`FaultType::None`] for a baseline run.
    pub fault: FaultType,
}

/// Kind of fault a scenario can inject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaultType {
    /// No fault: the scenario exercises the happy path.
    None,
    /// Inject latency into database calls.
    DatabaseLatency(Duration),
    /// Inject a timeout on downstream (orders) calls.
    DownstreamTimeout(Duration),
    /// Return a controlled error from the gateway.
    ControlledError(String),
    /// Chain multiple faults together (database latency + downstream timeout).
    CascadingFailure,
    /// Telemetry collector unavailable: the application still responds but
    /// telemetry export is broken.
    CollectorUnavailable,
    /// Database connection failure: the gateway degrades because the database
    /// is unreachable.
    DatabaseConnectionFailure,
}

/// Outcome of running a scenario.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    /// Name of the scenario that ran.
    pub name: String,
    /// Whether the scenario completed successfully.
    pub success: bool,
    /// Wall-clock time the scenario took to run.
    pub duration: Duration,
    /// Error message if the scenario failed, `None` otherwise.
    pub error: Option<String>,
}

impl ScenarioResult {
    /// Build a successful result.
    fn ok(name: String, duration: Duration) -> Self {
        Self {
            name,
            success: true,
            duration,
            error: None,
        }
    }

    /// Build a failed result.
    fn err(name: String, duration: Duration, error: String) -> Self {
        Self {
            name,
            success: false,
            duration,
            error: Some(error),
        }
    }
}

/// Registry of all built-in scenarios.
#[derive(Debug, Clone, Default)]
pub struct ScenarioRegistry {
    scenarios: Vec<Scenario>,
}

impl ScenarioRegistry {
    /// Create a new registry with all built-in scenarios registered.
    pub fn new() -> Self {
        let scenarios = vec![
            Scenario {
                name: "baseline".to_string(),
                description: "Run with no faults injected (happy path).".to_string(),
                fault: FaultType::None,
            },
            Scenario {
                name: "db-latency".to_string(),
                description: "Inject latency into database calls.".to_string(),
                fault: FaultType::DatabaseLatency(Duration::from_millis(200)),
            },
            Scenario {
                name: "downstream-timeout".to_string(),
                description: "Inject a timeout on downstream (orders) calls.".to_string(),
                fault: FaultType::DownstreamTimeout(Duration::from_secs(5)),
            },
            Scenario {
                name: "controlled-error".to_string(),
                description: "Return a controlled error from the gateway.".to_string(),
                fault: FaultType::ControlledError("simulated gateway error".to_string()),
            },
            Scenario {
                name: "cascading-failure".to_string(),
                description: "Chain database latency and downstream timeout.".to_string(),
                fault: FaultType::CascadingFailure,
            },
            Scenario {
                name: "otel-collector-down".to_string(),
                description: "Make the telemetry collector unavailable (app still responds)."
                    .to_string(),
                fault: FaultType::CollectorUnavailable,
            },
            Scenario {
                name: "db-connection-failure".to_string(),
                description: "Break database connectivity (gateway degrades).".to_string(),
                fault: FaultType::DatabaseConnectionFailure,
            },
        ];
        Self { scenarios }
    }

    /// Return an immutable slice of all registered scenarios.
    pub fn scenarios(&self) -> &[Scenario] {
        &self.scenarios
    }

    /// Look up a scenario by name.
    pub fn find(&self, name: &str) -> Option<&Scenario> {
        self.scenarios.iter().find(|s| s.name == name)
    }
}

/// Return the default registry of built-in scenarios.
pub fn default_registry() -> ScenarioRegistry {
    ScenarioRegistry::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_new_failure_scenarios() {
        let registry = default_registry();
        let names: Vec<&str> = registry
            .scenarios()
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert!(names.contains(&"otel-collector-down"));
        assert!(names.contains(&"db-connection-failure"));
    }

    #[test]
    fn otel_collector_down_has_collector_unavailable_fault() {
        let registry = default_registry();
        let scenario = registry.find("otel-collector-down").unwrap();
        assert_eq!(scenario.fault, FaultType::CollectorUnavailable);
    }

    #[test]
    fn db_connection_failure_has_database_connection_fault() {
        let registry = default_registry();
        let scenario = registry.find("db-connection-failure").unwrap();
        assert_eq!(scenario.fault, FaultType::DatabaseConnectionFailure);
    }

    #[test]
    fn fault_type_variants_compare_equal() {
        assert_eq!(
            FaultType::CollectorUnavailable,
            FaultType::CollectorUnavailable
        );
        assert_eq!(
            FaultType::DatabaseConnectionFailure,
            FaultType::DatabaseConnectionFailure
        );
        assert_ne!(
            FaultType::CollectorUnavailable,
            FaultType::DatabaseConnectionFailure
        );
    }
}

pub mod runner;
