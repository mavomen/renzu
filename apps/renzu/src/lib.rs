//! `renzu` command-line application.
//!
//! Provides `clap`-based argument parsing, top-level help, command and
//! error-handling conventions, and stable exit codes. `up`, `status`, and
//! `down` drive the local observability environment (`compose.yaml`) through
//! `docker compose`; `scenario` and `load-test` drive fault-injection
//! experiments; `report` reads a saved experiment report from disk.

mod loadgen;
mod report;
mod scenarios;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread::sleep;
use std::time::{Duration, Instant};

use clap::Parser;
use clap::Subcommand;
use thiserror::Error;

/// Top-level `renzu` command.
#[derive(Debug, Parser)]
#[command(name = "renzu")]
#[command(about = "Renzu developer CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Available `renzu` subcommands.
///
/// Every planned subcommand is listed as a variant so `--help` surfaces
/// the full command surface immediately. `scenario` was implemented by
/// F-016, `load-test` by F-017, and `report` by F-018.
#[derive(Debug, Subcommand)]
enum Command {
    /// Bring up the local observability environment.
    Up {
        /// Path to the compose file.
        ///
        /// Defaults to `compose.yaml` in the current working directory, or
        /// the value of the `RENZU_COMPOSE_FILE` environment variable.
        #[arg(long)]
        compose_file: Option<PathBuf>,
    },
    /// Show the status of the local environment.
    Status {
        /// Path to the compose file.
        ///
        /// Defaults to `compose.yaml` in the current working directory, or
        /// the value of the `RENZU_COMPOSE_FILE` environment variable.
        #[arg(long)]
        compose_file: Option<PathBuf>,
    },
    /// Tear down the local environment.
    Down {
        /// Path to the compose file.
        ///
        /// Defaults to `compose.yaml` in the current working directory, or
        /// the value of the `RENZU_COMPOSE_FILE` environment variable.
        #[arg(long)]
        compose_file: Option<PathBuf>,
    },
    /// Manage scenarios.
    #[command(subcommand)]
    Scenario(Scenario),
    /// Run a load test against a scenario.
    LoadTest {
        /// Name of the scenario to load test.
        #[arg(long)]
        scenario: String,
        /// Test duration in seconds (1-3600).
        #[arg(long, default_value = "10")]
        duration: u64,
        /// Requests per second (mutually exclusive with --concurrency).
        #[arg(long, conflicts_with = "concurrency")]
        rate: Option<u64>,
        /// Number of concurrent clients (mutually exclusive with --rate).
        #[arg(long, conflicts_with = "rate")]
        concurrency: Option<u64>,
        /// Target URL for each request.
        #[arg(long, default_value = "http://127.0.0.1:8080/health")]
        url: String,
    },
    /// Show a report for a run.
    Report {
        /// Run identifier to report on.
        run_id: String,
    },
}

/// Scenario subcommands.
#[derive(Debug, Subcommand)]
enum Scenario {
    /// List available scenarios.
    List,
    /// Run a named scenario.
    Run {
        /// Name of the scenario to run.
        name: String,
    },
}

/// Errors produced while running the `renzu` CLI.
#[derive(Debug, Error)]
pub enum CliError {
    #[error("command not yet implemented: {0}")]
    NotImplemented(String),
    #[error("CLI usage error: {0}")]
    Usage(String),
    #[error("orchestration error: {0}")]
    Orchestration(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl CliError {
    /// Exit code associated with this error.
    pub fn exit_code(&self) -> ExitCode {
        match self {
            CliError::Usage(_) => ExitCode::from(2),
            _ => ExitCode::from(1),
        }
    }
}

/// Compose file name used when no flag or environment variable provides one.
const COMPOSE_DEFAULT_FILE: &str = "compose.yaml";

/// Environment variable overriding the default compose file path.
const COMPOSE_FILE_ENV: &str = "RENZU_COMPOSE_FILE";

/// Interval between health polls during `renzu up`.
const UP_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Total time `renzu up` waits for all services to become ready.
const UP_TIMEOUT: Duration = Duration::from_secs(60);

/// Tokens that begin the STATUS column of `docker compose ps`.
const STATE_WORDS: &[&str] = &[
    "Up",
    "Exited",
    "Restarting",
    "Starting",
    "Paused",
    "Created",
    "Dead",
    "Exit",
];

/// Health of a single compose service as observed from `docker compose ps`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Health {
    /// The service has a passing healthcheck.
    Healthy,
    /// The service is running but has no healthcheck.
    Running,
    /// The service's healthcheck is failing.
    Unhealthy,
    /// The service is not running (exited, dead, created, ...).
    Stopped,
}

impl Health {
    /// Whether the service is ready to serve traffic.
    fn is_ready(self) -> bool {
        matches!(self, Self::Healthy | Self::Running)
    }
}

impl std::fmt::Display for Health {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "healthy"),
            Self::Running => write!(f, "running"),
            Self::Unhealthy => write!(f, "unhealthy"),
            Self::Stopped => write!(f, "stopped"),
        }
    }
}

/// Observed state of one compose service.
#[derive(Debug, Clone)]
struct ServiceStatus {
    service: String,
    health: Health,
}

/// Run the `renzu` CLI with the given command-line arguments.
///
/// Returns an `ExitCode` suitable for passing to `std::process::exit`.
/// Help is written to stdout; usage errors are written to stderr.
pub fn run<I, T>(args: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    match Cli::try_parse_from(args) {
        Ok(cli) => dispatch(cli.command),
        Err(err) => {
            let _ = err.print();
            ExitCode::from(err.exit_code() as u8)
        }
    }
}

/// Dispatch a parsed command to its handler.
fn dispatch(command: Command) -> ExitCode {
    match command {
        Command::Up { compose_file } => run_or_report(|| run_up(compose_file)),
        Command::Status { compose_file } => run_or_report(|| run_status(compose_file)),
        Command::Down { compose_file } => run_or_report(|| run_down(compose_file)),
        Command::Scenario(scenario) => match scenario {
            Scenario::List => run_or_report(scenario_list),
            Scenario::Run { name } => run_or_report(|| scenario_run(&name)),
        },
        Command::LoadTest {
            scenario,
            duration,
            rate,
            concurrency,
            url,
        } => run_or_report(|| load_test(scenario, duration, rate, concurrency, url)),
        Command::Report { run_id } => run_or_report(move || report_command(run_id)),
    }
}

/// Run a command handler, reporting any error on stderr, and return its exit code.
fn run_or_report(f: impl FnOnce() -> Result<ExitCode, CliError>) -> ExitCode {
    match f() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("{err}");
            err.exit_code()
        }
    }
}

/// Resolve the compose file: CLI flag > `RENZU_COMPOSE_FILE` > `./compose.yaml`.
fn resolve_compose_file(override_: Option<&Path>) -> PathBuf {
    if let Some(path) = override_ {
        return path.to_path_buf();
    }
    if let Ok(path) = std::env::var(COMPOSE_FILE_ENV) {
        return PathBuf::from(path);
    }
    std::env::current_dir()
        .unwrap_or_default()
        .join(COMPOSE_DEFAULT_FILE)
}

/// Invoke `docker compose -f <file> <args...>`.
fn run_docker_compose(file: &Path, args: &[&str]) -> Result<std::process::Output, CliError> {
    match std::process::Command::new("docker")
        .args(["compose", "-f"])
        .arg(file)
        .args(args)
        .output()
    {
        Ok(output) => Ok(output),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(CliError::Orchestration(
            "docker compose not found: install Docker and Docker Compose".to_string(),
        )),
        Err(err) => Err(CliError::Orchestration(format!(
            "failed to invoke docker compose: {err}"
        ))),
    }
}

/// Parse the plain-text output of `docker compose ps` into service states.
fn parse_compose_ps(output: &str) -> Vec<ServiceStatus> {
    let mut services = Vec::new();
    let mut lines = output.lines();
    lines.next(); // skip header row
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        let Some(offset) = tokens
            .iter()
            .skip(2)
            .position(|token| STATE_WORDS.contains(token))
        else {
            continue;
        };
        let status_start = offset + 2;
        let service = if tokens.get(status_start.saturating_sub(1)) == Some(&"ago") {
            tokens
                .get(status_start.saturating_sub(4))
                .copied()
                .unwrap_or("?")
        } else {
            tokens
                .get(status_start.saturating_sub(2))
                .copied()
                .unwrap_or("?")
        };
        let status = tokens[status_start..].join(" ");
        let health = if status.contains("healthy") {
            Health::Healthy
        } else if status.contains("unhealthy") {
            Health::Unhealthy
        } else if status.starts_with("Up") {
            Health::Running
        } else {
            Health::Stopped
        };
        services.push(ServiceStatus {
            service: service.to_string(),
            health,
        });
    }
    services
}

/// Whether every observed service is ready.
fn all_ready(services: &[ServiceStatus]) -> bool {
    services.iter().all(|service| service.health.is_ready())
}

/// Names of services that are not ready, joined for error messages.
fn not_ready(services: &[ServiceStatus]) -> String {
    services
        .iter()
        .filter(|service| !service.health.is_ready())
        .map(|service| service.service.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Bring up the local observability environment.
///
/// Starts services with `docker compose up -d`, then polls `docker compose
/// ps` until every service is ready or the timeout elapses.
fn run_up(compose_file: Option<PathBuf>) -> Result<ExitCode, CliError> {
    let file = resolve_compose_file(compose_file.as_deref());
    let output = run_docker_compose(&file, &["up", "-d"])?;
    if !output.status.success() {
        return Err(CliError::Orchestration(format!(
            "docker compose up -d failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let deadline = Instant::now() + UP_TIMEOUT;
    loop {
        let ps = run_docker_compose(&file, &["ps"])?;
        let services = parse_compose_ps(&String::from_utf8_lossy(&ps.stdout));
        if all_ready(&services) {
            return Ok(ExitCode::from(0));
        }
        if Instant::now() >= deadline {
            return Err(CliError::Orchestration(format!(
                "environment did not become healthy in time; not ready: {}",
                not_ready(&services)
            )));
        }
        sleep(UP_POLL_INTERVAL);
    }
}

/// Show the status of the local environment.
///
/// Prints a service/health table and exits 0 only when every service is
/// ready; exits 1 otherwise or when `docker compose ps` itself fails.
fn run_status(compose_file: Option<PathBuf>) -> Result<ExitCode, CliError> {
    let file = resolve_compose_file(compose_file.as_deref());
    let output = run_docker_compose(&file, &["ps"])?;
    if !output.status.success() {
        return Err(CliError::Orchestration(format!(
            "docker compose ps failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let services = parse_compose_ps(&String::from_utf8_lossy(&output.stdout));
    println!("{:<20} HEALTH", "SERVICE");
    for service in &services {
        println!("{:<20} {}", service.service, service.health);
    }
    if all_ready(&services) {
        Ok(ExitCode::from(0))
    } else {
        Err(CliError::Orchestration(
            "one or more services are not healthy".to_string(),
        ))
    }
}

/// Tear down the local environment.
///
/// Exits 0 only when `docker compose down` reports success; a failed
/// cleanup is never reported as success.
fn run_down(compose_file: Option<PathBuf>) -> Result<ExitCode, CliError> {
    let file = resolve_compose_file(compose_file.as_deref());
    let output = run_docker_compose(&file, &["down"])?;
    if output.status.success() {
        Ok(ExitCode::from(0))
    } else {
        Err(CliError::Orchestration(format!(
            "docker compose down failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

/// List all available scenarios.
fn scenario_list() -> Result<ExitCode, CliError> {
    let registry = scenarios::default_registry();
    println!("Available scenarios:");
    for scenario in registry.scenarios() {
        let fault_desc = match &scenario.fault {
            scenarios::FaultType::None => "none (baseline)".to_string(),
            scenarios::FaultType::DatabaseLatency(d) => {
                format!("database latency ({}ms)", d.as_millis())
            }
            scenarios::FaultType::DownstreamTimeout(d) => {
                format!("downstream timeout ({}s)", d.as_secs())
            }
            scenarios::FaultType::ControlledError(msg) => {
                format!("controlled error: {msg}")
            }
            scenarios::FaultType::CascadingFailure => {
                "cascading failure (db latency + downstream timeout)".to_string()
            }
            scenarios::FaultType::CollectorUnavailable => {
                "telemetry collector unavailable (app responds, export broken)".to_string()
            }
            scenarios::FaultType::DatabaseConnectionFailure => {
                "database connection failure (gateway degrades)".to_string()
            }
        };
        println!(
            "  {:<20} - {} ({})",
            scenario.name, scenario.description, fault_desc
        );
    }
    Ok(ExitCode::from(0))
}

/// Run a named scenario against the local environment.
fn scenario_run(name: &str) -> Result<ExitCode, CliError> {
    match scenarios::runner::run_scenario(name) {
        Ok(result) => {
            if result.success {
                println!(
                    "scenario '{}' completed in {:?}",
                    result.name, result.duration
                );
                Ok(ExitCode::from(0))
            } else {
                eprintln!(
                    "scenario '{}' failed in {:?}: {}",
                    result.name,
                    result.duration,
                    result.error.unwrap_or_default()
                );
                Ok(ExitCode::from(1))
            }
        }
        Err(err) => Err(CliError::Internal(err)),
    }
}

/// Run a load test against a scenario.
///
/// Generates a unique run ID, enables the scenario's fault on the gateway,
/// generates load for the configured duration, then always disables the fault.
/// A JSON report is written to `./reports/<run-id>.json` (or
/// `$RENZU_REPORTS_DIR`) and the run ID plus results are printed to stdout.
fn load_test(
    scenario: String,
    duration_secs: u64,
    rate: Option<u64>,
    concurrency: Option<u64>,
    url: String,
) -> Result<ExitCode, CliError> {
    let workload = match (rate, concurrency) {
        (Some(rps), None) => loadgen::Workload::Rate { rps },
        (None, Some(n)) => loadgen::Workload::Concurrency { n },
        (Some(_), Some(_)) => {
            return Err(CliError::Usage(
                "--rate and --concurrency are mutually exclusive".to_string(),
            ))
        }
        (None, None) => {
            return Err(CliError::Usage(
                "either --rate or --concurrency is required".to_string(),
            ))
        }
    };

    let config = loadgen::LoadTestConfig {
        scenario: scenario.clone(),
        duration: Duration::from_secs(duration_secs),
        workload,
        url,
    };

    let mut result = match loadgen::run_load_test(&config) {
        Ok(result) => result,
        Err(err) => return Err(err.to_cli_error()),
    };

    let run_id = report::generate_run_id(&scenario);
    result.run_id = run_id.clone();

    let start_time = report::now_rfc3339();
    let experiment_report =
        report::build_report(&run_id, &result, result.completion_status, start_time);
    report::write_report(&experiment_report)
        .map_err(|e| CliError::Orchestration(format!("failed to write report: {e}")))?;

    println!("run id: {run_id}");
    println!("{}", result.render());
    Ok(ExitCode::from(0))
}

/// Read a saved experiment report and print it to stdout.
///
/// Unknown run IDs produce an [`CliError::Orchestration`] (exit 1); the
/// failure is never silently reported as success.
fn report_command(run_id: String) -> Result<ExitCode, CliError> {
    match report::read_report(&run_id) {
        Ok(report) => {
            let json = serde_json::to_string_pretty(&report)
                .map_err(|e| CliError::Internal(format!("serialize report: {e}")))?;
            println!("{json}");
            Ok(ExitCode::from(0))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err(CliError::Orchestration(format!("unknown run id: {run_id}")))
        }
        Err(err) => Err(CliError::Orchestration(format!(
            "failed to read report {run_id}: {err}"
        ))),
    }
}
