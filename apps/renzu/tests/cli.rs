use renzu::run;
use std::path::PathBuf;
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
fn stub_subcommands_exit_zero() {
    for args in [
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
    let output = std::process::Command::new(&bin)
        .arg("scenario")
        .arg("list")
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("renzu scenario list: not yet implemented"));
}

fn docker_compose_available() -> bool {
    std::process::Command::new("docker")
        .arg("compose")
        .arg("version")
        .output()
        .is_ok()
}

fn renzu() -> std::process::Command {
    let bin = std::env::var("CARGO_BIN_EXE_renzu")
        .expect("CARGO_BIN_EXE_renzu is set by cargo for integration tests");
    let mut cmd = std::process::Command::new(bin);
    cmd.current_dir(workspace_root());
    cmd
}

/// Workspace root, where `compose.yaml` lives relative to the renzu crate.
fn workspace_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR is set for integration tests");
    PathBuf::from(manifest_dir).join("../..")
}

#[test]
fn up_status_down_lifecycle() {
    if !docker_compose_available() {
        eprintln!("skipping up/status/down lifecycle test: docker compose is not available");
        return;
    }
    let up = renzu()
        .arg("up")
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(
        up.status.code(),
        Some(0),
        "renzu up failed: {}",
        String::from_utf8_lossy(&up.stderr)
    );
    let status = renzu()
        .arg("status")
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(status.status.code(), Some(0));
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(
        status_out.contains("jaeger"),
        "status output missing jaeger: {status_out}"
    );
    assert!(
        status_out.contains("otel-collector"),
        "status output missing otel-collector: {status_out}"
    );
    let down = renzu()
        .arg("down")
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(
        down.status.code(),
        Some(0),
        "renzu down failed: {}",
        String::from_utf8_lossy(&down.stderr)
    );
}

#[test]
fn up_with_missing_compose_file_exits_one() {
    if !docker_compose_available() {
        eprintln!("skipping compose-file error test: docker compose is not available");
        return;
    }
    let output = renzu()
        .arg("up")
        .arg("--compose-file")
        .arg("no-such-compose-file.yaml")
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(output.status.code(), Some(1));
}
