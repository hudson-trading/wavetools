//------------------------------------------------------------------------------
// copyright_test.rs
// Ensures all source files contain the HRT MIT copyright header
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use std::path::Path;

const REQUIRED_LINES: &[&str] = &[
    "SPDX-FileCopyrightText: Hudson River Trading",
    "SPDX-License-Identifier: MIT",
];

fn collect_rs_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            // Skip vendored third-party code
            if path.ends_with("vcd") {
                continue;
            }
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn test_all_source_files_have_copyright() {
    let mut files = Vec::new();
    collect_rs_files(Path::new("src"), &mut files);
    collect_rs_files(Path::new("tests"), &mut files);
    files.sort();

    assert!(!files.is_empty(), "No .rs files found");

    let mut missing = Vec::new();
    for file in &files {
        let contents = std::fs::read_to_string(file).unwrap();
        for line in REQUIRED_LINES {
            if !contents.contains(line) {
                missing.push(format!("{}: missing '{}'", file.display(), line));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "Files missing copyright header:\n  {}",
        missing.join("\n  "),
    );
}
