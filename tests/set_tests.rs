//------------------------------------------------------------------------------
// set_tests.rs
// Tests for multi-file set merging, cat, diff, and CLI
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use std::path::Path;
use wavetools::{
    merge_signal_maps, names_only, open_wave_file, open_wave_files, write_signals_wave_multi,
    diff_wave_sets, compare_signal_names, compare_signal_meta,
    NameOptions, SignalOutputOptions,
};

fn sorted_names(map: &wavetools::SignalMap) -> Vec<String> {
    let handle_names = names_only(map);
    let mut names: Vec<String> = handle_names.values().flatten().cloned().collect();
    names.sort();
    names.dedup();
    names
}

// ---- Merge tests ----

#[test]
fn test_merge_disjoint_signals() {
    let (_, map_clk) = open_wave_file(Path::new("tests/data/set_clk.vcd"), &NameOptions::default()).unwrap();
    let (_, map_counter) = open_wave_file(Path::new("tests/data/set_counter.vcd"), &NameOptions::default()).unwrap();

    let (merged, offsets) = merge_signal_maps(&[
        (&map_clk, "set_clk.vcd"),
        (&map_counter, "set_counter.vcd"),
    ]).unwrap();

    let names = sorted_names(&merged);
    assert_eq!(names, vec![
        "t.clk",
        "t.cyc",
        "t.the_sub.cyc",
        "t.the_sub.cyc_plus_one",
    ]);
    assert_eq!(offsets.len(), 2);
    assert_eq!(offsets[0], 0);
    assert!(offsets[1] > 0);
}

#[test]
fn test_merge_duplicate_signal_error() {
    let (_, map_clk) = open_wave_file(Path::new("tests/data/set_clk.vcd"), &NameOptions::default()).unwrap();
    let (_, map_overlap) = open_wave_file(Path::new("tests/data/set_overlap.vcd"), &NameOptions::default()).unwrap();

    let result = merge_signal_maps(&[
        (&map_clk, "set_clk.vcd"),
        (&map_overlap, "set_overlap.vcd"),
    ]);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("duplicate signal"), "Expected duplicate signal error, got: {}", err);
    assert!(err.contains("t.clk"), "Expected error to mention 't.clk', got: {}", err);
}

#[test]
fn test_merge_single_file() {
    let (_, map_single) = open_wave_file(Path::new("tests/data/counter.vcd"), &NameOptions::default()).unwrap();

    let (merged, offsets) = merge_signal_maps(&[
        (&map_single, "counter.vcd"),
    ]).unwrap();

    assert_eq!(offsets, vec![0]);
    assert_eq!(sorted_names(&merged), sorted_names(&map_single));
}

#[test]
fn test_merge_handle_offsets() {
    let (_, map_clk) = open_wave_file(Path::new("tests/data/set_clk.vcd"), &NameOptions::default()).unwrap();
    let (_, map_counter) = open_wave_file(Path::new("tests/data/set_counter.vcd"), &NameOptions::default()).unwrap();
    let (_, map_overlap) = open_wave_file(Path::new("tests/data/set_counter_modified.vcd"), &NameOptions::default()).unwrap();

    // counter and counter_modified share signal names, so merge must fail
    let result = merge_signal_maps(&[
        (&map_clk, "a"),
        (&map_counter, "b"),
        (&map_overlap, "c"),
    ]);
    assert!(result.is_err());

    // Two disjoint files: verify offsets
    let (_, offsets) = merge_signal_maps(&[
        (&map_clk, "a"),
        (&map_counter, "b"),
    ]).unwrap();
    assert_eq!(offsets[0], 0);
    assert!(offsets[1] > 0, "second file should have non-zero offset");
}

// ---- open_wave_files tests ----

#[test]
fn test_open_wave_files_disjoint() {
    let paths: Vec<&Path> = vec![
        Path::new("tests/data/set_clk.vcd"),
        Path::new("tests/data/set_counter.vcd"),
    ];
    let (readers, map, offsets) = open_wave_files(&paths, &NameOptions::default(), None).unwrap();
    assert_eq!(readers.len(), 2);
    assert_eq!(offsets.len(), 2);
    assert_eq!(sorted_names(&map), vec![
        "t.clk",
        "t.cyc",
        "t.the_sub.cyc",
        "t.the_sub.cyc_plus_one",
    ]);
}

#[test]
fn test_open_wave_files_conflict() {
    let paths: Vec<&Path> = vec![
        Path::new("tests/data/set_clk.vcd"),
        Path::new("tests/data/set_overlap.vcd"),
    ];
    let result = open_wave_files(&paths, &NameOptions::default(), None);
    match result {
        Err(e) => assert!(e.contains("duplicate signal"), "Expected duplicate signal error: {}", e),
        Ok(_) => panic!("Expected error for overlapping signals"),
    }
}

// ---- Cat multi-file tests ----

#[test]
fn test_cat_multi_names_match_single() {
    // set_clk + set_counter names should equal counter.vcd names
    let paths: Vec<&Path> = vec![
        Path::new("tests/data/set_clk.vcd"),
        Path::new("tests/data/set_counter.vcd"),
    ];
    let (_, multi_map, _) = open_wave_files(&paths, &NameOptions::default(), None).unwrap();

    let (_, single_map) = open_wave_file(Path::new("tests/data/counter.vcd"), &NameOptions::default()).unwrap();

    assert_eq!(sorted_names(&multi_map), sorted_names(&single_map));
}

#[test]
fn test_cat_multi_signals_sorted() {
    // Sorted signal output from set_clk + set_counter should match counter.vcd
    let paths: Vec<&Path> = vec![
        Path::new("tests/data/set_clk.vcd"),
        Path::new("tests/data/set_counter.vcd"),
    ];
    let (readers, map, offsets) = open_wave_files(&paths, &NameOptions::default(), None).unwrap();
    let names = names_only(&map);
    let options = SignalOutputOptions {
        time_pound: false,
        sort: true,
    };
    let mut multi_output = Vec::new();
    write_signals_wave_multi(&mut multi_output, readers, &offsets, &names, 0, None, &options)
        .unwrap();

    let (mut single_reader, single_map) =
        open_wave_file(Path::new("tests/data/counter.vcd"), &NameOptions::default()).unwrap();
    let single_names = names_only(&single_map);
    let mut single_output = Vec::new();
    wavetools::write_signals_wave(
        &mut single_output,
        &mut single_reader,
        &single_names,
        0,
        None,
        &options,
    )
    .unwrap();

    assert_eq!(
        String::from_utf8(multi_output).unwrap(),
        String::from_utf8(single_output).unwrap(),
    );
}

// ---- Diff set tests ----

#[test]
fn test_diff_set_vs_single_identical() {
    // {set_clk, set_counter} vs {counter} = no differences
    let paths1: Vec<&Path> = vec![
        Path::new("tests/data/set_clk.vcd"),
        Path::new("tests/data/set_counter.vcd"),
    ];
    let (readers1, map1, offsets1) = open_wave_files(&paths1, &NameOptions::default(), None).unwrap();

    let paths2: Vec<&Path> = vec![Path::new("tests/data/counter.vcd")];
    let (readers2, map2, offsets2) = open_wave_files(&paths2, &NameOptions::default(), None).unwrap();

    let mut output = Vec::new();
    let has_diff = diff_wave_sets(
        &mut output,
        readers1, &map1, &offsets1,
        readers2, &map2, &offsets2,
        0, None, None,
    ).unwrap();

    assert!(!has_diff, "Expected no differences, got: {}", String::from_utf8_lossy(&output));
}

#[test]
fn test_diff_set_value_difference() {
    // {set_clk, set_counter_modified} vs {counter} = difference at t=10
    let paths1: Vec<&Path> = vec![
        Path::new("tests/data/set_clk.vcd"),
        Path::new("tests/data/set_counter_modified.vcd"),
    ];
    let (readers1, map1, offsets1) = open_wave_files(&paths1, &NameOptions::default(), None).unwrap();

    let paths2: Vec<&Path> = vec![Path::new("tests/data/counter.vcd")];
    let (readers2, map2, offsets2) = open_wave_files(&paths2, &NameOptions::default(), None).unwrap();

    let mut output = Vec::new();
    let has_diff = diff_wave_sets(
        &mut output,
        readers1, &map1, &offsets1,
        readers2, &map2, &offsets2,
        0, None, None,
    ).unwrap();

    assert!(has_diff, "Expected differences");
    let output_str = String::from_utf8(output).unwrap();
    assert!(output_str.contains("t.the_sub.cyc_plus_one"), "Expected cyc_plus_one in diff output: {}", output_str);
}

#[test]
fn test_diff_set_signal_name_comparison() {
    // Verify compare_signal_names works with merged maps
    let paths1: Vec<&Path> = vec![
        Path::new("tests/data/set_clk.vcd"),
        Path::new("tests/data/set_counter.vcd"),
    ];
    let (_, map1, _) = open_wave_files(&paths1, &NameOptions::default(), None).unwrap();

    let (_, map2) = open_wave_file(Path::new("tests/data/counter.vcd"), &NameOptions::default()).unwrap();

    let (only_in_1, only_in_2) = compare_signal_names(&map1, &map2);
    assert!(only_in_1.is_empty(), "Unexpected signals only in set: {:?}", only_in_1);
    assert!(only_in_2.is_empty(), "Unexpected signals only in single: {:?}", only_in_2);
}

#[test]
fn test_diff_set_meta_comparison() {
    // Merged set should have identical meta to single file
    let paths: Vec<&Path> = vec![
        Path::new("tests/data/set_clk.vcd"),
        Path::new("tests/data/set_counter.vcd"),
    ];
    let (_, map1, _) = open_wave_files(&paths, &NameOptions::default(), None).unwrap();
    let (_, map2) = open_wave_file(Path::new("tests/data/counter.vcd"), &NameOptions::default()).unwrap();

    let diffs = compare_signal_meta(&map1, &map2);
    assert!(diffs.is_empty(), "Expected no meta diffs, got: {:?}", diffs);
}

// ---- CLI tests ----

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
fn test_cli_wavecat_multi_file_names() {
    let output = run_wavecat_cli(&[
        "--names", "--sort",
        "tests/data/set_clk.vcd",
        "tests/data/set_counter.vcd",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let names: Vec<&str> = stdout.lines().collect();
    assert_eq!(names, vec![
        "t.clk",
        "t.cyc",
        "t.the_sub.cyc",
        "t.the_sub.cyc_plus_one",
    ]);
}

#[test]
fn test_cli_wavecat_multi_file_conflict() {
    let output = run_wavecat_cli(&[
        "--names",
        "tests/data/set_clk.vcd",
        "tests/data/set_overlap.vcd",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("duplicate signal"), "Expected duplicate signal error: {}", stderr);
}

#[test]
fn test_cli_wavecat_multi_file_attrs() {
    let output = run_wavecat_cli(&[
        "--attrs", "--sort",
        "tests/data/set_clk.vcd",
        "tests/data/set_counter.vcd",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("t.clk"));
    assert!(stdout.contains("t.cyc"));
}

#[test]
fn test_cli_wavediff_set_no_diff() {
    // {set_clk + set_counter} vs {counter} = identical, exit 0
    let output = run_wavediff_cli(&[
        "--set1", "tests/data/set_counter.vcd",
        "tests/data/set_clk.vcd",
        "tests/data/counter.vcd",
    ]);
    assert!(output.status.success(), "Expected exit 0, stderr: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn test_cli_wavediff_set_value_diff() {
    // {set_clk + set_counter_modified} vs {counter} = difference, exit 1
    let output = run_wavediff_cli(&[
        "--set1", "tests/data/set_counter_modified.vcd",
        "tests/data/set_clk.vcd",
        "tests/data/counter.vcd",
    ]);
    assert_eq!(output.status.code(), Some(1), "Expected exit 1");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("t.the_sub.cyc_plus_one"));
}

#[test]
fn test_cli_wavediff_set_conflict() {
    // set_clk + set_overlap in set1 = duplicate signal, exit 2
    let output = run_wavediff_cli(&[
        "--set1", "tests/data/set_overlap.vcd",
        "tests/data/set_clk.vcd",
        "tests/data/counter.vcd",
    ]);
    assert_eq!(output.status.code(), Some(2), "Expected exit 2");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("duplicate signal"), "Expected duplicate signal error: {}", stderr);
}

#[test]
fn test_cli_wavediff_no_sets_unchanged() {
    // Without --set1/--set2, existing behavior unchanged
    let output = run_wavediff_cli(&[
        "tests/data/counter.vcd",
        "tests/data/counter.vcd",
    ]);
    assert!(output.status.success(), "Expected exit 0 for identical files");
}

#[test]
fn test_cli_wavediff_set2_only() {
    // Only --set2 with positional args
    let output = run_wavediff_cli(&[
        "--set2", "tests/data/set_counter.vcd",
        "tests/data/counter.vcd",
        "tests/data/set_clk.vcd",
    ]);
    assert!(output.status.success(), "Expected exit 0, stderr: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn test_cli_wavediff_sets_only_no_positional() {
    // Both --set1 and --set2, no positional args
    let output = run_wavediff_cli(&[
        "--set1", "tests/data/set_clk.vcd",
        "--set1", "tests/data/set_counter.vcd",
        "--set2", "tests/data/counter.vcd",
    ]);
    assert!(output.status.success(), "Expected exit 0, stderr: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn test_cli_wavediff_sets_only_value_diff() {
    // Both --set1 and --set2 with no positional args, difference detected
    let output = run_wavediff_cli(&[
        "--set1", "tests/data/set_clk.vcd",
        "--set1", "tests/data/set_counter_modified.vcd",
        "--set2", "tests/data/counter.vcd",
    ]);
    assert_eq!(output.status.code(), Some(1), "Expected exit 1");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("t.the_sub.cyc_plus_one"));
}

#[test]
fn test_cli_wavediff_set1_only_no_positional_fails() {
    // Only --set1 without positional args should fail
    let output = run_wavediff_cli(&[
        "--set1", "tests/data/counter.vcd",
    ]);
    assert_eq!(output.status.code(), Some(2), "Expected exit 2");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("FILE1 and FILE2 are required"), "Expected missing args error: {}", stderr);
}

#[test]
fn test_cli_wavediff_no_args_fails() {
    let output = run_wavediff_cli(&[]);
    assert_eq!(output.status.code(), Some(2), "Expected exit 2");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("FILE1 and FILE2 are required"), "Expected missing args error: {}", stderr);
}

#[test]
fn test_cli_wavecat_multi_file_filter() {
    // Filter should work with multi-file
    let output = run_wavecat_cli(&[
        "--names", "--filter", "*.clk",
        "tests/data/set_clk.vcd",
        "tests/data/set_counter.vcd",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let names: Vec<&str> = stdout.lines().collect();
    assert_eq!(names, vec!["t.clk"]);
}
