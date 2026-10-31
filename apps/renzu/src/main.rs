use std::process::ExitCode;

use renzu::run;

fn main() -> ExitCode {
    run(std::env::args())
}
