//------------------------------------------------------------------------------
// attr_tests.rs
// Tests for cross-format attribute comparison (FST vs VCD)
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use std::path::Path;
use wavetools::{compare_signal_meta, open_wave_file, NameOptions};

/// Helper: get signal attrs by signal name from a file
fn get_signal_attrs(path: &str, signal_name: &str) -> Vec<String> {
    let (_, signal_map) = open_wave_file(Path::new(path), &NameOptions::default()).unwrap();
    for info in signal_map.values() {
        for var in &info.vars {
            if var.name == signal_name {
                return var.attrs.clone();
            }
        }
    }
    panic!("Signal '{}' not found in {}", signal_name, path);
}

// ---- VCD structural attribute parsing ----

#[test]
fn test_vcd_array_packed_attr() {
    let attrs = get_signal_attrs("tests/data/struct_attrs.vcd", "top.arrp [2:1]");
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0], "array packed: bounds 17179869187");
}

#[test]
fn test_vcd_array_unpacked_attr() {
    let attrs = get_signal_attrs("tests/data/struct_attrs.vcd", "top.v_real");
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0], "array unpacked: bounds 1");
}

#[test]
fn test_vcd_class_packed_attr() {
    let attrs = get_signal_attrs("tests/data/struct_attrs.vcd", "top.cyc");
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0], "class packed: members 2");
}

#[test]
fn test_vcd_enum_attr_ordering() {
    // Enum attrs should have value_name=encoding format (e.g. IDLE=00)
    let attrs = get_signal_attrs("tests/data/struct_attrs.vcd", "top.state");
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0], "enum state_t: IDLE=00 ACTIVE=01 DONE=10");
}

#[test]
fn test_vcd_no_attr_signal() {
    let attrs = get_signal_attrs("tests/data/struct_attrs.vcd", "top.clk");
    assert!(attrs.is_empty(), "clk should have no attributes");
}

// ---- Enum ordering (value_name=encoding convention) ----

#[test]
fn test_enum_attr_format_vcd() {
    // VCD enum: value names first, then encodings
    // "state_t 3 IDLE ACTIVE DONE 0 1 2" → IDLE=0 ACTIVE=1 DONE=2
    let attrs = get_signal_attrs("tests/data/enum_attrs.a.vcd", "top.state");
    assert_eq!(attrs.len(), 1);
    assert!(
        attrs[0].contains("IDLE=0"),
        "Expected value_name=encoding format, got: {}",
        attrs[0]
    );
    assert!(
        !attrs[0].contains("0=IDLE"),
        "Should not have encoding=value_name format, got: {}",
        attrs[0]
    );
}

// ---- Cross-format meta comparison ----

#[test]
fn test_meta_self_comparison_vcd() {
    // A file compared to itself should have no meta diffs
    let (_, map) =
        open_wave_file(Path::new("tests/data/struct_attrs.vcd"), &NameOptions::default()).unwrap();
    let diffs = compare_signal_meta(&map, &map);
    assert!(diffs.is_empty(), "Self-comparison should have no diffs: {:?}", diffs);
}

#[test]
fn test_meta_comparison_enum_files() {
    // enum_attrs.a.vcd and enum_attrs.b.vcd should differ in enum attrs
    let (_, map_a) =
        open_wave_file(Path::new("tests/data/enum_attrs.a.vcd"), &NameOptions::default()).unwrap();
    let (_, map_b) =
        open_wave_file(Path::new("tests/data/enum_attrs.b.vcd"), &NameOptions::default()).unwrap();
    let diffs = compare_signal_meta(&map_a, &map_b);
    // They should differ (enum_attrs.b has different enum mapping)
    assert!(!diffs.is_empty(), "Expected meta diffs between a and b enum files");
}

// ---- CLI tests for struct attrs ----

fn run_wavecat_cli(args: &[&str]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_wavecat");
    std::process::Command::new(bin)
        .args(args)
        .output()
        .expect("Failed to run wavecat")
}

fn run_wavediff_cli(args: &[&str]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_wavediff");
    std::process::Command::new(bin)
        .args(args)
        .output()
        .expect("Failed to run wavediff")
}

#[test]
fn test_cli_struct_attrs_output() {
    let output = run_wavecat_cli(&["--attrs", "--sort", "tests/data/struct_attrs.vcd"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("array packed: bounds 17179869187"));
    assert!(stdout.contains("array unpacked: bounds 1"));
    assert!(stdout.contains("class packed: members 2"));
    assert!(stdout.contains("enum state_t: IDLE=00 ACTIVE=01 DONE=10"));
}

#[test]
fn test_cli_struct_attrs_self_diff() {
    // Diffing struct_attrs.vcd against itself should produce no differences
    let output = run_wavediff_cli(&[
        "tests/data/struct_attrs.vcd",
        "tests/data/struct_attrs.vcd",
    ]);
    assert!(
        output.status.success(),
        "Expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
