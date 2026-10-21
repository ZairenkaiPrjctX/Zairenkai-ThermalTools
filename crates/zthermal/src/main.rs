// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! `zthermal`: read-only thermal diagnostics, configuration analysis and policy planning.
//!
//! Exit codes: 0 success, 1 runtime error, 2 usage error, 3 validation problems found,
//! 4 refused by the safety policy, 5 not available on this device.

mod args;
mod commands;
mod render;

use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let code = match args::parse(&argv) {
        Ok(a) => commands::run(&a),
        Err(e) => {
            eprintln!("zthermal: {e}\n\n{}", args::USAGE);
            commands::EXIT_USAGE
        }
    };
    ExitCode::from(code)
}
