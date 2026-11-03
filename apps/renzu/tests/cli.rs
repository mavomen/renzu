use renzu::run;
use std::collections::HashSet;
use std::fs;
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
fn scenario_subcommands_exit_codes_match_behavior() {
    // `scenario list` succeeds and prints known scenario names to stdout.
    let list = run(["renzu", "scenario", "list"]);
    assert_eq!(list, ExitCode::from(0));

    // `scenario run` with an unknown scenario fails with exit 1.
    let run_unknown = run(["renzu", "scenario", "run", "no-such-scenario"]);
    assert_eq!(run_unknown, ExitCode::from(1));

    // `load-test` requires --rate or --concurrency before scenario validation;
    // an unknown scenario with a valid workload fails with exit 2 (Usage).
    let load_unknown = run([
        "renzu",
        "load-test",
        "--scenario",
        "no-such-scenario",
        "--rate",
        "1",
    ]);
    assert_eq!(load_unknown, ExitCode::from(2));
}

#[test]
fn scenario_list_prints_names_to_stdout_with_empty_stderr() {
    let bin = std::env::var("CARGO_BIN_EXE_renzu")
        .expect("CARGO_BIN_EXE_renzu is set by cargo for integration tests");
    let output = std::process::Command::new(&bin)
        .arg("scenario")
        .arg("list")
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("baseline"),
        "stdout missing scenario: {stdout}"
    );
    assert!(
        stdout.contains("otel-collector-down"),
        "stdout missing scenario: {stdout}"
    );
    assert!(
        stdout.contains("db-connection-failure"),
        "stdout missing scenario: {stdout}"
    );
}

#[test]
fn report_unknown_run_id_exits_one() {
    let output = renzu()
        .arg("report")
        .arg("does-not-exist")
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown run id: does-not-exist"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn report_reads_saved_report() {
    let run_id = "report-test-run";
    let reports_dir = workspace_root().join("reports");
    fs::create_dir_all(&reports_dir).expect("create reports dir");
    let report_path = reports_dir.join(format!("{run_id}.json"));
    let json = r#"{"run_id":"report-test-run","scenario":"baseline","start_time":"2026-01-01T00:00:00Z","duration_secs":10,"completion_status":"complete","workload":{"scenario":"baseline","duration_secs":10,"kind":"rate","value":20,"url":"http://127.0.0.1:8080/health"},"counts":{"total":100,"successful":95,"failed":5},"latency":{"count":100,"min_ms":1.0,"max_ms":50.0,"mean_ms":10.0,"p50_ms":9.0,"p95_ms":20.0,"p99_ms":40.0},"throughput_rps":10.0,"telemetry_references":[],"warnings":["missing telemetry: trace IDs are not collected in F-018"]}"#;
    fs::write(&report_path, json).expect("write report fixture");
    let output = renzu()
        .arg("report")
        .arg(run_id)
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(run_id), "stdout missing run id: {stdout}");
    assert!(
        stdout.contains("baseline"),
        "stdout missing scenario: {stdout}"
    );
    let _ = fs::remove_file(&report_path);
    let _ = fs::remove_dir(&reports_dir);
}

#[test]
fn compare_unknown_run_id_exits_one() {
    let output = renzu()
        .arg("compare")
        .arg("does-not-exist")
        .arg("also-missing")
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown run id: does-not-exist"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn compare_prints_delta_table() {
    let run_a = "compare-left";
    let run_b = "compare-right";
    let reports_dir = workspace_root().join("reports");
    fs::create_dir_all(&reports_dir).expect("create reports dir");
    let write = |run_id: &str,
                 scenario: &str,
                 kind: &str,
                 value: u64,
                 throughput: f64,
                 total: u64| {
        let path = reports_dir.join(format!("{run_id}.json"));
        let json = format!(
            r#"{{
  "run_id": "{run_id}",
  "scenario": "{scenario}",
  "start_time": "2026-01-01T00:00:00Z",
  "duration_secs": 10,
  "completion_status": "complete",
  "workload": {{"scenario": "{scenario}", "duration_secs": 10, "kind": "{kind}", "value": {value}, "url": "http://127.0.0.1:8080/health"}},
  "counts": {{"total": {total}, "successful": {total}, "failed": 0}},
  "latency": {{"count": {total}, "min_ms": 1.0, "max_ms": 10.0, "mean_ms": 5.0, "p50_ms": 5.0, "p95_ms": 9.0, "p99_ms": 10.0}},
  "throughput_rps": {throughput},
  "telemetry_references": [],
  "warnings": []
}}"#
        );
        fs::write(&path, json).expect("write report fixture");
    };
    write(run_a, "baseline", "rate", 20, 10.0, 100);
    write(run_b, "baseline", "rate", 20, 12.0, 120);

    let output = renzu()
        .arg("compare")
        .arg(run_a)
        .arg(run_b)
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Performance comparison"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains(run_a), "stdout missing run a: {stdout}");
    assert!(stdout.contains(run_b), "stdout missing run b: {stdout}");
    assert!(
        stdout.contains("baseline"),
        "stdout missing scenario: {stdout}"
    );
    assert!(
        stdout.contains("+20.00%"),
        "stdout missing throughput delta: {stdout}"
    );
    assert!(
        !stdout.contains("incompatible"),
        "unexpected incompatible warning: {stdout}"
    );

    let _ = fs::remove_file(reports_dir.join(format!("{run_a}.json")));
    let _ = fs::remove_file(reports_dir.join(format!("{run_b}.json")));
    let _ = fs::remove_dir(&reports_dir);
}

#[test]
fn compare_flags_incompatible_runs() {
    let run_a = "compare-left-incompat";
    let run_b = "compare-right-incompat";
    let reports_dir = workspace_root().join("reports");
    fs::create_dir_all(&reports_dir).expect("create reports dir");
    let write = |run_id: &str,
                 scenario: &str,
                 kind: &str,
                 value: u64,
                 throughput: f64,
                 total: u64| {
        let path = reports_dir.join(format!("{run_id}.json"));
        let json = format!(
            r#"{{
  "run_id": "{run_id}",
  "scenario": "{scenario}",
  "start_time": "2026-01-01T00:00:00Z",
  "duration_secs": 10,
  "completion_status": "complete",
  "workload": {{"scenario": "{scenario}", "duration_secs": 10, "kind": "{kind}", "value": {value}, "url": "http://127.0.0.1:8080/health"}},
  "counts": {{"total": {total}, "successful": {total}, "failed": 0}},
  "latency": {{"count": {total}, "min_ms": 1.0, "max_ms": 10.0, "mean_ms": 5.0, "p50_ms": 5.0, "p95_ms": 9.0, "p99_ms": 10.0}},
  "throughput_rps": {throughput},
  "telemetry_references": [],
  "warnings": []
}}"#
        );
        fs::write(&path, json).expect("write report fixture");
    };
    write(run_a, "baseline", "rate", 20, 10.0, 100);
    write(run_b, "faulted", "rate", 20, 8.0, 80);

    let output = renzu()
        .arg("compare")
        .arg(run_a)
        .arg(run_b)
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("incompatible"),
        "stdout missing incompatible warning: {stdout}"
    );
    assert!(
        stdout.contains("WARNING"),
        "stdout missing warning prefix: {stdout}"
    );
    assert!(
        stdout.contains("scenario differs"),
        "stdout missing scenario reason: {stdout}"
    );
    assert!(stdout.contains(run_a), "stdout missing run a: {stdout}");
    assert!(stdout.contains(run_b), "stdout missing run b: {stdout}");

    let _ = fs::remove_file(reports_dir.join(format!("{run_a}.json")));
    let _ = fs::remove_file(reports_dir.join(format!("{run_b}.json")));
    let _ = fs::remove_dir(&reports_dir);
}

fn docker_compose_available() -> bool {
    let docker_version = std::process::Command::new("docker")
        .arg("version")
        .arg("--format")
        .arg("{{.Server.Version}}")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !docker_version {
        return false;
    }

    let compose_config = std::process::Command::new("docker")
        .arg("compose")
        .arg("config")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !compose_config {
        return false;
    }

    let output = match std::process::Command::new("docker")
        .arg("compose")
        .arg("config")
        .arg("--format")
        .arg("json")
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };

    if !output.status.success() {
        return false;
    }

    let config: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let services = match config.get("services").and_then(|s| s.as_object()) {
        Some(s) => s,
        None => return false,
    };

    let images_output = match std::process::Command::new("docker")
        .arg("images")
        .arg("--format")
        .arg("{{.Repository}}:{{.Tag}}")
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };

    let available_images: HashSet<String> = String::from_utf8_lossy(&images_output.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect();

    for (_, service) in services {
        if let Some(build) = service.get("build") {
            let image_name = if let Some(ctx) = build.get("context").and_then(|c| c.as_str()) {
                let dockerfile = build
                    .get("dockerfile")
                    .and_then(|d| d.as_str())
                    .unwrap_or("Dockerfile");
                let path = std::path::Path::new(ctx).join(dockerfile);
                if path.exists() {
                    let name = std::path::Path::new(ctx)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");
                    format!("renzu-{name}:latest")
                } else {
                    continue;
                }
            } else {
                continue;
            };
            if !available_images.contains(&image_name) {
                return false;
            }
        } else if let Some(image) = service.get("image").and_then(|i| i.as_str()) {
            if !available_images.contains(image) {
                return false;
            }
        }
    }

    true
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

#[test]
fn otel_collector_down_scenario_repro_and_recovery() {
    if !docker_compose_available() {
        eprintln!("skipping otel-collector-down scenario test: docker compose is not available");
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

    // The telemetry collector is unavailable: the app still responds, so the
    // scenario completes successfully (exit 0).
    let run = renzu()
        .arg("scenario")
        .arg("run")
        .arg("otel-collector-down")
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(
        run.status.code(),
        Some(0),
        "otel-collector-down scenario failed: {}",
        String::from_utf8_lossy(&run.stderr)
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
fn db_connection_failure_scenario_repro_and_recovery() {
    if !docker_compose_available() {
        eprintln!("skipping db-connection-failure scenario test: docker compose is not available");
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

    // The database connection is down: the gateway degrades but still
    // responds, so the scenario completes successfully (exit 0).
    let run = renzu()
        .arg("scenario")
        .arg("run")
        .arg("db-connection-failure")
        .output()
        .expect("failed to execute renzu binary");
    assert_eq!(
        run.status.code(),
        Some(0),
        "db-connection-failure scenario failed: {}",
        String::from_utf8_lossy(&run.stderr)
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
