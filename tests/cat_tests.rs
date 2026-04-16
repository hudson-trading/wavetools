//------------------------------------------------------------------------------
// cat_tests.rs
// Data-driven tests comparing wavecat output against expected golden files
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use std::path::Path;

use wavetools::{names_only, open_wave_file, write_attrs, write_names, write_signals_wave, NameOptions, SignalOutputOptions};

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
    let (_, hier) = open_wave_file(path, &name_options)?;
    let handle_to_names = names_only(&hier.signal_map, &hier.names);

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
    let (mut reader, hier) = open_wave_file(path, &name_options)?;
    let handle_to_names = names_only(&hier.signal_map, &hier.names);

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

/// For each .fst/.vcd file, compare sorted `write_attrs` output against
/// `tests/data/expected/{filename}.attrs`.  Unlike names/cat tests, attrs
/// include the format extension because type metadata differs across formats
/// (e.g. FST preserves "reg"/"integer" while VCD uses "wire").
fn test_attrs(path: &Path) -> datatest_stable::Result<()> {
    let file_name = path.file_name().unwrap().to_str().unwrap();
    let expected_path = path
        .parent()
        .unwrap()
        .join(format!("expected/{}.attrs", file_name));
    let expected = std::fs::read_to_string(&expected_path).map_err(|e| {
        format!(
            "Missing expected file {}: {} — generate it with: wavecat --attrs --sort {}",
            expected_path.display(),
            e,
            path.display(),
        )
    })?;

    let name_options = NameOptions::default();
    let (_, hier) = open_wave_file(path, &name_options)?;

    let mut output = Vec::new();
    write_attrs(&mut output, &hier.signal_map, &hier.names, true)?;
    let actual = String::from_utf8(output)?;

    assert_eq!(actual, expected, "attrs mismatch for {}", path.display());
    Ok(())
}

datatest_stable::harness! {
    { test = test_names, root = "tests/data", pattern = r"^[^/]+\.(fst|vcd)$" },
    { test = test_cat,   root = "tests/data", pattern = r"^[^/]+\.(fst|vcd)$" },
    { test = test_attrs, root = "tests/data", pattern = r"^[^/]+\.(fst|vcd)$" },
}
