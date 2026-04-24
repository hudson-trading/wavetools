//------------------------------------------------------------------------------
// common/mod.rs
// Shared test helpers for CLI integration tests
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

pub fn run_wavecat_cli(args: &[&str]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_wavecat");
    std::process::Command::new(bin)
        .args(args)
        .output()
        .expect("Failed to run wavecat")
}

pub fn run_wavediff_cli(args: &[&str]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_wavediff");
    std::process::Command::new(bin)
        .args(args)
        .output()
        .expect("Failed to run wavediff")
}
