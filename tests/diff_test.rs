//------------------------------------------------------------------------------
// diff_test.rs
// Tests for waveform diffing
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use wavetools::{compare_signal_meta, compare_signal_names, diff_waves, open_and_read_waves, NameOptions};

// Helper to check signal name differences
fn check_signal_names(file1: &str, file2: &str) -> (bool, String) {
    let name_options = NameOptions::default();
    let (_reader1, handle_to_names1, _reader2, handle_to_names2) =
        open_and_read_waves(file1, file2, &name_options)
            .expect("Failed to open wave files");

    let (only_in_1, only_in_2) = compare_signal_names(&handle_to_names1, &handle_to_names2);

    let has_differences = !only_in_1.is_empty() || !only_in_2.is_empty();
    let mut msg = String::new();
    if has_differences {
        if !only_in_1.is_empty() {
            msg.push_str(&format!("Only in {}: {:?}\n", file1, only_in_1));
        }
        if !only_in_2.is_empty() {
            msg.push_str(&format!("Only in {}: {:?}\n", file2, only_in_2));
        }
    }
    (has_differences, msg)
}

#[test]
fn test_diff_identical_files() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.fst",
    );
    assert!(!has_diff, "Identical files should have no differences");
    assert_eq!(output.len(), 0, "No output expected for identical files");
}

#[test]
fn test_diff_end_time() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.end_time.diff.fst",
    );
    assert!(has_diff, "counter.end_time.diff.fst should differ from counter.fst");

    let expected = "\
50 t.clk 1 (missing time in file2)
";
    assert_eq!(output, expected, "Expected exact diff output");
}

// Tests for files that should NOT differ from counter.fst

#[test]
fn test_change_reorder_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.change_reorder.no_diff.fst",
    );
    assert!(!has_diff, "counter.change_reorder.no_diff.fst should not differ. Output:\n{}", output);
}

#[test]
fn test_identifier_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.identifier.no_diff.fst",
    );
    assert!(!has_diff, "counter.identifier.no_diff.fst should not differ. Output:\n{}", output);
}

#[test]
fn test_scope_move_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.scope_move.no_diff.fst",
    );
    assert!(!has_diff, "counter.scope_move.no_diff.fst should not differ. Output:\n{}", output);
}

#[test]
fn test_time_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.time.no_diff.fst",
    );
    assert!(!has_diff, "counter.time.no_diff.fst should not differ. Output:\n{}", output);
}

#[test]
fn test_var_reorder_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.var_reorder.no_diff.fst",
    );
    assert!(!has_diff, "counter.var_reorder.no_diff.fst should not differ. Output:\n{}", output);
}

#[test]
fn test_shared_handle_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.shared_handle.no_diff.fst",
    );
    assert!(!has_diff, "counter.shared_handle.no_diff.fst should not differ. Output:\n{}", output);
}

#[test]
fn test_shared_handle_reverse_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.shared_handle.no_diff.fst",
        "tests/data/counter.fst",
    );
    assert!(!has_diff, "counter.fst should not differ when compared in reverse. Output:\n{}", output);
}

// Tests for files that SHOULD differ from counter.fst

#[test]
fn test_edge_time_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.edge_time.diff.fst",
    );
    assert!(has_diff, "counter.edge_time.diff.fst should differ from counter.fst");

    // The only difference should be: time 20 in FST1 vs time 21 in FST2
    let expected = "\
20 t.clk 0 (missing time in file2)
21 t.clk 0 (only in file2)
";
    assert_eq!(output, expected, "Expected exact diff output");
}

#[test]
fn test_new_sig_diff() {
    let (has_name_diff, msg) = check_signal_names(
        "tests/data/counter.fst",
        "tests/data/counter.new_sig.diff.fst",
    );
    assert!(has_name_diff, "counter.new_sig.diff.fst should have different signal names");

    let expected = "\
Only in tests/data/counter.new_sig.diff.fst: {\"t.the_sub.new_sig\"}
";
    assert_eq!(msg, expected, "Expected exact signal name difference");
}

#[test]
fn test_sig_name_diff() {
    let (has_name_diff, msg) = check_signal_names(
        "tests/data/counter.fst",
        "tests/data/counter.sig_name.diff.fst",
    );
    assert!(has_name_diff, "counter.sig_name.diff.fst should have different signal names");

    let expected = "\
Only in tests/data/counter.fst: {\"t.the_sub.cyc_plus_one\"}
Only in tests/data/counter.sig_name.diff.fst: {\"t.the_sub.blargh\"}
";
    assert_eq!(msg, expected, "Expected exact signal name difference");
}

#[test]
fn test_value_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
    );
    assert!(has_diff, "counter.value.diff.fst should differ from counter.fst");

    let expected = "\
10 t.the_sub.cyc_plus_one 00000000000000000000000000000010 != 00000000000000000000000000000100
";
    assert_eq!(output, expected, "Expected exact diff output");
}

// ── VCD and cross-format tests ────────────────────────────────────────────────

fn run_wave_diff_test(file1: &str, file2: &str) -> (bool, String) {
    let name_options = NameOptions::default();
    let (reader1, handle_to_names1, reader2, handle_to_names2) =
        open_and_read_waves(file1, file2, &name_options)
            .expect("Failed to open wave files");

    let mut output = Vec::new();
    let has_differences = diff_waves(
        &mut output,
        reader1,
        &handle_to_names1,
        reader2,
        &handle_to_names2,
        0,
        None,
        None,
    )
    .expect("Failed to diff files");

    let output_str = String::from_utf8(output).expect("Invalid UTF-8");
    (has_differences, output_str)
}

#[test]
fn test_diff_vcd_identical() {
    let (has_diff, output) =
        run_wave_diff_test("tests/data/counter.vcd", "tests/data/counter.vcd");
    assert!(!has_diff, "Identical VCD files should have no differences");
    assert_eq!(output.len(), 0);
}

#[test]
fn test_diff_cross_format_fst_vcd() {
    let (has_diff, output) =
        run_wave_diff_test("tests/data/counter.fst", "tests/data/counter.vcd");
    assert!(!has_diff, "FST and equivalent VCD should have no differences. Output:\n{}", output);
}

#[test]
fn test_diff_cross_format_vcd_fst() {
    let (has_diff, output) =
        run_wave_diff_test("tests/data/counter.vcd", "tests/data/counter.fst");
    assert!(!has_diff, "VCD and equivalent FST should have no differences. Output:\n{}", output);
}

#[test]
fn test_diff_vcd_value_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.vcd",
        "tests/data/counter.value.diff.vcd",
    );
    assert!(has_diff, "counter.value.diff.vcd should differ from counter.vcd");
    let expected = "\
10 t.the_sub.cyc_plus_one 00000000000000000000000000000010 != 00000000000000000000000000000100
";
    assert_eq!(output, expected, "Expected exact diff output for VCD value diff");
}

#[test]
fn test_diff_vcd_end_time() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.vcd",
        "tests/data/counter.end_time.diff.vcd",
    );
    assert!(has_diff, "counter.end_time.diff.vcd should differ from counter.vcd");
    let expected = "\
50 t.clk 1 (missing time in file2)
";
    assert_eq!(output, expected, "Expected exact diff output for VCD end time diff");
}

// ── Real epsilon tests ───────────────────────────────────────────────────────

fn run_wave_diff_test_with_epsilon(
    file1: &str,
    file2: &str,
    real_epsilon: Option<f64>,
) -> (bool, String) {
    let name_options = NameOptions::default();
    let (reader1, handle_to_names1, reader2, handle_to_names2) =
        open_and_read_waves(file1, file2, &name_options)
            .expect("Failed to open wave files");

    let mut output = Vec::new();
    let has_differences = diff_waves(
        &mut output,
        reader1,
        &handle_to_names1,
        reader2,
        &handle_to_names2,
        0,
        None,
        real_epsilon,
    )
    .expect("Failed to diff files");

    let output_str = String::from_utf8(output).expect("Invalid UTF-8");
    (has_differences, output_str)
}

#[test]
fn test_diff_real_no_epsilon_reports_diff() {
    let (has_diff, output) = run_wave_diff_test_with_epsilon(
        "tests/data/real_base.vcd",
        "tests/data/real_close.vcd",
        None,
    );
    assert!(has_diff, "Without epsilon, close real values should differ. Output:\n{}", output);
}

#[test]
fn test_diff_real_within_epsilon_no_diff() {
    let (has_diff, output) = run_wave_diff_test_with_epsilon(
        "tests/data/real_base.vcd",
        "tests/data/real_close.vcd",
        Some(0.001),
    );
    assert!(!has_diff, "Within epsilon, close real values should not differ. Output:\n{}", output);
}

#[test]
fn test_diff_real_outside_epsilon_reports_diff() {
    let (has_diff, output) = run_wave_diff_test_with_epsilon(
        "tests/data/real_base.vcd",
        "tests/data/real_far.vcd",
        Some(0.001),
    );
    assert!(has_diff, "Outside epsilon, far real values should differ. Output:\n{}", output);
}

#[test]
fn test_diff_real_large_epsilon_no_diff() {
    let (has_diff, output) = run_wave_diff_test_with_epsilon(
        "tests/data/real_base.vcd",
        "tests/data/real_far.vcd",
        Some(1.0),
    );
    assert!(!has_diff, "With large epsilon, even far real values should not differ. Output:\n{}", output);
}

// ── VCD id code aliasing tests ───────────────────────────────────────────────
// Reproducer for a vcddiff bug: when one file aliases signals (multiple signals
// sharing the same VCD id code) and the other assigns unique ids, vcddiff's
// per-code mapping array overwrites earlier entries, causing "Never found"
// false positives.  wavediff matches by signal name and handles this correctly.

#[test]
fn test_diff_vcd_aliased_idcodes_no_diff() {
    // idcode_a.vcd: signals a,b (code !) and c,d (code ") share ids across scopes
    // idcode_b.vcd: every signal gets a unique id (0–4)
    // Signal names and values are identical — only the id codes differ.
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/idcode_a.vcd",
        "tests/data/idcode_b.vcd",
    );
    assert!(!has_diff, "Files with aliased vs unique VCD id codes should not differ. Output:\n{}", output);
}

#[test]
fn test_diff_vcd_aliased_idcodes_reverse_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/idcode_b.vcd",
        "tests/data/idcode_a.vcd",
    );
    assert!(!has_diff, "Reversed aliased vs unique VCD id codes should not differ. Output:\n{}", output);
}

// ── Time range filtering tests ──────────────────────────────────────────────

fn run_wave_diff_test_with_range(
    file1: &str,
    file2: &str,
    start: u64,
    end: Option<u64>,
) -> (bool, String) {
    let name_options = NameOptions::default();
    let (reader1, handle_to_names1, reader2, handle_to_names2) =
        open_and_read_waves(file1, file2, &name_options)
            .expect("Failed to open wave files");

    let mut output = Vec::new();
    let has_differences = diff_waves(
        &mut output,
        reader1,
        &handle_to_names1,
        reader2,
        &handle_to_names2,
        start,
        end,
        None,
    )
    .expect("Failed to diff files");

    let output_str = String::from_utf8(output).expect("Invalid UTF-8");
    (has_differences, output_str)
}

#[test]
fn test_diff_start_skips_early_difference() {
    // counter.value.diff differs only at time 10
    let (has_diff, output) = run_wave_diff_test_with_range(
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
        20,
        None,
    );
    assert!(!has_diff, "Starting at time 20 should skip the time-10 difference. Output:\n{}", output);
}

#[test]
fn test_diff_end_skips_late_difference() {
    // counter.end_time.diff differs only at time 50
    let (has_diff, output) = run_wave_diff_test_with_range(
        "tests/data/counter.fst",
        "tests/data/counter.end_time.diff.fst",
        0,
        Some(40),
    );
    assert!(!has_diff, "Ending at time 40 should skip the time-50 difference. Output:\n{}", output);
}

#[test]
fn test_diff_start_and_end_skip_difference() {
    // counter.edge_time.diff differs at times 20 and 21
    let (has_diff, output) = run_wave_diff_test_with_range(
        "tests/data/counter.fst",
        "tests/data/counter.edge_time.diff.fst",
        30,
        Some(50),
    );
    assert!(!has_diff, "Range 30-50 should skip differences at times 20-21. Output:\n{}", output);
}

#[test]
fn test_diff_start_beyond_all_data() {
    let (has_diff, output) = run_wave_diff_test_with_range(
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
        1000,
        None,
    );
    assert!(!has_diff, "Starting beyond all data should show no differences. Output:\n{}", output);
}

#[test]
fn test_diff_vcd_start_skips_early_difference() {
    let (has_diff, output) = run_wave_diff_test_with_range(
        "tests/data/counter.vcd",
        "tests/data/counter.value.diff.vcd",
        20,
        None,
    );
    assert!(!has_diff, "VCD: starting at time 20 should skip the time-10 difference. Output:\n{}", output);
}

#[test]
fn test_diff_vcd_end_skips_late_difference() {
    let (has_diff, output) = run_wave_diff_test_with_range(
        "tests/data/counter.vcd",
        "tests/data/counter.end_time.diff.vcd",
        0,
        Some(40),
    );
    assert!(!has_diff, "VCD: ending at time 40 should skip the time-50 difference. Output:\n{}", output);
}

// ── Additional epsilon edge cases ───────────────────────────────────────────

#[test]
fn test_diff_zero_epsilon() {
    // Zero epsilon should require exact match, same as no epsilon
    let (has_diff, output) = run_wave_diff_test_with_epsilon(
        "tests/data/real_base.vcd",
        "tests/data/real_close.vcd",
        Some(0.0),
    );
    assert!(has_diff, "Zero epsilon should require exact match. Output:\n{}", output);
}

// ── Metadata comparison tests ──────────────────────────────────────────────

#[test]
fn test_diff_type_mismatch() {
    let name_options = NameOptions::default();
    let (_r1, map1, _r2, map2) =
        open_and_read_waves(
            "tests/data/type_mismatch.a.vcd",
            "tests/data/type_mismatch.b.vcd",
            &name_options,
        )
        .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&map1, &map2);
    assert!(!diffs.is_empty(), "Should detect type mismatches");

    // clk: wire vs reg
    assert!(
        diffs.iter().any(|d| d.contains("top.clk") && d.contains("wire") && d.contains("reg")),
        "Should detect clk type mismatch: {:?}",
        diffs
    );
    // state: wire vs reg
    assert!(
        diffs.iter().any(|d| d.contains("top.state") && d.contains("wire") && d.contains("reg")),
        "Should detect state type mismatch: {:?}",
        diffs
    );
}

#[test]
fn test_diff_size_mismatch() {
    let name_options = NameOptions::default();
    let (_r1, map1, _r2, map2) =
        open_and_read_waves(
            "tests/data/type_mismatch.a.vcd",
            "tests/data/type_mismatch.b.vcd",
            &name_options,
        )
        .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&map1, &map2);

    // data: size 8 vs 16
    assert!(
        diffs.iter().any(|d| d.contains("top.data") && d.contains("8") && d.contains("16")),
        "Should detect data size mismatch: {:?}",
        diffs
    );
}

#[test]
fn test_diff_identical_metadata() {
    let name_options = NameOptions::default();
    let (_r1, map1, _r2, map2) =
        open_and_read_waves(
            "tests/data/type_mismatch.a.vcd",
            "tests/data/type_mismatch.a.vcd",
            &name_options,
        )
        .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&map1, &map2);
    assert!(diffs.is_empty(), "Same file should have no metadata diffs: {:?}", diffs);
}

#[test]
fn test_diff_cross_format_metadata() {
    // FST and VCD of the same design may have different var types (FST preserves
    // original types like "reg"/"integer" while VCD might use "wire"). Direction
    // comparison should be skipped since VCD has no direction info ("implicit").
    let name_options = NameOptions::default();
    let (_r1, map1, _r2, map2) =
        open_and_read_waves(
            "tests/data/counter.fst",
            "tests/data/counter.vcd",
            &name_options,
        )
        .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&map1, &map2);
    // Direction diffs should NOT appear since VCD direction is "implicit"
    assert!(
        !diffs.iter().any(|d| d.contains("direction")),
        "Should not report direction diffs when VCD side is implicit: {:?}",
        diffs
    );
}

// ── Attribute comparison tests ──────────────────────────────────────────────

#[test]
fn test_diff_enum_attr_difference() {
    // Same signal names/types/values, but different enum table attributes:
    //   a: state has enum state_t (IDLE/ACTIVE/DONE)
    //   b: state has enum alt_state_t (OFF/ON/ERR)
    let name_options = NameOptions::default();
    let (_r1, map1, _r2, map2) =
        open_and_read_waves(
            "tests/data/enum_attrs.a.vcd",
            "tests/data/enum_attrs.b.vcd",
            &name_options,
        )
        .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&map1, &map2);
    assert!(
        diffs.iter().any(|d| d.contains("top.state")),
        "Should detect enum attribute difference on top.state: {:?}",
        diffs
    );
}

#[test]
fn test_diff_misc_attr_difference() {
    // Same signal names/types/values, but different misc attributes:
    //   a: data has source path /path/to/source.v
    //   b: data has source path /different/path.v
    let name_options = NameOptions::default();
    let (_r1, map1, _r2, map2) =
        open_and_read_waves(
            "tests/data/enum_attrs.a.vcd",
            "tests/data/enum_attrs.b.vcd",
            &name_options,
        )
        .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&map1, &map2);
    assert!(
        diffs.iter().any(|d| d.contains("top.data")),
        "Should detect misc attribute difference on top.data: {:?}",
        diffs
    );
}

#[test]
fn test_diff_attr_present_vs_absent() {
    // a: state has enum attr, data has source path attr
    // missing: no attrs on any signal
    let name_options = NameOptions::default();
    let (_r1, map1, _r2, map2) =
        open_and_read_waves(
            "tests/data/enum_attrs.a.vcd",
            "tests/data/enum_attrs.missing.vcd",
            &name_options,
        )
        .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&map1, &map2);
    assert!(
        diffs.iter().any(|d| d.contains("top.state")),
        "Should detect missing enum attr on top.state: {:?}",
        diffs
    );
    assert!(
        diffs.iter().any(|d| d.contains("top.data")),
        "Should detect missing source attr on top.data: {:?}",
        diffs
    );
}

#[test]
fn test_diff_identical_attrs_no_diff() {
    // Same file compared to itself — no attr differences
    let name_options = NameOptions::default();
    let (_r1, map1, _r2, map2) =
        open_and_read_waves(
            "tests/data/enum_attrs.a.vcd",
            "tests/data/enum_attrs.a.vcd",
            &name_options,
        )
        .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&map1, &map2);
    assert!(diffs.is_empty(), "Same file should have no diffs: {:?}", diffs);
}

#[test]
fn test_diff_real_size_normalized_across_formats() {
    // FST stores real signal sizes in bytes (8), VCD in bits (64).
    // After normalization both should report 64 — no size mismatch.
    let name_options = NameOptions::default();
    let (_r1, map1, _r2, map2) =
        open_and_read_waves(
            "tests/data/real_base.fst",
            "tests/data/real_base.vcd",
            &name_options,
        )
        .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&map1, &map2);
    assert!(
        diffs.is_empty(),
        "FST and VCD of the same design should have no metadata diffs: {:?}",
        diffs
    );
}

// ── --no-attrs CLI tests ────────────────────────────────────────────────────

fn run_wavediff_cli(file1: &str, file2: &str, extra_args: &[&str]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_wavediff");
    std::process::Command::new(bin)
        .args(extra_args)
        .arg(file1)
        .arg(file2)
        .output()
        .expect("Failed to run wavediff")
}

#[test]
fn test_cli_attr_diff_nonzero_exit() {
    // Different attrs, same values — should exit 1
    let output = run_wavediff_cli(
        "tests/data/enum_attrs.a.vcd",
        "tests/data/enum_attrs.b.vcd",
        &[],
    );
    assert_eq!(output.status.code(), Some(1), "Attr diffs should cause exit 1");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("top.state"), "stderr should mention top.state: {}", stderr);
    assert!(stderr.contains("top.data"), "stderr should mention top.data: {}", stderr);
}

#[test]
fn test_cli_no_attrs_ignores_attr_diff() {
    // Different attrs, same values — --no-attrs should make it exit 0
    let output = run_wavediff_cli(
        "tests/data/enum_attrs.a.vcd",
        "tests/data/enum_attrs.b.vcd",
        &["--no-attrs"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "--no-attrs should ignore attr differences. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_cli_no_attrs_ignores_missing_attrs() {
    // Attrs in one file, none in the other — --no-attrs should exit 0
    let output = run_wavediff_cli(
        "tests/data/enum_attrs.a.vcd",
        "tests/data/enum_attrs.missing.vcd",
        &["--no-attrs"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "--no-attrs should ignore missing attrs. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_cli_no_attrs_still_detects_value_diffs() {
    // --no-attrs skips metadata but should still catch value differences
    let output = run_wavediff_cli(
        "tests/data/counter.vcd",
        "tests/data/counter.value.diff.vcd",
        &["--no-attrs"],
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "--no-attrs should still detect value diffs"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cyc_plus_one"),
        "--no-attrs stdout should contain value diff: {}",
        stdout
    );
}
