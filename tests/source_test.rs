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
    // Honor .gitignore even in source archives without a Git checkout.
    let walker = ignore::WalkBuilder::new(dir)
        .require_git(false)
        .hidden(false)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        // Source archives may omit .gitignore; always exclude build output.
        .filter_entry(|entry| entry.file_name() != ".git" && entry.file_name() != "target")
        .build();
    for entry in walker {
        let entry = entry.unwrap_or_else(|err| panic!("Failed to walk {}: {err}", dir.display()));
        let path = entry.path();
        if path.is_file() {
            if rs_only {
                // The copyright test excludes vendored third-party code.
                if path.extension().is_some_and(|ext| ext == "rs")
                    && !path.components().any(|part| part.as_os_str() == "vcd")
                {
                    files.push(path.to_path_buf());
                }
            } else if !is_binary(path) {
                files.push(path.to_path_buf());
            }
        }
    }
    files.sort();
    files.dedup();
}

#[test]
fn test_collect_files_honors_gitignore_without_git() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("wavetools-hygiene-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(root.join(".gitignore"), "*.log\n").unwrap();
    std::fs::write(root.join("nested/.gitignore"), "!keep.log\n").unwrap();
    for name in [
        ".hidden",
        "skip.log",
        "nested/skip.log",
        "nested/keep.log",
        "target/build.txt",
    ] {
        std::fs::write(root.join(name), "text\n").unwrap();
    }

    let mut files = Vec::new();
    collect_files(&root, &mut files, false);
    let relative: Vec<_> = files
        .iter()
        .map(|path| path.strip_prefix(&root).unwrap().to_path_buf())
        .collect();
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(
        relative,
        [
            ".gitignore",
            ".hidden",
            "nested/.gitignore",
            "nested/keep.log"
        ]
        .map(std::path::PathBuf::from),
    );
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
        let contents = std::fs::read_to_string(file)
            .unwrap_or_else(|err| panic!("Failed to read {}: {err}", file.display()));
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
        let contents = std::fs::read_to_string(file)
            .unwrap_or_else(|err| panic!("Failed to read {}: {err}", file.display()));
        for (line_num, line) in contents.lines().enumerate() {
            if line.bytes().any(|b| b > 127) {
                violations.push(format!("{}:{}: {}", file.display(), line_num + 1, line,));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Non-ASCII characters found:\n  {}",
        violations.join("\n  "),
    );
}
