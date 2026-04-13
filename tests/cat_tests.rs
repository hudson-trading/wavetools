//------------------------------------------------------------------------------
// cat_tests.rs
// Data-driven tests comparing wavecat output against expected golden files
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use std::path::Path;

use wavetools::{open_wave_file, write_names, write_signals_wave, NameOptions, SignalOutputOptions};

/// Strip the .fst or .vcd extension to get the base name used for expected files.
fn base_name(path: &Path) -> String {
    let file_name = path.file_name().unwrap().to_str().unwrap();
    file_name
        .strip_suffix(".fst")
        .or_else(|| file_name.strip_suffix(".vcd"))
        .expect("test file must have .fst or .vcd extension")
        .to_string()
}

/// For each .fst/.vcd file, compare `write_names --sort` output against
/// `tests/data/expected/{base}.names`.
fn test_names(path: &Path) -> datatest_stable::Result<()> {
    let base = base_name(path);
    let expected_path = path
        .parent()
        .unwrap()
        .join(format!("expected/{}.names", base));
    let expected = std::fs::read_to_string(&expected_path).map_err(|e| {
        format!(
            "Missing expected file {}: {} — generate it with: wavecat --names --sort {}",
            expected_path.display(),
            e,
            path.display(),
        )
    })?;

    let name_options = NameOptions::default();
    let (_, handle_to_names) = open_wave_file(path, &name_options)?;

    let mut output = Vec::new();
    write_names(&mut output, &handle_to_names, true)?;
    let actual = String::from_utf8(output)?;

    assert_eq!(actual, expected, "names mismatch for {}", path.display());
    Ok(())
}

/// For each .fst/.vcd file, compare sorted signal dump against
/// `tests/data/expected/{base}.cat`.
fn test_cat(path: &Path) -> datatest_stable::Result<()> {
    let base = base_name(path);
    let expected_path = path
        .parent()
        .unwrap()
        .join(format!("expected/{}.cat", base));
    let expected = std::fs::read_to_string(&expected_path).map_err(|e| {
        format!(
            "Missing expected file {}: {} — generate it with: wavecat --sort {}",
            expected_path.display(),
            e,
            path.display(),
        )
    })?;

    let name_options = NameOptions::default();
    let (mut reader, handle_to_names) = open_wave_file(path, &name_options)?;

    let options = SignalOutputOptions {
        sort: true,
        time_pound: false,
    };
    let mut output = Vec::new();
    write_signals_wave(&mut output, &mut reader, &handle_to_names, 0, None, &options)?;
    let actual = String::from_utf8(output)?;

    assert_eq!(actual, expected, "cat mismatch for {}", path.display());
    Ok(())
}

datatest_stable::harness! {
    { test = test_names, root = "tests/data", pattern = r"^[^/]+\.(fst|vcd)$" },
    { test = test_cat,   root = "tests/data", pattern = r"^[^/]+\.(fst|vcd)$" },
}
