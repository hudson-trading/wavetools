//------------------------------------------------------------------------------
// diff_test.rs
// Tests for waveform diffing
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use wavetools::{
    compare_signal_meta, compare_signal_names, diff_waves, open_and_read_waves,
    retain_common_signals, DiffOptions, NameOptions,
};

// Helper to check signal name differences
fn check_signal_names(file1: &str, file2: &str) -> (bool, String) {
    let name_options = NameOptions::default();
    let (_reader1, hier1, _reader2, hier2) =
        open_and_read_waves(file1, file2, &name_options).expect("Failed to open wave files");

    let (only_in_1, only_in_2) = compare_signal_names(&hier1, &hier2);

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
    let (has_diff, output) = run_wave_diff_test("tests/data/counter.fst", "tests/data/counter.fst");
    assert!(!has_diff, "Identical files should have no differences");
    assert_eq!(output.len(), 0, "No output expected for identical files");
}

#[test]
fn test_diff_end_time() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.end_time.diff.fst",
    );
    assert!(
        !has_diff,
        "Trailing samples beyond the shorter file should be ignored. Output:\n{}",
        output
    );
    assert_eq!(
        output, "",
        "Ignored trailing samples should not produce diff output"
    );
}

// Tests for files that should NOT differ from counter.fst

#[test]
fn test_change_reorder_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.change_reorder.no_diff.fst",
    );
    assert!(
        !has_diff,
        "counter.change_reorder.no_diff.fst should not differ. Output:\n{}",
        output
    );
}

#[test]
fn test_identifier_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.identifier.no_diff.fst",
    );
    assert!(
        !has_diff,
        "counter.identifier.no_diff.fst should not differ. Output:\n{}",
        output
    );
}

#[test]
fn test_scope_move_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.scope_move.no_diff.fst",
    );
    assert!(
        !has_diff,
        "counter.scope_move.no_diff.fst should not differ. Output:\n{}",
        output
    );
}

#[test]
fn test_time_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.time.no_diff.fst",
    );
    assert!(
        !has_diff,
        "counter.time.no_diff.fst should not differ. Output:\n{}",
        output
    );
}

#[test]
fn test_var_reorder_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.var_reorder.no_diff.fst",
    );
    assert!(
        !has_diff,
        "counter.var_reorder.no_diff.fst should not differ. Output:\n{}",
        output
    );
}

#[test]
fn test_shared_handle_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.shared_handle.no_diff.fst",
    );
    assert!(
        !has_diff,
        "counter.shared_handle.no_diff.fst should not differ. Output:\n{}",
        output
    );
}

#[test]
fn test_shared_handle_reverse_no_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.shared_handle.no_diff.fst",
        "tests/data/counter.fst",
    );
    assert!(
        !has_diff,
        "counter.fst should not differ when compared in reverse. Output:\n{}",
        output
    );
}

// Tests for files that SHOULD differ from counter.fst

#[test]
fn test_edge_time_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.edge_time.diff.fst",
    );
    assert!(
        has_diff,
        "counter.edge_time.diff.fst should differ from counter.fst"
    );

    // The only difference should be: time 20 in FST1 vs time 21 in FST2
    let expected = "\
20 t.clk 0 (missing time in file2)
21 t.clk 0 (only in file2)
";
    assert_eq!(output, expected, "Expected exact diff output");
}

#[test]
fn test_buffered_file2_only_diffs_are_sorted() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/buffered_file2_base.vcd",
        "tests/data/buffered_file2_extra.vcd",
    );
    assert!(has_diff, "file2-only intermediate changes should differ");

    let expected = "\
10 t.a 1 (only in file2)
10 t.b 1 (only in file2)
10 t.c 1 (only in file2)
";
    assert_eq!(
        output, expected,
        "Expected deterministic file2-only diff output"
    );
}

#[test]
fn test_report_timestamps_are_monotonic_across_buffered_file2_only_rows() {
    let pid = std::process::id();
    let file1 = std::env::temp_dir().join(format!("wavetools-monotonic-{}-1.vcd", pid));
    let file2 = std::env::temp_dir().join(format!("wavetools-monotonic-{}-2.vcd", pid));
    std::fs::write(
        &file1,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 1 ! a $end
$var wire 1 \" b $end
$upscope $end
$enddefinitions $end
#0
0!
0\"
#20
1!
",
    )
    .expect("write file1");
    std::fs::write(
        &file2,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 1 ! a $end
$var wire 1 \" b $end
$upscope $end
$enddefinitions $end
#0
0!
0\"
#10
1\"
#30
1!
",
    )
    .expect("write file2");

    let (has_diff, output) = run_wave_diff_test(
        file1.to_str().expect("temp path should be UTF-8"),
        file2.to_str().expect("temp path should be UTF-8"),
    );
    assert!(
        has_diff,
        "expected buffered file2-only row and missing time"
    );

    let expected = "\
10 t.b 1 (only in file2)
20 t.a 1 (missing time in file2)
";
    assert_eq!(output, expected, "diff output should be time-sorted");

    let _ = std::fs::remove_file(file1);
    let _ = std::fs::remove_file(file2);
}

#[test]
fn test_terminal_same_tick_file1_only_changes_are_trimmed() {
    let pid = std::process::id();
    let file1 = std::env::temp_dir().join(format!("wavetools-terminal-trim-{}-1.vcd", pid));
    let file2 = std::env::temp_dir().join(format!("wavetools-terminal-trim-{}-2.vcd", pid));
    std::fs::write(
        &file1,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 1 ! clk $end
$var wire 1 \" a $end
$upscope $end
$enddefinitions $end
#0
0!
0\"
#10
1!
1\"
",
    )
    .expect("write file1");
    std::fs::write(
        &file2,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 1 ! clk $end
$var wire 1 \" a $end
$upscope $end
$enddefinitions $end
#0
0!
0\"
#10
1!
",
    )
    .expect("write file2");

    let (has_diff, output) = run_wave_diff_test(
        file1.to_str().expect("temp path should be UTF-8"),
        file2.to_str().expect("temp path should be UTF-8"),
    );
    assert!(
        !has_diff,
        "terminal file1-only changes should be trim-only. Output:\n{}",
        output
    );
    assert_eq!(output, "");

    let _ = std::fs::remove_file(file1);
    let _ = std::fs::remove_file(file2);
}

#[test]
fn test_terminal_same_tick_file2_only_changes_are_trimmed() {
    let pid = std::process::id();
    let file1 = std::env::temp_dir().join(format!("wavetools-terminal-trim-{}-a.vcd", pid));
    let file2 = std::env::temp_dir().join(format!("wavetools-terminal-trim-{}-b.vcd", pid));
    std::fs::write(
        &file1,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 1 ! clk $end
$var wire 1 \" a $end
$upscope $end
$enddefinitions $end
#0
0!
0\"
#10
1!
",
    )
    .expect("write file1");
    std::fs::write(
        &file2,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 1 ! clk $end
$var wire 1 \" a $end
$upscope $end
$enddefinitions $end
#0
0!
0\"
#10
1!
1\"
",
    )
    .expect("write file2");

    let (has_diff, output) = run_wave_diff_test(
        file1.to_str().expect("temp path should be UTF-8"),
        file2.to_str().expect("temp path should be UTF-8"),
    );
    assert!(
        !has_diff,
        "terminal file2-only changes should be trim-only. Output:\n{}",
        output
    );
    assert_eq!(output, "");

    let _ = std::fs::remove_file(file1);
    let _ = std::fs::remove_file(file2);
}

#[test]
fn test_new_sig_diff() {
    let (has_name_diff, msg) = check_signal_names(
        "tests/data/counter.fst",
        "tests/data/counter.new_sig.diff.fst",
    );
    assert!(
        has_name_diff,
        "counter.new_sig.diff.fst should have different signal names"
    );

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
    assert!(
        has_name_diff,
        "counter.sig_name.diff.fst should have different signal names"
    );

    let expected = "\
Only in tests/data/counter.fst: {\"t.the_sub.cyc_plus_one\"}
Only in tests/data/counter.sig_name.diff.fst: {\"t.the_sub.blargh\"}
";
    assert_eq!(msg, expected, "Expected exact signal name difference");
}

#[test]
fn test_retain_common_signals_drops_asymmetric_names() {
    let name_options = NameOptions::default();
    let (_reader1, mut hier1, _reader2, mut hier2) = open_and_read_waves(
        "tests/data/counter.fst",
        "tests/data/counter.new_sig.diff.fst",
        &name_options,
    )
    .expect("Failed to open wave files");

    let common_count = retain_common_signals(&mut hier1, &mut hier2);
    assert!(common_count > 0, "Expected at least one common signal");

    let (only_in_1, only_in_2) = compare_signal_names(&hier1, &hier2);
    assert!(
        only_in_1.is_empty(),
        "Unexpected file1-only signals: {:?}",
        only_in_1
    );
    assert!(
        only_in_2.is_empty(),
        "Unexpected file2-only signals: {:?}",
        only_in_2
    );
}

#[test]
fn test_value_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
    );
    assert!(
        has_diff,
        "counter.value.diff.fst should differ from counter.fst"
    );

    let expected = "\
10 t.the_sub.cyc_plus_one 00000000000000000000000000000010 != 00000000000000000000000000000100
";
    assert_eq!(output, expected, "Expected exact diff output");
}

// -- VCD and cross-format tests -----------------------------------------------

fn run_wave_diff_test(file1: &str, file2: &str) -> (bool, String) {
    let name_options = NameOptions::default();
    let (reader1, hier1, reader2, hier2) =
        open_and_read_waves(file1, file2, &name_options).expect("Failed to open wave files");

    let mut output = Vec::new();
    let options = DiffOptions {
        start: 0,
        end: None,
        real_epsilon: None,
    };
    let has_differences = diff_waves(&mut output, reader1, hier1, reader2, hier2, &options)
        .expect("Failed to diff files");

    let output_str = String::from_utf8(output).expect("Invalid UTF-8");
    (has_differences, output_str)
}

#[test]
fn test_diff_vcd_identical() {
    let (has_diff, output) = run_wave_diff_test("tests/data/counter.vcd", "tests/data/counter.vcd");
    assert!(!has_diff, "Identical VCD files should have no differences");
    assert_eq!(output.len(), 0);
}

#[test]
fn test_diff_cross_format_fst_vcd() {
    let (has_diff, output) = run_wave_diff_test("tests/data/counter.fst", "tests/data/counter.vcd");
    assert!(
        !has_diff,
        "FST and equivalent VCD should have no differences. Output:\n{}",
        output
    );
}

#[test]
fn test_diff_cross_format_vcd_fst() {
    let (has_diff, output) = run_wave_diff_test("tests/data/counter.vcd", "tests/data/counter.fst");
    assert!(
        !has_diff,
        "VCD and equivalent FST should have no differences. Output:\n{}",
        output
    );
}

#[test]
fn test_diff_vcd_value_diff() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.vcd",
        "tests/data/counter.value.diff.vcd",
    );
    assert!(
        has_diff,
        "counter.value.diff.vcd should differ from counter.vcd"
    );
    let expected = "\
10 t.the_sub.cyc_plus_one 00000000000000000000000000000010 != 00000000000000000000000000000100
";
    assert_eq!(
        output, expected,
        "Expected exact diff output for VCD value diff"
    );
}

#[test]
fn test_diff_vcd_end_time() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/counter.vcd",
        "tests/data/counter.end_time.diff.vcd",
    );
    assert!(
        !has_diff,
        "Trailing samples beyond the shorter file should be ignored. Output:\n{}",
        output
    );
    assert_eq!(
        output, "",
        "Ignored trailing samples should not produce diff output"
    );
}

// -- Real epsilon tests -------------------------------------------------------

fn run_wave_diff_test_with_epsilon(
    file1: &str,
    file2: &str,
    real_epsilon: Option<f64>,
) -> (bool, String) {
    let name_options = NameOptions::default();
    let (reader1, hier1, reader2, hier2) =
        open_and_read_waves(file1, file2, &name_options).expect("Failed to open wave files");

    let mut output = Vec::new();
    let options = DiffOptions {
        start: 0,
        end: None,
        real_epsilon,
    };
    let has_differences = diff_waves(&mut output, reader1, hier1, reader2, hier2, &options)
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
    assert!(
        has_diff,
        "Without epsilon, close real values should differ. Output:\n{}",
        output
    );
}

#[test]
fn test_diff_real_within_epsilon_no_diff() {
    let (has_diff, output) = run_wave_diff_test_with_epsilon(
        "tests/data/real_base.vcd",
        "tests/data/real_close.vcd",
        Some(0.001),
    );
    assert!(
        !has_diff,
        "Within epsilon, close real values should not differ. Output:\n{}",
        output
    );
}

#[test]
fn test_diff_real_outside_epsilon_reports_diff() {
    let (has_diff, output) = run_wave_diff_test_with_epsilon(
        "tests/data/real_base.vcd",
        "tests/data/real_far.vcd",
        Some(0.001),
    );
    assert!(
        has_diff,
        "Outside epsilon, far real values should differ. Output:\n{}",
        output
    );
}

#[test]
fn test_diff_real_large_epsilon_no_diff() {
    let (has_diff, output) = run_wave_diff_test_with_epsilon(
        "tests/data/real_base.vcd",
        "tests/data/real_far.vcd",
        Some(1.0),
    );
    assert!(
        !has_diff,
        "With large epsilon, even far real values should not differ. Output:\n{}",
        output
    );
}

// -- VCD id code aliasing tests -----------------------------------------------
// Reproducer for a vcddiff bug: when one file aliases signals (multiple signals
// sharing the same VCD id code) and the other assigns unique ids, vcddiff's
// per-code mapping array overwrites earlier entries, causing "Never found"
// false positives.  wavediff matches by signal name and handles this correctly.

#[test]
fn test_diff_vcd_aliased_idcodes_no_diff() {
    // idcode_a.vcd: signals a,b (code !) and c,d (code ") share ids across scopes
    // idcode_b.vcd: every signal gets a unique id (0-4)
    // Signal names and values are identical -- only the id codes differ.
    let (has_diff, output) =
        run_wave_diff_test("tests/data/idcode_a.vcd", "tests/data/idcode_b.vcd");
    assert!(
        !has_diff,
        "Files with aliased vs unique VCD id codes should not differ. Output:\n{}",
        output
    );
}

#[test]
fn test_diff_vcd_aliased_idcodes_reverse_no_diff() {
    let (has_diff, output) =
        run_wave_diff_test("tests/data/idcode_b.vcd", "tests/data/idcode_a.vcd");
    assert!(
        !has_diff,
        "Reversed aliased vs unique VCD id codes should not differ. Output:\n{}",
        output
    );
}

#[test]
fn test_diff_vcd_aliased_idcodes_reports_each_name_once() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/idcode_a.vcd",
        "tests/data/error/idcode_a_value_diff.vcd",
    );
    assert!(has_diff, "Changed aliased id should differ");
    let expected = "\
0 m.s0.a 0 != 1
0 m.s1.c 0 != 1
";
    assert_eq!(
        output, expected,
        "Expected one diff per aliased signal name"
    );
}

#[test]
fn test_diff_aliased_to_split_idcodes_reports_matched_name_once() {
    let (has_diff, output) = run_wave_diff_test(
        "tests/data/diff/alias_split_a.vcd",
        "tests/data/error/alias_split_b_value_diff.vcd",
    );
    assert!(has_diff, "Changed split alias should differ");
    let expected = "\
0 m.s1.c 0 != 1
";
    assert_eq!(
        output, expected,
        "Expected only the split signal name to be reported"
    );
}

// -- Time range filtering tests -----------------------------------------------

fn run_wave_diff_test_with_range(
    file1: &str,
    file2: &str,
    start: u64,
    end: Option<u64>,
) -> (bool, String) {
    let name_options = NameOptions::default();
    let (reader1, hier1, reader2, hier2) =
        open_and_read_waves(file1, file2, &name_options).expect("Failed to open wave files");

    let mut output = Vec::new();
    let options = DiffOptions {
        start,
        end,
        real_epsilon: None,
    };
    let has_differences = diff_waves(&mut output, reader1, hier1, reader2, hier2, &options)
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
    assert!(
        !has_diff,
        "Starting at time 20 should skip the time-10 difference. Output:\n{}",
        output
    );
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
    assert!(
        !has_diff,
        "Ending at time 40 should skip the time-50 difference. Output:\n{}",
        output
    );
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
    assert!(
        !has_diff,
        "Range 30-50 should skip differences at times 20-21. Output:\n{}",
        output
    );
}

#[test]
fn test_diff_start_beyond_all_data() {
    let (has_diff, output) = run_wave_diff_test_with_range(
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
        1000,
        None,
    );
    assert!(
        !has_diff,
        "Starting beyond all data should show no differences. Output:\n{}",
        output
    );
}

#[test]
fn test_diff_vcd_start_skips_early_difference() {
    let (has_diff, output) = run_wave_diff_test_with_range(
        "tests/data/counter.vcd",
        "tests/data/counter.value.diff.vcd",
        20,
        None,
    );
    assert!(
        !has_diff,
        "VCD: starting at time 20 should skip the time-10 difference. Output:\n{}",
        output
    );
}

#[test]
fn test_diff_vcd_end_skips_late_difference() {
    let (has_diff, output) = run_wave_diff_test_with_range(
        "tests/data/counter.vcd",
        "tests/data/counter.end_time.diff.vcd",
        0,
        Some(40),
    );
    assert!(
        !has_diff,
        "VCD: ending at time 40 should skip the time-50 difference. Output:\n{}",
        output
    );
}

// -- Additional epsilon edge cases --------------------------------------------

#[test]
fn test_diff_zero_epsilon() {
    // Zero epsilon should require exact match, same as no epsilon
    let (has_diff, output) = run_wave_diff_test_with_epsilon(
        "tests/data/real_base.vcd",
        "tests/data/real_close.vcd",
        Some(0.0),
    );
    assert!(
        has_diff,
        "Zero epsilon should require exact match. Output:\n{}",
        output
    );
}

#[test]
fn test_diff_epsilon_wide_bitvector_no_false_positive() {
    // A 512-bit all-1s value parses as a float, but identical bit-vectors
    // must not be reported as different just because --epsilon is set.
    let (has_diff, output) = run_wave_diff_test_with_epsilon(
        "tests/data/wide_bits.vcd",
        "tests/data/wide_bits.vcd",
        Some(0.0000001),
    );
    assert!(
        !has_diff,
        "Identical wide bit-vectors should not differ with epsilon. Output:\n{}",
        output
    );
}

#[test]
fn test_diff_epsilon_does_not_mask_bitvector_difference() {
    let (has_diff, output) = run_wave_diff_test_with_epsilon(
        "tests/data/counter.vcd",
        "tests/data/counter.value.diff.vcd",
        Some(1000.0),
    );
    assert!(
        has_diff,
        "Bit-vector differences should stay exact with epsilon. Output:\n{}",
        output
    );

    let expected = "\
10 t.the_sub.cyc_plus_one 00000000000000000000000000000010 != 00000000000000000000000000000100
";
    assert_eq!(
        output, expected,
        "Expected exact diff output for VCD value diff"
    );
}

// -- Metadata comparison tests ------------------------------------------------

#[test]
fn test_diff_type_mismatch() {
    let name_options = NameOptions::default();
    let (_r1, hier1, _r2, hier2) = open_and_read_waves(
        "tests/data/type_mismatch.a.vcd",
        "tests/data/type_mismatch.b.vcd",
        &name_options,
    )
    .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&hier1, &hier2);
    assert!(!diffs.is_empty(), "Should detect type mismatches");

    // clk: wire vs reg
    assert!(
        diffs
            .iter()
            .any(|d| d.contains("top.clk") && d.contains("wire") && d.contains("reg")),
        "Should detect clk type mismatch: {:?}",
        diffs
    );
    // state: wire vs reg
    assert!(
        diffs
            .iter()
            .any(|d| d.contains("top.state") && d.contains("wire") && d.contains("reg")),
        "Should detect state type mismatch: {:?}",
        diffs
    );
}

#[test]
fn test_diff_size_mismatch() {
    let name_options = NameOptions::default();
    let (_r1, hier1, _r2, hier2) = open_and_read_waves(
        "tests/data/type_mismatch.a.vcd",
        "tests/data/type_mismatch.b.vcd",
        &name_options,
    )
    .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&hier1, &hier2);

    // data: size 8 vs 16
    assert!(
        diffs
            .iter()
            .any(|d| d.contains("top.data") && d.contains("8") && d.contains("16")),
        "Should detect data size mismatch: {:?}",
        diffs
    );
}

#[test]
fn test_diff_identical_metadata() {
    let name_options = NameOptions::default();
    let (_r1, hier1, _r2, hier2) = open_and_read_waves(
        "tests/data/type_mismatch.a.vcd",
        "tests/data/type_mismatch.a.vcd",
        &name_options,
    )
    .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&hier1, &hier2);
    assert!(
        diffs.is_empty(),
        "Same file should have no metadata diffs: {:?}",
        diffs
    );
}

#[test]
fn test_diff_cross_format_metadata() {
    // FST and VCD of the same design may have different var types (FST preserves
    // original types like "reg"/"integer" while VCD might use "wire"). Direction
    // comparison should be skipped since VCD has no direction info ("implicit").
    let name_options = NameOptions::default();
    let (_r1, hier1, _r2, hier2) = open_and_read_waves(
        "tests/data/counter.fst",
        "tests/data/counter.vcd",
        &name_options,
    )
    .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&hier1, &hier2);
    // Direction diffs should NOT appear since VCD direction is "implicit"
    assert!(
        !diffs.iter().any(|d| d.contains("direction")),
        "Should not report direction diffs when VCD side is implicit: {:?}",
        diffs
    );
}

// -- Attribute comparison tests -----------------------------------------------

#[test]
fn test_diff_enum_attr_difference() {
    // Same signal names/types/values, but different enum table attributes:
    //   a: state has enum state_t (IDLE/ACTIVE/DONE)
    //   b: state has enum alt_state_t (OFF/ON/ERR)
    let name_options = NameOptions::default();
    let (_r1, hier1, _r2, hier2) = open_and_read_waves(
        "tests/data/enum_attrs.a.vcd",
        "tests/data/enum_attrs.b.vcd",
        &name_options,
    )
    .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&hier1, &hier2);
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
    let (_r1, hier1, _r2, hier2) = open_and_read_waves(
        "tests/data/enum_attrs.a.vcd",
        "tests/data/enum_attrs.b.vcd",
        &name_options,
    )
    .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&hier1, &hier2);
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
    let (_r1, hier1, _r2, hier2) = open_and_read_waves(
        "tests/data/enum_attrs.a.vcd",
        "tests/data/enum_attrs.missing.vcd",
        &name_options,
    )
    .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&hier1, &hier2);
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
    // Same file compared to itself -- no attr differences
    let name_options = NameOptions::default();
    let (_r1, hier1, _r2, hier2) = open_and_read_waves(
        "tests/data/enum_attrs.a.vcd",
        "tests/data/enum_attrs.a.vcd",
        &name_options,
    )
    .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&hier1, &hier2);
    assert!(
        diffs.is_empty(),
        "Same file should have no diffs: {:?}",
        diffs
    );
}

#[test]
fn test_diff_real_size_normalized_across_formats() {
    // FST stores real signal sizes in bytes (8), VCD in bits (64).
    // After normalization both should report 64 -- no size mismatch.
    let name_options = NameOptions::default();
    let (_r1, hier1, _r2, hier2) = open_and_read_waves(
        "tests/data/real_base.fst",
        "tests/data/real_base.vcd",
        &name_options,
    )
    .expect("Failed to open wave files");

    let diffs = compare_signal_meta(&hier1, &hier2);
    assert!(
        diffs.is_empty(),
        "FST and VCD of the same design should have no metadata diffs: {:?}",
        diffs
    );
}

// -- --no-attrs CLI tests -----------------------------------------------------

mod common;
use common::{run_wavecat_cli, run_wavediff_cli};

#[test]
fn test_cli_attr_diff_nonzero_exit() {
    // Different attrs, same values -- should exit 1
    let output = run_wavediff_cli(&["tests/data/enum_attrs.a.vcd", "tests/data/enum_attrs.b.vcd"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "Attr diffs should cause exit 1"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("top.state"),
        "stderr should mention top.state: {}",
        stderr
    );
    assert!(
        stderr.contains("top.data"),
        "stderr should mention top.data: {}",
        stderr
    );
}

#[test]
fn test_cli_no_attrs_ignores_attr_diff() {
    // Different attrs, same values -- --no-attrs should make it exit 0
    let output = run_wavediff_cli(&[
        "--no-attrs",
        "tests/data/enum_attrs.a.vcd",
        "tests/data/enum_attrs.b.vcd",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "--no-attrs should ignore attr differences. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_cli_no_attrs_ignores_missing_attrs() {
    // Attrs in one file, none in the other -- --no-attrs should exit 0
    let output = run_wavediff_cli(&[
        "--no-attrs",
        "tests/data/enum_attrs.a.vcd",
        "tests/data/enum_attrs.missing.vcd",
    ]);
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
    let output = run_wavediff_cli(&[
        "--no-attrs",
        "tests/data/counter.vcd",
        "tests/data/counter.value.diff.vcd",
    ]);
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

#[test]
fn test_cli_wavecat_malformed_vcd_data_exits_with_error() {
    let output = run_wavecat_cli(&["tests/data/error/malformed_data.vcd"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "wavecat should exit 1 on malformed VCD data"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected character"),
        "wavecat stderr should report the parser error: {}",
        stderr
    );
}

#[test]
fn test_cli_wavediff_malformed_vcd_data_exits_with_error() {
    let output = run_wavediff_cli(&[
        "tests/data/error/malformed_data.vcd",
        "tests/data/error/malformed_data.vcd",
    ]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "wavediff should exit 2 on malformed VCD data"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected character"),
        "wavediff stderr should report the parser error: {}",
        stderr
    );
}

// -- Enum conflict detection tests --------------------------------------------

#[test]
fn test_enum_conflict_errors_on_open() {
    let name_options = NameOptions::default();
    let result = wavetools::open_wave_file(
        std::path::Path::new("tests/data/error/enum_conflict.vcd"),
        &name_options,
    );
    assert!(result.is_err(), "Conflicting enum definitions should error");
    let err = result.err().unwrap();
    assert!(
        err.contains("conflicting enum definitions") && err.contains("$unit::state_t"),
        "Error should mention the conflicting enum name: {}",
        err
    );
}

#[test]
fn test_enum_no_conflict_succeeds() {
    let name_options = NameOptions::default();
    let result = wavetools::open_wave_file(
        std::path::Path::new("tests/data/enum_no_conflict.vcd"),
        &name_options,
    );
    assert!(
        result.is_ok(),
        "Non-conflicting duplicate enum definitions should succeed: {:?}",
        result.err()
    );
}

#[test]
fn test_cli_wavecat_enum_conflict_exits_with_error() {
    let output = run_wavecat_cli(&["--names", "tests/data/error/enum_conflict.vcd"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "wavecat should exit 1 on enum conflict"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("conflicting enum definitions"),
        "wavecat stderr should report conflict: {}",
        stderr
    );
}

#[test]
fn test_cli_wavediff_enum_conflict_exits_with_error() {
    let output = run_wavediff_cli(&[
        "tests/data/error/enum_conflict.vcd",
        "tests/data/enum_no_conflict.vcd",
    ]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "wavediff should exit 2 on enum conflict"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("conflicting enum definitions"),
        "wavediff stderr should report conflict: {}",
        stderr
    );
}

#[test]
fn test_non_qualified_duplicate_enums_no_conflict() {
    // Enum names without "::" are not checked for conflicts -- they are local
    // definitions that can legitimately differ across scopes.
    let name_options = NameOptions::default();
    let result = wavetools::open_wave_file(
        std::path::Path::new("tests/data/enum_attrs.a.vcd"),
        &name_options,
    );
    assert!(
        result.is_ok(),
        "Non-qualified enum names should never trigger conflict detection: {:?}",
        result.err()
    );
}

// -- --filter tests -----------------------------------------------------------

fn run_wave_diff_test_with_filter(file1: &str, file2: &str, filter: &[&str]) -> (bool, String) {
    let name_options = NameOptions::default();
    let (reader1, mut hier1, reader2, mut hier2) =
        wavetools::open_and_read_waves(file1, file2, &name_options)
            .expect("Failed to open wave files");

    let filter_strings: Vec<String> = filter.iter().map(|&s| s.to_string()).collect();
    let patterns =
        wavetools::parse_filter_patterns(&filter_strings).expect("Failed to parse filter patterns");
    wavetools::apply_filter(&mut hier1, &patterns);
    wavetools::apply_filter(&mut hier2, &patterns);

    let mut output = Vec::new();
    let options = DiffOptions {
        start: 0,
        end: None,
        real_epsilon: None,
    };
    let has_differences =
        wavetools::diff_waves(&mut output, reader1, hier1, reader2, hier2, &options)
            .expect("Failed to diff files");

    let output_str = String::from_utf8(output).expect("Invalid UTF-8");
    (has_differences, output_str)
}

#[test]
fn test_filter_excludes_differing_signal() {
    // counter.value.diff.fst differs only at t.the_sub.cyc_plus_one.
    // Filtering it out leaves only matching signals -- no diff.
    let (has_diff, output) = run_wave_diff_test_with_filter(
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
        &["*.clk"],
    );
    assert!(
        !has_diff,
        "Filter excluding the differing signal should report no diff. Output:\n{}",
        output
    );
    assert_eq!(output, "", "No diff output expected");
}

#[test]
fn test_filter_includes_differing_signal() {
    // Same case but the filter targets the differing signal.
    let (has_diff, output) = run_wave_diff_test_with_filter(
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
        &["*.cyc_plus_one"],
    );
    assert!(
        has_diff,
        "Filter including the differing signal should report a diff"
    );
    let expected = "\
10 t.the_sub.cyc_plus_one 00000000000000000000000000000010 != 00000000000000000000000000000100
";
    assert_eq!(
        output, expected,
        "Expected diff output for the targeted signal"
    );
}

#[test]
fn test_filter_space_separated_patterns() {
    // A single --filter value with multiple whitespace-separated globs should
    // be split into individual patterns.
    let (has_diff, output) = run_wave_diff_test_with_filter(
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
        &["*.clk *.cyc"],
    );
    assert!(
        !has_diff,
        "Two safe-signal globs should still report no diff. Output:\n{}",
        output
    );
}

#[test]
fn test_filter_multiple_args_are_unioned() {
    // Several --filter values should union: signals matching any pattern are kept.
    let (has_diff, output) = run_wave_diff_test_with_filter(
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
        &["*.clk", "*.cyc"],
    );
    assert!(
        !has_diff,
        "Unioned safe-signal filters should report no diff. Output:\n{}",
        output
    );
}

#[test]
fn test_filter_matches_nothing_yields_no_diff() {
    // A filter that matches no signals in either file leaves both hierarchies
    // empty -- no name diffs, no value diffs, nothing to report.
    let (has_diff, output) = run_wave_diff_test_with_filter(
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
        &["*.nonexistent"],
    );
    assert!(
        !has_diff,
        "Filter matching nothing should not synthesize a diff. Output:\n{}",
        output
    );
    assert_eq!(output, "", "Empty filtered set should produce no output");
}

#[test]
fn test_filter_preserves_unrelated_diff() {
    // Filter includes the differing signal AND others. Diff should still fire,
    // and only the differing signal's line should appear.
    let (has_diff, output) = run_wave_diff_test_with_filter(
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
        &["*.clk", "*.cyc_plus_one"],
    );
    assert!(
        has_diff,
        "Filter including the differing signal should still diff"
    );
    let expected = "\
10 t.the_sub.cyc_plus_one 00000000000000000000000000000010 != 00000000000000000000000000000100
";
    assert_eq!(
        output, expected,
        "Only the targeted differing signal should be reported"
    );
}

// -- --filter CLI tests -------------------------------------------------------

#[test]
fn test_cli_filter_excludes_diff_exits_zero() {
    let output = run_wavediff_cli(&[
        "--filter",
        "*.clk",
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "Filtering out the diff should exit 0. stderr: {} stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
}

#[test]
fn test_cli_filter_short_flag_includes_diff_exits_one() {
    let output = run_wavediff_cli(&[
        "-f",
        "*.cyc_plus_one",
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "Targeting the differing signal should exit 1"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cyc_plus_one"),
        "stdout should contain the diff: {}",
        stdout
    );
}

#[test]
fn test_cli_filter_repeated_flag() {
    // Repeated --filter flags should union, leaving only safe signals.
    let output = run_wavediff_cli(&[
        "--filter",
        "*.clk",
        "--filter",
        "*.cyc",
        "tests/data/counter.fst",
        "tests/data/counter.value.diff.fst",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "Repeated filters covering only safe signals should exit 0. stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

fn filter_name_mismatch(
    file1: &str,
    file2: &str,
    filter: &[&str],
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    let name_options = NameOptions::default();
    let (_r1, mut hier1, _r2, mut hier2) =
        wavetools::open_and_read_waves(file1, file2, &name_options)
            .expect("Failed to open wave files");
    let filter_strings: Vec<String> = filter.iter().map(|&s| s.to_string()).collect();
    let patterns =
        wavetools::parse_filter_patterns(&filter_strings).expect("Failed to parse filter patterns");
    wavetools::apply_filter(&mut hier1, &patterns);
    wavetools::apply_filter(&mut hier2, &patterns);
    compare_signal_names(&hier1, &hier2)
}

#[test]
fn test_filter_matches_only_file1_reports_name_mismatch() {
    // counter.sig_name.diff.fst renames cyc_plus_one -> blargh, so the filter
    // hits a signal in file1 only. This asymmetry must surface as a diff so
    // the user knows their filter scope didn't line up across the two files.
    let (only_in_1, only_in_2) = filter_name_mismatch(
        "tests/data/counter.fst",
        "tests/data/counter.sig_name.diff.fst",
        &["*.cyc_plus_one"],
    );
    assert!(
        only_in_1.contains("t.the_sub.cyc_plus_one"),
        "Should report cyc_plus_one only in file1, got: {:?}",
        only_in_1
    );
    assert!(
        only_in_2.is_empty(),
        "Nothing should be only in file2, got: {:?}",
        only_in_2
    );
}

#[test]
fn test_filter_matches_only_file2_reports_name_mismatch() {
    // Symmetric case: *.blargh hits the renamed signal in file2 but nothing
    // in file1.
    let (only_in_1, only_in_2) = filter_name_mismatch(
        "tests/data/counter.fst",
        "tests/data/counter.sig_name.diff.fst",
        &["*.blargh"],
    );
    assert!(
        only_in_2.contains("t.the_sub.blargh"),
        "Should report blargh only in file2, got: {:?}",
        only_in_2
    );
    assert!(
        only_in_1.is_empty(),
        "Nothing should be only in file1, got: {:?}",
        only_in_1
    );
}

#[test]
fn test_cli_filter_matches_only_file1_exits_one() {
    let output = run_wavediff_cli(&[
        "--filter",
        "*.cyc_plus_one",
        "tests/data/counter.fst",
        "tests/data/counter.sig_name.diff.fst",
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "Filter matching only file1 should exit 1. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("t.the_sub.cyc_plus_one"),
        "stderr should name the asymmetric signal: {}",
        stderr
    );
}

#[test]
fn test_cli_filter_matches_only_file2_exits_one() {
    let output = run_wavediff_cli(&[
        "--filter",
        "*.blargh",
        "tests/data/counter.fst",
        "tests/data/counter.sig_name.diff.fst",
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "Filter matching only file2 should exit 1. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("t.the_sub.blargh"),
        "stderr should name the asymmetric signal: {}",
        stderr
    );
}

#[test]
fn test_cli_filter_invalid_glob_exits_with_error() {
    let output = run_wavediff_cli(&[
        "--filter",
        "[",
        "tests/data/counter.fst",
        "tests/data/counter.fst",
    ]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "Invalid glob pattern should exit 2 with error"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid glob pattern"),
        "stderr should mention the invalid pattern: {}",
        stderr
    );
}

#[test]
fn test_cli_ignore_xz_masks_x_differences() {
    let output = run_wavediff_cli(&[
        "--ignore-xz",
        "tests/data/diff/x_base.vcd",
        "tests/data/diff/x_diff.vcd",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "--ignore-xz should ignore differences involving x. stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn test_cli_ignore_xz_masks_z_differences() {
    let pid = std::process::id();
    let file1 = std::env::temp_dir().join(format!("wavetools-ignore-xz-z-{}-1.vcd", pid));
    let file2 = std::env::temp_dir().join(format!("wavetools-ignore-xz-z-{}-2.vcd", pid));
    std::fs::write(
        &file1,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 4 ! data $end
$upscope $end
$enddefinitions $end
#0
b0000 !
#10
bzzzz !
",
    )
    .expect("write file1");
    std::fs::write(
        &file2,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 4 ! data $end
$upscope $end
$enddefinitions $end
#0
b0000 !
#10
b1010 !
",
    )
    .expect("write file2");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wavediff"))
        .args([
            "--ignore-xz",
            file1.to_str().expect("temp path should be UTF-8"),
            file2.to_str().expect("temp path should be UTF-8"),
        ])
        .output()
        .expect("Failed to run wavediff");
    assert_eq!(
        output.status.code(),
        Some(0),
        "--ignore-xz should ignore differences involving z. stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let _ = std::fs::remove_file(file1);
    let _ = std::fs::remove_file(file2);
}

#[test]
fn test_cli_ignore_xz_still_reports_known_bit_differences() {
    let pid = std::process::id();
    let file1 = std::env::temp_dir().join(format!("wavetools-ignore-xz-{}-1.vcd", pid));
    let file2 = std::env::temp_dir().join(format!("wavetools-ignore-xz-{}-2.vcd", pid));
    std::fs::write(
        &file1,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 2 ! data $end
$upscope $end
$enddefinitions $end
#0
b00 !
#10
b0x !
",
    )
    .expect("write file1");
    std::fs::write(
        &file2,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 2 ! data $end
$upscope $end
$enddefinitions $end
#0
b00 !
#10
b11 !
",
    )
    .expect("write file2");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wavediff"))
        .args([
            "--ignore-xz",
            file1.to_str().expect("temp path should be UTF-8"),
            file2.to_str().expect("temp path should be UTF-8"),
        ])
        .output()
        .expect("Failed to run wavediff");
    assert_eq!(
        output.status.code(),
        Some(1),
        "--ignore-xz should still report known-bit differences. stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("10 t.data 0x != 11"),
        "stdout should contain the known-bit diff: {}",
        stdout
    );

    let _ = std::fs::remove_file(file1);
    let _ = std::fs::remove_file(file2);
}

#[test]
fn test_cli_without_ignore_xz_reports_x_differences() {
    let output = run_wavediff_cli(&["tests/data/diff/x_base.vcd", "tests/data/diff/x_diff.vcd"]);
    assert_eq!(output.status.code(), Some(1), "Expected exit 1");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("t.data"),
        "stdout should contain the x diff: {}",
        stdout
    );
}

// Regression: --ignore-xz must mask a difference even when only ONE side
// changes at the differing time and the other is merely HOLDING an x from an
// earlier time. The change-driven comparison could not see the held x; the
// state-tracking comparison can.
#[test]
fn test_cli_ignore_xz_masks_held_x_on_unaligned_change() {
    let pid = std::process::id();
    let file1 = std::env::temp_dir().join(format!("wavetools-held-x-{}-1.vcd", pid));
    let file2 = std::env::temp_dir().join(format!("wavetools-held-x-{}-2.vcd", pid));
    // file1 settles to a known value at #20 (no change at #20 on file2).
    std::fs::write(
        &file1,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 2 ! data $end
$upscope $end
$enddefinitions $end
#0
b00 !
#20
b11 !
#30
b00 !
",
    )
    .expect("write file1");
    // file2 goes to xx at #10 and holds it across #20 and #30.
    std::fs::write(
        &file2,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 2 ! data $end
$upscope $end
$enddefinitions $end
#0
b00 !
#10
bxx !
#30
b00 !
",
    )
    .expect("write file2");

    let with_ignore = std::process::Command::new(env!("CARGO_BIN_EXE_wavediff"))
        .args([
            "--ignore-xz",
            file1.to_str().expect("temp path should be UTF-8"),
            file2.to_str().expect("temp path should be UTF-8"),
        ])
        .output()
        .expect("Failed to run wavediff");
    assert_eq!(
        with_ignore.status.code(),
        Some(0),
        "--ignore-xz should mask the held-x difference. stdout: {} stderr: {}",
        String::from_utf8_lossy(&with_ignore.stdout),
        String::from_utf8_lossy(&with_ignore.stderr),
    );

    // Without --ignore-xz the same inputs differ (file1 b11 vs file2 held xx).
    let without_ignore = std::process::Command::new(env!("CARGO_BIN_EXE_wavediff"))
        .args([
            file1.to_str().expect("temp path should be UTF-8"),
            file2.to_str().expect("temp path should be UTF-8"),
        ])
        .output()
        .expect("Failed to run wavediff");
    assert_eq!(
        without_ignore.status.code(),
        Some(1),
        "without --ignore-xz the held-x case should still differ. stdout: {} stderr: {}",
        String::from_utf8_lossy(&without_ignore.stdout),
        String::from_utf8_lossy(&without_ignore.stderr),
    );

    let _ = std::fs::remove_file(file1);
    let _ = std::fs::remove_file(file2);
}

#[test]
fn test_cli_ignore_xz_masks_held_z_on_unaligned_change() {
    let pid = std::process::id();
    let file1 = std::env::temp_dir().join(format!("wavetools-held-z-{}-1.vcd", pid));
    let file2 = std::env::temp_dir().join(format!("wavetools-held-z-{}-2.vcd", pid));
    std::fs::write(
        &file1,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 2 ! data $end
$upscope $end
$enddefinitions $end
#0
b00 !
#20
b11 !
#30
b00 !
",
    )
    .expect("write file1");
    std::fs::write(
        &file2,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 2 ! data $end
$upscope $end
$enddefinitions $end
#0
b00 !
#10
bzz !
#30
b00 !
",
    )
    .expect("write file2");

    let with_ignore = std::process::Command::new(env!("CARGO_BIN_EXE_wavediff"))
        .args([
            "--ignore-xz",
            file1.to_str().expect("temp path should be UTF-8"),
            file2.to_str().expect("temp path should be UTF-8"),
        ])
        .output()
        .expect("Failed to run wavediff");
    assert_eq!(
        with_ignore.status.code(),
        Some(0),
        "--ignore-xz should mask the held-z difference. stdout: {} stderr: {}",
        String::from_utf8_lossy(&with_ignore.stdout),
        String::from_utf8_lossy(&with_ignore.stderr),
    );

    let without_ignore = std::process::Command::new(env!("CARGO_BIN_EXE_wavediff"))
        .args([
            file1.to_str().expect("temp path should be UTF-8"),
            file2.to_str().expect("temp path should be UTF-8"),
        ])
        .output()
        .expect("Failed to run wavediff");
    assert_eq!(
        without_ignore.status.code(),
        Some(1),
        "without --ignore-xz the held-z case should still differ. stdout: {} stderr: {}",
        String::from_utf8_lossy(&without_ignore.stdout),
        String::from_utf8_lossy(&without_ignore.stderr),
    );

    let _ = std::fs::remove_file(file1);
    let _ = std::fs::remove_file(file2);
}

// The state-tracking path must fan a held value out to every name an aliased
// id resolves to, reporting each known-bit difference once.
#[test]
fn test_cli_ignore_xz_aliased_idcodes_report_each_name_once() {
    let output = run_wavediff_cli(&[
        "--ignore-xz",
        "tests/data/idcode_a.vcd",
        "tests/data/error/idcode_a_value_diff.vcd",
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "known-bit diffs on aliased ids should report under --ignore-xz. stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout, "0 m.s0.a 0 != 1\n0 m.s1.c 0 != 1\n",
        "each aliased name should be reported exactly once"
    );
}

// Trailing samples beyond the shorter input are trimmed (not diffed) on the
// state-tracking path too.
#[test]
fn test_cli_ignore_xz_trims_trailing_samples() {
    let pid = std::process::id();
    let file1 = std::env::temp_dir().join(format!("wavetools-ixz-trim-{}-1.vcd", pid));
    let file2 = std::env::temp_dir().join(format!("wavetools-ixz-trim-{}-2.vcd", pid));
    // file1 runs longer than file2; the extra #30 sample should be trimmed.
    std::fs::write(
        &file1,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 1 ! data $end
$upscope $end
$enddefinitions $end
#0
b0 !
#10
b1 !
#30
b0 !
",
    )
    .expect("write file1");
    std::fs::write(
        &file2,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 1 ! data $end
$upscope $end
$enddefinitions $end
#0
b0 !
#10
b1 !
",
    )
    .expect("write file2");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wavediff"))
        .args([
            "--ignore-xz",
            file1.to_str().expect("temp path should be UTF-8"),
            file2.to_str().expect("temp path should be UTF-8"),
        ])
        .output()
        .expect("Failed to run wavediff");
    assert_eq!(
        output.status.code(),
        Some(0),
        "trailing file1 samples should be trimmed, not diffed. stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Ignored trailing samples"),
        "should report the trim on stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let _ = std::fs::remove_file(file1);
    let _ = std::fs::remove_file(file2);
}

#[test]
fn test_cli_wavediff_reports_ignored_longer_input() {
    let output = run_wavediff_cli(&[
        "tests/data/counter.fst",
        "tests/data/counter.end_time.diff.fst",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "Trailing-only comparison should exit 0. stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Ignored trailing samples in tests/data/counter.fst after time 40"),
        "stderr should report ignored trailing samples: {}",
        stderr
    );
}

#[test]
fn test_cli_fst_diff_writes_side_by_side_signals() {
    let out_path =
        std::env::temp_dir().join(format!("wavetools-fst-diff-{}.fst", std::process::id()));
    let _ = std::fs::remove_file(&out_path);
    let out_str = out_path.to_str().expect("temp path should be UTF-8");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wavediff"))
        .args([
            "--fst-diff",
            out_str,
            "tests/data/counter.fst",
            "tests/data/counter.value.diff.fst",
        ])
        .output()
        .expect("Failed to run wavediff");
    assert_eq!(
        output.status.code(),
        Some(1),
        "Expected value diff. stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        out_path.exists(),
        "Expected fst diff output at {}",
        out_path.display()
    );

    let names = std::process::Command::new(env!("CARGO_BIN_EXE_wavecat"))
        .args(["--names", "--sort", out_str])
        .output()
        .expect("Failed to run wavecat");
    assert!(
        names.status.success(),
        "wavecat should read fst diff. stderr: {}",
        String::from_utf8_lossy(&names.stderr)
    );
    let stdout = String::from_utf8_lossy(&names.stdout);
    let name_lines: Vec<&str> = stdout.lines().collect();
    assert!(
        name_lines.contains(&"t.clk"),
        "missing unsuffixed matching clock signal: {}",
        stdout
    );
    assert!(
        name_lines.contains(&"t.cyc"),
        "missing unsuffixed matching signal: {}",
        stdout
    );
    assert!(
        !name_lines.contains(&"t.the_sub.cyc_plus_one"),
        "differing signal should not also get an unsuffixed trace: {}",
        stdout
    );
    assert!(
        stdout.contains("t.the_sub.cyc_plus_one__counter"),
        "missing file1 side signal: {}",
        stdout
    );
    assert!(
        stdout.contains("t.the_sub.cyc_plus_one__counter_value_diff"),
        "missing file2 side signal: {}",
        stdout
    );

    let attrs = std::process::Command::new(env!("CARGO_BIN_EXE_wavecat"))
        .args(["--attrs", out_str])
        .output()
        .expect("Failed to run wavecat --attrs");
    assert!(
        attrs.status.success(),
        "wavecat should read fst diff attrs. stderr: {}",
        String::from_utf8_lossy(&attrs.stderr)
    );
    let attrs_stdout = String::from_utf8_lossy(&attrs.stdout);
    assert!(
        attrs_stdout
            .lines()
            .any(|line| line == "t.cyc  integer  32"),
        "matching signal should keep raw integer metadata: {}",
        attrs_stdout
    );
    assert!(
        !attrs_stdout
            .lines()
            .any(|line| line == "t.the_sub.cyc_plus_one  integer  32"),
        "differing signal should not have unsuffixed metadata: {}",
        attrs_stdout
    );
    assert!(
        attrs_stdout.contains("t.the_sub.cyc_plus_one__counter  integer  32"),
        "file1 side signal should keep raw integer metadata: {}",
        attrs_stdout
    );
    assert!(
        attrs_stdout.contains("t.the_sub.cyc_plus_one__counter_value_diff  integer  32"),
        "file2 side signal should keep raw integer metadata: {}",
        attrs_stdout
    );

    let values = std::process::Command::new(env!("CARGO_BIN_EXE_wavecat"))
        .arg(out_str)
        .output()
        .expect("Failed to run wavecat values");
    assert!(
        values.status.success(),
        "wavecat should read fst diff values. stderr: {}",
        String::from_utf8_lossy(&values.stderr)
    );
    let values_stdout = String::from_utf8_lossy(&values.stdout);
    assert!(
        values_stdout.contains("0 t.cyc 00000000000000000000000000000000"),
        "matching signal should include raw unsuffixed values: {}",
        values_stdout
    );
    assert!(
        !values_stdout
            .lines()
            .any(|line| line.starts_with("0 t.the_sub.cyc_plus_one ")),
        "differing signal should not include unsuffixed values: {}",
        values_stdout
    );
    assert!(
        values_stdout
            .contains("0 t.the_sub.cyc_plus_one__counter 00000000000000000000000000000001"),
        "file1 side signal should include initial raw values: {}",
        values_stdout
    );
    assert!(
        values_stdout.contains(
            "0 t.the_sub.cyc_plus_one__counter_value_diff 00000000000000000000000000000001"
        ),
        "file2 side signal should include initial raw values: {}",
        values_stdout
    );
    assert!(
        values_stdout
            .contains("10 t.the_sub.cyc_plus_one__counter 00000000000000000000000000000010"),
        "file1 side signal should include divergent raw values: {}",
        values_stdout
    );
    assert!(
        values_stdout.contains(
            "10 t.the_sub.cyc_plus_one__counter_value_diff 00000000000000000000000000000100"
        ),
        "file2 side signal should include divergent raw values: {}",
        values_stdout
    );

    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn test_cli_fst_diff_does_not_bake_ranges_into_signal_names() {
    let pid = std::process::id();
    let file1 = std::env::temp_dir().join(format!("wavetools-fst-range-base-{}.vcd", pid));
    let file2 = std::env::temp_dir().join(format!("wavetools-fst-range-diff-{}.vcd", pid));
    let out_path = std::env::temp_dir().join(format!("wavetools-fst-range-{}.fst", pid));
    let _ = std::fs::remove_file(&out_path);

    std::fs::write(
        &file1,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 2 ! data [1:0] $end
$upscope $end
$enddefinitions $end
#0
b00 !
#10
b01 !
",
    )
    .expect("write file1");
    std::fs::write(
        &file2,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 2 ! data [1:0] $end
$upscope $end
$enddefinitions $end
#0
b00 !
#10
b10 !
",
    )
    .expect("write file2");

    let out_str = out_path.to_str().expect("temp path should be UTF-8");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wavediff"))
        .args([
            "--fst-diff",
            out_str,
            file1.to_str().expect("temp path should be UTF-8"),
            file2.to_str().expect("temp path should be UTF-8"),
        ])
        .output()
        .expect("Failed to run wavediff");
    assert_eq!(
        output.status.code(),
        Some(1),
        "Expected value diff. stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let names = std::process::Command::new(env!("CARGO_BIN_EXE_wavecat"))
        .args(["--names", "--sort", out_str])
        .output()
        .expect("Failed to run wavecat");
    assert!(
        names.status.success(),
        "wavecat should read fst diff. stderr: {}",
        String::from_utf8_lossy(&names.stderr)
    );
    let stdout = String::from_utf8_lossy(&names.stdout);
    assert!(
        stdout.contains("t.data__"),
        "range signal should keep its base name: {}",
        stdout
    );
    assert!(
        !stdout.contains("_1_0"),
        "range suffix should not be baked into generated FST identifiers: {}",
        stdout
    );

    let _ = std::fs::remove_file(file1);
    let _ = std::fs::remove_file(file2);
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn test_cli_fst_diff_includes_side_only_signals() {
    let pid = std::process::id();
    let file1 = std::env::temp_dir().join(format!("wavetools-fst-side-only-a-{}.vcd", pid));
    let file2 = std::env::temp_dir().join(format!("wavetools-fst-side-only-b-{}.vcd", pid));
    let out_path = std::env::temp_dir().join(format!("wavetools-fst-side-only-{}.fst", pid));
    let _ = std::fs::remove_file(&out_path);

    std::fs::write(
        &file1,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 1 ! clk $end
$var wire 2 \" only_a $end
$upscope $end
$enddefinitions $end
#0
0!
b01 \"
#10
1!
b10 \"
",
    )
    .expect("write file1");
    std::fs::write(
        &file2,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 1 ! clk $end
$var wire 2 \" only_b $end
$upscope $end
$enddefinitions $end
#0
0!
b11 \"
#10
1!
b00 \"
",
    )
    .expect("write file2");

    let out_str = out_path.to_str().expect("temp path should be UTF-8");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wavediff"))
        .args([
            "--fst-diff",
            out_str,
            file1.to_str().expect("temp path should be UTF-8"),
            file2.to_str().expect("temp path should be UTF-8"),
        ])
        .output()
        .expect("Failed to run wavediff");
    assert_eq!(
        output.status.code(),
        Some(1),
        "Expected name diff. stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let names = std::process::Command::new(env!("CARGO_BIN_EXE_wavecat"))
        .args(["--names", "--sort", out_str])
        .output()
        .expect("Failed to run wavecat names");
    assert!(
        names.status.success(),
        "wavecat should read fst diff. stderr: {}",
        String::from_utf8_lossy(&names.stderr)
    );
    let names_stdout = String::from_utf8_lossy(&names.stdout);
    assert!(
        names_stdout
            .lines()
            .any(|line| line.starts_with("t.only_a__")),
        "missing file1-only suffixed trace: {}",
        names_stdout
    );
    assert!(
        names_stdout
            .lines()
            .any(|line| line.starts_with("t.only_b__")),
        "missing file2-only suffixed trace: {}",
        names_stdout
    );
    assert!(
        !names_stdout.lines().any(|line| line == "t.only_a"),
        "file1-only signal should not be unsuffixed: {}",
        names_stdout
    );
    assert!(
        !names_stdout.lines().any(|line| line == "t.only_b"),
        "file2-only signal should not be unsuffixed: {}",
        names_stdout
    );

    let values = std::process::Command::new(env!("CARGO_BIN_EXE_wavecat"))
        .arg(out_str)
        .output()
        .expect("Failed to run wavecat values");
    assert!(
        values.status.success(),
        "wavecat should read fst diff values. stderr: {}",
        String::from_utf8_lossy(&values.stderr)
    );
    let values_stdout = String::from_utf8_lossy(&values.stdout);
    assert!(
        values_stdout
            .lines()
            .any(|line| line.starts_with("0 t.only_a__") && line.ends_with("01")),
        "missing file1-only raw value: {}",
        values_stdout
    );
    assert!(
        values_stdout
            .lines()
            .any(|line| line.starts_with("0 t.only_b__") && line.ends_with("11")),
        "missing file2-only raw value: {}",
        values_stdout
    );

    let _ = std::fs::remove_file(file1);
    let _ = std::fs::remove_file(file2);
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn test_cli_fst_diff_omits_missing_side_changes() {
    let out_path = std::env::temp_dir().join(format!(
        "wavetools-fst-diff-edge-{}.fst",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&out_path);
    let out_str = out_path.to_str().expect("temp path should be UTF-8");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wavediff"))
        .args([
            "--fst-diff",
            out_str,
            "tests/data/counter.fst",
            "tests/data/counter.edge_time.diff.fst",
        ])
        .output()
        .expect("Failed to run wavediff");
    assert_eq!(
        output.status.code(),
        Some(1),
        "Expected edge-time diff. stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let values = std::process::Command::new(env!("CARGO_BIN_EXE_wavecat"))
        .arg(out_str)
        .output()
        .expect("Failed to run wavecat");
    assert!(
        values.status.success(),
        "wavecat should read fst diff. stderr: {}",
        String::from_utf8_lossy(&values.stderr)
    );
    let stdout = String::from_utf8_lossy(&values.stdout);
    assert!(
        !stdout.lines().any(|line| line.starts_with("0 t.clk ")),
        "differing clock should not be represented as an unsuffixed matching trace: {}",
        stdout
    );
    assert!(
        stdout.contains("20 t.clk__counter 0"),
        "missing file1 raw clock edge: {}",
        stdout
    );
    assert!(
        stdout.contains("21 t.clk__counter_edge_time_diff 0"),
        "missing file2 raw clock edge: {}",
        stdout
    );
    assert!(
        !stdout.contains("MISSING"),
        "fst diff should not emit synthetic MISSING values: {}",
        stdout
    );

    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn test_cli_fst_diff_truncates_to_shorter_input() {
    let pid = std::process::id();
    let file1 = std::env::temp_dir().join(format!("wavetools-fst-trim-{}-1.vcd", pid));
    let file2 = std::env::temp_dir().join(format!("wavetools-fst-trim-{}-2.vcd", pid));
    let out_path = std::env::temp_dir().join(format!("wavetools-fst-trim-{}.fst", pid));
    let _ = std::fs::remove_file(&out_path);

    std::fs::write(
        &file1,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 1 ! clk $end
$var wire 1 \" a $end
$upscope $end
$enddefinitions $end
#0
0!
0\"
#10
1!
1\"
#20
0!
0\"
#30
1!
1\"
#40
0!
0\"
",
    )
    .expect("write file1");
    std::fs::write(
        &file2,
        "\
$timescale 1ns $end
$scope module t $end
$var wire 1 ! clk $end
$var wire 1 \" a $end
$upscope $end
$enddefinitions $end
#0
0!
0\"
#10
1!
0\"
#20
0!
0\"
",
    )
    .expect("write file2");

    let out_str = out_path.to_str().expect("temp path should be UTF-8");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wavediff"))
        .args([
            "--fst-diff",
            out_str,
            file1.to_str().expect("temp path should be UTF-8"),
            file2.to_str().expect("temp path should be UTF-8"),
        ])
        .output()
        .expect("Failed to run wavediff");
    assert_eq!(
        output.status.code(),
        Some(1),
        "Expected value diff. stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let values = std::process::Command::new(env!("CARGO_BIN_EXE_wavecat"))
        .arg(out_str)
        .output()
        .expect("Failed to run wavecat");
    assert!(
        values.status.success(),
        "wavecat should read fst diff. stderr: {}",
        String::from_utf8_lossy(&values.stderr)
    );
    let stdout = String::from_utf8_lossy(&values.stdout);
    assert!(
        stdout.contains("20 t.clk 0"),
        "fst diff should include samples through the shorter input: {}",
        stdout
    );
    assert!(
        !stdout.contains("30 "),
        "fst diff should not include raw samples after the shorter input: {}",
        stdout
    );
    assert!(
        !stdout.contains("40 "),
        "fst diff should not include raw samples after the shorter input: {}",
        stdout
    );

    let _ = std::fs::remove_file(file1);
    let _ = std::fs::remove_file(file2);
    let _ = std::fs::remove_file(out_path);
}
