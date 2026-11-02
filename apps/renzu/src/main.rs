use std::process::ExitCode;

use renzu::run;
use renzu::telemetry::Telemetry;

fn main() -> ExitCode {
    let _telemetry = Telemetry::init("info").expect("telemetry should initialize");
    run(std::env::args())
}
