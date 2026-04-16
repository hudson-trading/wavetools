//------------------------------------------------------------------------------
// cat_option_tests.rs
// Tests for wavecat output options: time filtering, formatting, format forcing
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use wavetools::{
    names_only, open_wave_file, open_wave_file_with_format, write_names, write_signals_wave,
    NameOptions, SignalOutputOptions, WaveFormat,
};
use std::path::Path;

fn read_names_with_options(path: &str, name_options: &NameOptions) -> String {
    let (_, hier) =
        open_wave_file(Path::new(path), name_options).expect("Failed to open wave file");
    let handle_to_names = names_only(&hier.signal_map, &hier.names);

    let mut output = Vec::new();
    write_names(&mut output, &handle_to_names, true).expect("Failed to write names");
    String::from_utf8(output).expect("Invalid UTF-8")
}

fn read_signals_with_options(
    path: &str,
    start: u64,
    end: Option<u64>,
    output_options: &SignalOutputOptions,
) -> String {
    let name_options = NameOptions::default();
    let (mut reader, hier) =
        open_wave_file(Path::new(path), &name_options).expect("Failed to open wave file");
    let handle_to_names = names_only(&hier.signal_map, &hier.names);

    let mut output = Vec::new();
    write_signals_wave(
        &mut output,
        &mut reader,
        &handle_to_names,
        start,
        end,
        output_options,
    )
    .expect("Failed to write signals");
    String::from_utf8(output).expect("Invalid UTF-8")
}

// ── Time filtering tests ────────────────────────────────────────────────────

#[test]
fn test_signals_start_end_fst() {
    let options = SignalOutputOptions { sort: true, time_pound: false };
    let output = read_signals_with_options("tests/data/counter.fst", 10, Some(30), &options);
    let expected = "\
10 t.clk 1
10 t.cyc 00000000000000000000000000000001
10 t.the_sub.cyc 00000000000000000000000000000001
10 t.the_sub.cyc_plus_one 00000000000000000000000000000010
20 t.clk 0
30 t.clk 1
30 t.cyc 00000000000000000000000000000010
30 t.the_sub.cyc 00000000000000000000000000000010
30 t.the_sub.cyc_plus_one 00000000000000000000000000000011
";
    assert_eq!(output, expected);
}

#[test]
fn test_signals_start_end_vcd() {
    let options = SignalOutputOptions { sort: true, time_pound: false };
    let output = read_signals_with_options("tests/data/counter.vcd", 10, Some(30), &options);
    let expected = "\
10 t.clk 1
10 t.cyc 00000000000000000000000000000001
10 t.the_sub.cyc 00000000000000000000000000000001
10 t.the_sub.cyc_plus_one 00000000000000000000000000000010
20 t.clk 0
30 t.clk 1
30 t.cyc 00000000000000000000000000000010
30 t.the_sub.cyc 00000000000000000000000000000010
30 t.the_sub.cyc_plus_one 00000000000000000000000000000011
";
    assert_eq!(output, expected);
}

#[test]
fn test_signals_start_only() {
    let options = SignalOutputOptions { sort: true, time_pound: false };
    let output = read_signals_with_options("tests/data/counter.fst", 40, None, &options);
    let expected = "\
40 t.clk 0
50 t.clk 1
";
    assert_eq!(output, expected);
}

#[test]
fn test_signals_end_only() {
    let options = SignalOutputOptions { sort: true, time_pound: false };
    let output = read_signals_with_options("tests/data/counter.fst", 0, Some(10), &options);
    let expected = "\
0 t.clk 0
0 t.cyc 00000000000000000000000000000000
0 t.the_sub.cyc 00000000000000000000000000000000
0 t.the_sub.cyc_plus_one 00000000000000000000000000000001
10 t.clk 1
10 t.cyc 00000000000000000000000000000001
10 t.the_sub.cyc 00000000000000000000000000000001
10 t.the_sub.cyc_plus_one 00000000000000000000000000000010
";
    assert_eq!(output, expected);
}

#[test]
fn test_signals_start_beyond_data() {
    let options = SignalOutputOptions { sort: true, time_pound: false };
    let output = read_signals_with_options("tests/data/counter.fst", 1000, None, &options);
    assert!(output.is_empty(), "No output expected when start is beyond all data");
}

// ── Output option tests ─────────────────────────────────────────────────────

#[test]
fn test_signals_time_pound() {
    let options = SignalOutputOptions { sort: true, time_pound: true };
    let output = read_signals_with_options("tests/data/counter.fst", 0, Some(10), &options);
    let expected = "\
#0 t.clk 0
#0 t.cyc 00000000000000000000000000000000
#0 t.the_sub.cyc 00000000000000000000000000000000
#0 t.the_sub.cyc_plus_one 00000000000000000000000000000001
#10 t.clk 1
#10 t.cyc 00000000000000000000000000000001
#10 t.the_sub.cyc 00000000000000000000000000000001
#10 t.the_sub.cyc_plus_one 00000000000000000000000000000010
";
    assert_eq!(output, expected);
}

#[test]
fn test_signals_time_pound_vcd() {
    let options = SignalOutputOptions { sort: true, time_pound: true };
    let output = read_signals_with_options("tests/data/counter.vcd", 0, Some(10), &options);
    let expected = "\
#0 t.clk 0
#0 t.cyc 00000000000000000000000000000000
#0 t.the_sub.cyc 00000000000000000000000000000000
#0 t.the_sub.cyc_plus_one 00000000000000000000000000000001
#10 t.clk 1
#10 t.cyc 00000000000000000000000000000001
#10 t.the_sub.cyc 00000000000000000000000000000001
#10 t.the_sub.cyc_plus_one 00000000000000000000000000000010
";
    assert_eq!(output, expected);
}

// ── no_range_space tests ─────────────────────────────────────────────────────

#[test]
fn test_names_with_range_default() {
    let output = read_names_with_options("tests/data/range.vcd", &NameOptions::default());
    let expected = "\
t.clk
t.dat [3:0]
";
    assert_eq!(output, expected);
}

#[test]
fn test_names_with_range_no_space() {
    let options = NameOptions { no_range_space: true };
    let output = read_names_with_options("tests/data/range.vcd", &options);
    let expected = "\
t.clk
t.dat[3:0]
";
    assert_eq!(output, expected);
}

// ── Format forcing tests ─────────────────────────────────────────────────────

#[test]
fn test_format_forcing_fst() {
    let name_options = NameOptions::default();
    let result = open_wave_file_with_format(
        Path::new("tests/data/counter.fst"),
        &name_options,
        Some(WaveFormat::Fst),
    );
    assert!(result.is_ok(), "Should open FST file with forced FST format");
}

#[test]
fn test_format_forcing_vcd() {
    let name_options = NameOptions::default();
    let result = open_wave_file_with_format(
        Path::new("tests/data/counter.vcd"),
        &name_options,
        Some(WaveFormat::Vcd),
    );
    assert!(result.is_ok(), "Should open VCD file with forced VCD format");
}

#[test]
fn test_format_forcing_wrong_format() {
    let name_options = NameOptions::default();
    let result = open_wave_file_with_format(
        Path::new("tests/data/counter.fst"),
        &name_options,
        Some(WaveFormat::Vcd),
    );
    assert!(result.is_err(), "Should fail to open FST file as VCD");
}
