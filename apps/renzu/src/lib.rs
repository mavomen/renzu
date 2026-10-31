//! `renzu` command-line application skeleton.
//!
//! Provides `clap`-based argument parsing, top-level help, command and
//! error-handling conventions, and stable exit codes. Subcommands are
//! stubs that print "not yet implemented" until F-015 through F-018
//! implement real handlers.

use std::process::ExitCode;

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
/// the full command surface immediately. Subcommands are stubs until
/// F-015-F-018 implement real handlers.
#[derive(Debug, Subcommand)]
enum Command {
    /// Bring up the local observability environment.
    Up,
    /// Show the status of the local environment.
    Status,
    /// Tear down the local environment.
    Down,
    /// Manage scenarios.
    #[command(subcommand)]
    Scenario(Scenario),
    /// Run a load test against a scenario.
    LoadTest {
        /// Name of the scenario to load test.
        #[arg(long)]
        scenario: String,
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
        Command::Up => run_stub("up"),
        Command::Status => run_stub("status"),
        Command::Down => run_stub("down"),
        Command::Scenario(scenario) => match scenario {
            Scenario::List => run_stub("scenario list"),
            Scenario::Run { name } => run_stub(&format!("scenario run {name}")),
        },
        Command::LoadTest { scenario } => run_stub(&format!("load-test --scenario {scenario}")),
        Command::Report { run_id } => run_stub(&format!("report {run_id}")),
    }
}

/// Run a stub subcommand.
///
/// Prints `renzu <command>: not yet implemented` to stderr and exits 0.
fn run_stub(command: &str) -> ExitCode {
    eprintln!("renzu {command}: not yet implemented");
    ExitCode::from(0)
}
