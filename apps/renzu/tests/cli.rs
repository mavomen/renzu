use renzu::run;
use std::process::ExitCode;

#[test]
fn help_exits_zero() {
    assert_eq!(run(["renzu", "--help"]), ExitCode::from(0));
}

#[test]
fn no_subcommand_exits_non_zero() {
    assert_ne!(run(["renzu"]), ExitCode::from(0));
}

#[test]
fn unknown_subcommand_exits_non_zero() {
    assert_ne!(run(["renzu", "bogus"]), ExitCode::from(0));
}

#[test]
fn invalid_arg_exits_non_zero() {
    assert_ne!(run(["renzu", "up", "--bogus"]), ExitCode::from(0));
}

#[test]
fn all_stub_subcommands_exit_zero() {
    for args in [
        vec!["renzu", "up"],
        vec!["renzu", "status"],
        vec!["renzu", "down"],
        vec!["renzu", "scenario", "list"],
        vec!["renzu", "scenario", "run", "name"],
        vec!["renzu", "load-test", "--scenario", "name"],
        vec!["renzu", "report", "run-id"],
    ] {
        assert_eq!(run(args), ExitCode::from(0));
    }
}

#[test]
fn stub_subcommand_prints_not_implemented_to_stderr() {
    let bin = std::env::var("CARGO_BIN_EXE_renzu")
        .expect("CARGO_BIN_EXE_renzu is set by cargo for integration tests");
    let output = std::process::Command::new(bin)
        .arg("up")
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("renzu up: not yet implemented"));
}
