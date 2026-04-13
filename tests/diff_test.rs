//------------------------------------------------------------------------------
// diff_test.rs
// Tests for waveform diffing
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use wavetools::{compare_signal_names, diff_waves, open_and_read_waves, NameOptions};

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
