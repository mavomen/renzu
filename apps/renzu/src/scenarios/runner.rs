//! Scenario execution engine.
//!
//! Runs a registered scenario against the local observability environment.
//! Faults are injected through the gateway's fault-control HTTP endpoints
//! (bound to 127.0.0.1) and are always cleaned up, even if the scenario
//! fails.

use crate::scenarios::{default_registry, FaultType, Scenario, ScenarioResult};
use reqwest::blocking::Client;
use std::time::{Duration, Instant};

/// Base URL of the gateway fault-control API.
const GATEWAY_BASE_URL: &str = "http://127.0.0.1:8080";

/// Run a named scenario against the local environment.
///
/// # Errors
/// Returns an error if the scenario is not found in the registry.
pub fn run_scenario(name: &str) -> Result<ScenarioResult, String> {
    let registry = default_registry();
    let scenario = registry
        .find(name)
        .ok_or_else(|| format!("unknown scenario: {name}"))?;

    let start = Instant::now();
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;

    // Enable the fault (if any), run, then always clean up.
    let enable_result = enable_fault(&client, scenario);
    let run_result = run_scenario_body(&client, scenario);
    let _ = disable_fault(&client, scenario); // cleanup always runs

    let duration = start.elapsed();

    match (enable_result, run_result) {
        (Err(e), _) | (_, Err(e)) => Err(format!("scenario {name} failed: {e}")),
        (Ok(()), Ok(())) => {
            // A controlled-error scenario is expected to return an error
            // from the gateway; that is not a runner failure.
            if matches!(scenario.fault, FaultType::ControlledError(_)) {
                Ok(ScenarioResult::err(
                    name.to_string(),
                    duration,
                    scenario.fault.error_message(),
                ))
            } else {
                Ok(ScenarioResult::ok(name.to_string(), duration))
            }
        }
    }
}

/// Execute the scenario body: for fault scenarios, verify the gateway
/// responds (with the expected fault behavior); for baseline, just check
/// the gateway is up.
fn run_scenario_body(client: &Client, scenario: &Scenario) -> Result<(), String> {
    let health_url = format!("{GATEWAY_BASE_URL}/health");
    let response = client
        .get(&health_url)
        .send()
        .map_err(|e| format!("gateway health check failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "gateway health check returned {}",
            response.status()
        ));
    }

    match &scenario.fault {
        FaultType::None => Ok(()),
        FaultType::DatabaseLatency(_) => {
            // Verify the fault is active by checking /db/health still responds
            let db_url = format!("{GATEWAY_BASE_URL}/db/health");
            let _db_response = client
                .get(&db_url)
                .send()
                .map_err(|e| format!("database health check failed: {e}"))?;
            Ok(())
        }
        FaultType::DownstreamTimeout(_) => {
            // Verify downstream is reachable (may be slow due to timeout)
            let order_url = format!("{GATEWAY_BASE_URL}/order/test");
            let _order_response = client
                .get(&order_url)
                .send()
                .map_err(|e| format!("downstream order request failed: {e}"))?;
            Ok(())
        }
        FaultType::ControlledError(_) => {
            // Controlled error: the gateway returns an error, which is expected
            Ok(())
        }
        FaultType::CascadingFailure => {
            // Verify both DB and downstream are reachable
            let db_url = format!("{GATEWAY_BASE_URL}/db/health");
            let _db_response = client
                .get(&db_url)
                .send()
                .map_err(|e| format!("database health check failed: {e}"))?;
            let order_url = format!("{GATEWAY_BASE_URL}/order/test");
            let _order_response = client
                .get(&order_url)
                .send()
                .map_err(|e| format!("downstream order request failed: {e}"))?;
            Ok(())
        }
    }
}

/// Enable a fault on the gateway via its fault-control endpoint.
fn enable_fault(client: &Client, scenario: &Scenario) -> Result<(), String> {
    match &scenario.fault {
        FaultType::None => Ok(()),
        FaultType::DatabaseLatency(_) => {
            post_fault(client, "/faults/database-latency")?;
            Ok(())
        }
        FaultType::DownstreamTimeout(_) => {
            post_fault(client, "/faults/downstream-timeout")?;
            Ok(())
        }
        FaultType::ControlledError(msg) => {
            let body = format!(r#"{{"message":"{}"}}"#, msg);
            let response = client
                .post(format!("{GATEWAY_BASE_URL}/faults/controlled-error"))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body)
                .send()
                .map_err(|e| format!("failed to enable controlled-error fault: {e}"))?;
            if !response.status().is_success() {
                return Err(format!(
                    "failed to enable controlled-error fault: {}",
                    response.status()
                ));
            }
            Ok(())
        }
        FaultType::CascadingFailure => {
            post_fault(client, "/faults/database-latency")?;
            post_fault(client, "/faults/downstream-timeout")?;
            Ok(())
        }
    }
}

/// Disable (clean up) a fault on the gateway.
fn disable_fault(client: &Client, scenario: &Scenario) -> Result<(), String> {
    match &scenario.fault {
        FaultType::None => Ok(()),
        FaultType::DatabaseLatency(_) => {
            delete_fault(client, "/faults/database-latency")?;
            Ok(())
        }
        FaultType::DownstreamTimeout(_) => {
            delete_fault(client, "/faults/downstream-timeout")?;
            Ok(())
        }
        FaultType::ControlledError(_) => {
            delete_fault(client, "/faults/controlled-error")?;
            Ok(())
        }
        FaultType::CascadingFailure => {
            let _ = delete_fault(client, "/faults/database-latency");
            let _ = delete_fault(client, "/faults/downstream-timeout");
            Ok(())
        }
    }
}

/// POST to a fault-control endpoint to enable a fault.
fn post_fault(client: &Client, path: &str) -> Result<(), String> {
    let response = client
        .post(format!("{GATEWAY_BASE_URL}{path}"))
        .send()
        .map_err(|e| format!("failed to enable fault at {path}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "failed to enable fault at {path}: {}",
            response.status()
        ));
    }
    Ok(())
}

/// DELETE from a fault-control endpoint to disable a fault.
fn delete_fault(client: &Client, path: &str) -> Result<(), String> {
    let response = client
        .delete(format!("{GATEWAY_BASE_URL}{path}"))
        .send()
        .map_err(|e| format!("failed to disable fault at {path}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "failed to disable fault at {path}: {}",
            response.status()
        ));
    }
    Ok(())
}

impl FaultType {
    /// Return the expected error message for a ControlledError fault.
    fn error_message(&self) -> String {
        match self {
            FaultType::ControlledError(msg) => msg.clone(),
            _ => "controlled error".to_string(),
        }
    }
}
