//! Resource and telemetry overhead analysis for load test runs.
//!
//! Measures process CPU time and memory (VmRSS) before and after a load test
//! run, reporting resource deltas alongside throughput and error rate.
//!
//! # Measurement model
//!
//! CPU time is read from `/proc/self/stat` (utime + stime, in clock ticks,
//! converted using `CLK_TCK`). Resident set size is read from
//! `/proc/self/status` (`VmRSS`, in kB). Both are Linux-specific.
//!
//! # Limitations
//!
//! - Linux-only (`/proc` filesystem).
//! - Measures the `renzu` CLI process itself, not separate gateway/orders
//!   services. Service-side overhead requires instrumenting those binaries.
//! - Telemetry on/off comparison is not implemented; this command measures
//!   overhead with telemetry enabled (the current default). A future
//!   telemetry toggle in the service would enable direct comparison.

use crate::loadgen::{run_load_test, LoadTestConfig, Workload};
use std::fs;
use std::io;
use std::time::Duration;

/// Default number of clock ticks per second on Linux (`CLK_TCK`).
const CLK_TCK: f64 = 100.0;

/// A snapshot of process resource usage at a point in time.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ResourceSample {
    /// CPU user time in seconds.
    pub cpu_user_seconds: f64,
    /// CPU system time in seconds.
    pub cpu_system_seconds: f64,
    /// Resident set size in kilobytes.
    pub vm_rss_kb: u64,
    /// Wall-clock time the sample was taken (seconds since Unix epoch).
    pub timestamp_secs: f64,
}

/// Result of measuring overhead for a load test run.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OverheadResult {
    /// Scenario name.
    pub scenario: String,
    /// Test duration in seconds.
    pub duration_secs: u64,
    /// Requests per second (if rate-limited).
    pub rate_rps: Option<u64>,
    /// Concurrent clients (if concurrency-limited).
    pub concurrency: Option<u64>,
    /// Target URL.
    pub url: String,
    /// CPU user-time delta in seconds (after - before).
    pub cpu_user_delta_seconds: f64,
    /// CPU system-time delta in seconds (after - before).
    pub cpu_system_delta_seconds: f64,
    /// VmRSS delta in kilobytes (after - before).
    pub vm_rss_delta_kb: i64,
    /// Total requests made.
    pub total_requests: u64,
    /// Successful requests.
    pub successful: u64,
    /// Failed requests.
    pub failed: u64,
    /// Observed throughput in requests per second.
    pub throughput_rps: f64,
    /// Observed error rate as a percentage.
    pub error_rate_percent: f64,
}

/// Read the current process resource usage from `/proc`.
///
/// Returns an error if `/proc/self/stat` or `/proc/self/status` cannot be
/// read or parsed.
pub fn read_resource_sample() -> io::Result<ResourceSample> {
    let stat = fs::read_to_string("/proc/self/stat")?;
    let status = fs::read_to_string("/proc/self/status")?;

    let cpu = parse_proc_stat_cpu(&stat)?;
    let vm_rss = parse_proc_status_vmrss(&status)?;
    let timestamp = now_epoch_secs();

    Ok(ResourceSample {
        cpu_user_seconds: cpu.0,
        cpu_system_seconds: cpu.1,
        vm_rss_kb: vm_rss,
        timestamp_secs: timestamp,
    })
}

/// Parse CPU user/system time from `/proc/self/stat`.
///
/// The stat line fields (space-separated, with commname possibly containing
/// spaces or parens): pid, comm, state, ppid, pgrp, session, tty_nr,
/// tpgid, flags, minflt, cminflt, majflt, cmajflt, utime, stime, ...
/// utime is field 14 (1-indexed), stime is field 15.
///
/// Returns `(utime_seconds, stime_seconds)`.
fn parse_proc_stat_cpu(stat: &str) -> io::Result<(f64, f64)> {
    // Find the closing parenthesis of comm; everything after is fixed fields.
    let after_paren = stat
        .rfind(')')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing comm parens"))?;
    let rest = &stat[after_paren + 2..]; // skip ") "
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // Fields after comm: state, ppid, pgrp, session, tty_nr, tpgid, flags,
    // minflt, cminflt, majflt, cmajflt, utime(12th), stime(13th)
    let utime_idx = 12;
    let stime_idx = 13;
    if fields.len() <= stime_idx {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not enough fields in /proc/self/stat",
        ));
    }
    let utime_ticks: f64 = fields[utime_idx].parse().map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse utime: {e}"),
        )
    })?;
    let stime_ticks: f64 = fields[stime_idx].parse().map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse stime: {e}"),
        )
    })?;
    Ok((utime_ticks / CLK_TCK, stime_ticks / CLK_TCK))
}

/// Parse VmRSS in kB from `/proc/self/status`.
fn parse_proc_status_vmrss(status: &str) -> io::Result<u64> {
    for line in status.lines() {
        if line.starts_with("VmRSS:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "malformed VmRSS line",
                ));
            }
            let kb: u64 = parts[1].parse().map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("failed to parse VmRSS: {e}"),
                )
            })?;
            return Ok(kb);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "VmRSS not found in /proc/self/status",
    ))
}

/// Current wall-clock time in seconds since the Unix epoch.
fn now_epoch_secs() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Measure resource overhead for a load test run.
///
/// Samples `/proc/self/stat` and `/proc/self/status` before and after
/// executing the load test, then computes deltas.
pub fn measure_overhead(
    scenario: &str,
    duration_secs: u64,
    rate: Option<u64>,
    concurrency: Option<u64>,
    url: &str,
) -> io::Result<OverheadResult> {
    let before = read_resource_sample()?;

    let workload = match (rate, concurrency) {
        (Some(rps), None) => Workload::Rate { rps },
        (None, Some(n)) => Workload::Concurrency { n },
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "exactly one of --rate or --concurrency must be provided",
            ))
        }
    };

    let config = LoadTestConfig {
        scenario: scenario.to_string(),
        duration: Duration::from_secs(duration_secs),
        workload,
        url: url.to_string(),
    };

    let result =
        run_load_test(&config).map_err(|e| io::Error::other(format!("load test failed: {e}")))?;

    let after = read_resource_sample()?;

    let total = result.total;
    let failed = result.failed;
    let error_rate = if total > 0 {
        failed as f64 / total as f64 * 100.0
    } else {
        0.0
    };

    Ok(OverheadResult {
        scenario: scenario.to_string(),
        duration_secs,
        rate_rps: rate,
        concurrency,
        url: url.to_string(),
        cpu_user_delta_seconds: after.cpu_user_seconds - before.cpu_user_seconds,
        cpu_system_delta_seconds: after.cpu_system_seconds - before.cpu_system_seconds,
        vm_rss_delta_kb: after.vm_rss_kb as i64 - before.vm_rss_kb as i64,
        total_requests: total,
        successful: result.successful,
        failed,
        throughput_rps: result.throughput_rps,
        error_rate_percent: error_rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proc_stat_cpu_valid_line() {
        // Synthetic /proc/self/stat line. Fields after comm:
        // state ppid pgrp session tty_nr tpgid flags minflt cminflt majflt
        // cmajflt utime stime ...
        let stat =
            "1 (comm) S 1 1 1 0 -1 4194304 1 1 1 1 1 100 200 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        let (utime, stime) = parse_proc_stat_cpu(stat).unwrap();
        assert_eq!(utime, 100.0 / CLK_TCK);
        assert_eq!(stime, 200.0 / CLK_TCK);
    }

    #[test]
    fn parse_proc_stat_cpu_missing_parens() {
        let stat = "1 comm S 1 1 1 0 -1 4194304 1 1 1 1 1 100 200";
        let err = parse_proc_stat_cpu(stat).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn parse_proc_stat_cpu_not_enough_fields() {
        let stat = "1 (comm) S";
        let err = parse_proc_stat_cpu(stat).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn parse_proc_status_vmrss_valid() {
        let status = "Name:   comm\nVmRSS:     12345 kB\nVmSize:  67890 kB\n";
        let rss = parse_proc_status_vmrss(status).unwrap();
        assert_eq!(rss, 12345);
    }

    #[test]
    fn parse_proc_status_vmrss_missing() {
        let status = "Name:   comm\nVmSize:  67890 kB\n";
        let err = parse_proc_status_vmrss(status).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn parse_proc_status_vmrss_malformed() {
        let status = "VmRSS: notanumber kB\n";
        let err = parse_proc_status_vmrss(status).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn error_rate_zero_total() {
        let total: u64 = 0;
        let failed: u64 = 0;
        let error_rate = if total > 0 {
            failed as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        assert_eq!(error_rate, 0.0);
    }

    #[test]
    fn error_rate_partial() {
        let total: u64 = 100;
        let failed: u64 = 5;
        let error_rate = failed as f64 / total as f64 * 100.0;
        assert_eq!(error_rate, 5.0);
    }
}
