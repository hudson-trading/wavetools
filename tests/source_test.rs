//------------------------------------------------------------------------------
// source_test.rs
// Source file hygiene: copyright headers, ASCII-only content
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use std::path::Path;

const REQUIRED_LINES: &[&str] = &[
    "SPDX-FileCopyrightText: Hudson River Trading",
    "SPDX-License-Identifier: MIT",
];

const SKIP_DIRS: &[&str] = &[".git", "target"];

/// Returns true if the file appears to be binary (contains a NUL byte in the
/// first 8 KB, same heuristic as `grep -I`).
fn is_binary(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return true;
    };
    let mut buf = [0u8; 8192];
    let Ok(n) = f.read(&mut buf) else {
        return true;
    };
    buf[..n].contains(&0)
}

fn collect_files(dir: &Path, files: &mut Vec<std::path::PathBuf>, rs_only: bool) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap().to_str().unwrap();
            if SKIP_DIRS.contains(&name) {
                continue;
            }
            // Skip vendored third-party code (copyright test only covers our files)
            if rs_only && name == "vcd" {
                continue;
            }
            collect_files(&path, files, rs_only);
        } else if path.is_file() {
            if rs_only {
                if path.extension().is_some_and(|ext| ext == "rs") {
                    files.push(path);
                }
            } else if !is_binary(&path) {
                files.push(path);
            }
        }
    }
}

#[test]
fn test_all_source_files_have_copyright() {
    let mut files = Vec::new();
    collect_files(Path::new("src"), &mut files, true);
    collect_files(Path::new("tests"), &mut files, true);
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

#[test]
fn test_no_non_ascii_characters() {
    let mut files = Vec::new();
    collect_files(Path::new("."), &mut files, false);
    files.sort();

    assert!(!files.is_empty(), "No text files found");

    let mut violations = Vec::new();
    for file in &files {
        let contents = std::fs::read_to_string(file).unwrap();
        for (line_num, line) in contents.lines().enumerate() {
            if line.bytes().any(|b| b > 127) {
                violations.push(format!(
                    "{}:{}: {}",
                    file.display(),
                    line_num + 1,
                    line,
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Non-ASCII characters found:\n  {}",
        violations.join("\n  "),
    );
}
