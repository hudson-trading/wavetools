//------------------------------------------------------------------------------
// wavediff.rs
// CLI tool to compare two waveform files and report differences
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use clap::Parser;
use std::io::Write;
use std::path::{Path, PathBuf};
use wavetools::{
    apply_filter, compare_signal_meta, compare_signal_names, diff_wave_sets_with_report,
    open_and_read_wave_sets, parse_filter_patterns, retain_common_signals, DiffOptions,
    DiffOutputOptions, DiffSide, FstDiffOutput, FstDiffSide, NameOptions, SidePair, WaveHierarchy,
};

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (rev ",
    env!("WAVETOOLS_GIT_REV"),
    ")",
);

#[derive(Parser, Debug)]
#[command(name = "wavediff")]
#[command(version = VERSION)]
#[command(
    about = "Compare two waveform files (FST or VCD)",
    long_about = "\
Compare two waveform files (FST or VCD format) by signal name and value.
Multiple files can be combined into each side using --set1 and --set2.
When both --set1 and --set2 are provided, positional FILE arguments are not needed.

Exit codes:
  0  files are identical
  1  differences found
  2  error

Examples:
  wavediff baseline.fst current.fst
  wavediff --start 100 --end 500 sim1.vcd sim2.vcd
  wavediff --epsilon 0.001 analog1.fst analog2.vcd
  wavediff --ignore-xz sim1.fst sim2.fst
  wavediff --ignore-missing sim1.fst sim2.fst
  wavediff --fst-diff diff.fst baseline.fst current.fst
  wavediff --set1 extra1.vcd baseline.vcd current.vcd
  wavediff --set1 clk.vcd --set1 regs.vcd --set2 counter.vcd
  wavediff --set1 clk.vcd --set1 regs.vcd --set2 clk.vcd --set2 regs_new.vcd baseline.vcd current.vcd"
)]
struct Args {
    /// First waveform file to compare (FST or VCD)
    file1: Option<PathBuf>,

    /// Second waveform file to compare (FST or VCD)
    file2: Option<PathBuf>,

    /// Start time for comparison
    #[arg(short = 's', long)]
    start: Option<u64>,

    /// End time for comparison
    #[arg(short = 'e', long)]
    end: Option<u64>,

    /// Epsilon for comparing real-valued signals (absolute tolerance)
    #[arg(long)]
    epsilon: Option<f64>,

    /// Skip metadata comparison (type, size, direction, attributes)
    #[arg(long)]
    no_attrs: bool,

    /// File(s) for set 1; may be specified multiple times
    #[arg(long, action = clap::ArgAction::Append)]
    set1: Vec<PathBuf>,

    /// File(s) for set 2; may be specified multiple times
    #[arg(long, action = clap::ArgAction::Append)]
    set2: Vec<PathBuf>,

    /// Filter signals by glob pattern(s); may be specified multiple times or as a
    /// space-separated list (e.g. --filter "*.foo *.bar" or --filter "*.foo" --filter "*.bar")
    #[arg(short, long, action = clap::ArgAction::Append)]
    filter: Vec<String>,

    /// Ignore signals that are missing from either input
    #[arg(long)]
    ignore_missing: bool,

    /// Ignore value differences only for bits where either side is X or Z
    #[arg(long = "ignore-xz")]
    ignore_xz: bool,

    /// Write an FST containing only signals that had value differences
    #[arg(long = "fst-diff")]
    fst_diff: Option<PathBuf>,
}

fn main() {
    let args = Args::parse();

    match run(args) {
        Ok(has_differences) => {
            if has_differences {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(2);
        }
    }
}

fn report_name_mismatch(
    file1: &std::path::Path,
    only_in_1: &std::collections::HashSet<String>,
    file2: &std::path::Path,
    only_in_2: &std::collections::HashSet<String>,
) -> bool {
    if only_in_1.is_empty() && only_in_2.is_empty() {
        return false;
    }
    eprintln!("Signal name mismatch:");
    let print_sorted = |label: std::path::Display, names: &std::collections::HashSet<String>| {
        eprintln!("  Only in {}:", label);
        let mut names: Vec<_> = names.iter().collect();
        names.sort();
        for name in names {
            eprintln!("    {}", name);
        }
    };
    if !only_in_1.is_empty() {
        print_sorted(file1.display(), only_in_1);
    }
    if !only_in_2.is_empty() {
        print_sorted(file2.display(), only_in_2);
    }
    true
}

fn report_meta_diffs(hier1: &WaveHierarchy, hier2: &WaveHierarchy) -> bool {
    let meta_diffs = compare_signal_meta(hier1, hier2);
    if !meta_diffs.is_empty() {
        let mut stderr = std::io::stderr();
        for diff in &meta_diffs {
            let _ = writeln!(stderr, "{}", diff);
        }
        true
    } else {
        false
    }
}

fn side_label(path: &Path, fallback: &str) -> String {
    path.file_stem()
        .or_else(|| path.file_name())
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn run(args: Args) -> Result<bool, String> {
    if let (Some(s), Some(e)) = (args.start, args.end) {
        if s > e {
            return Err(format!("--start ({}) must be <= --end ({})", s, e));
        }
    }
    if let Some(eps) = args.epsilon {
        if eps < 0.0 {
            return Err(format!("--epsilon must be non-negative, got {}", eps));
        }
    }

    // Build path lists: positional FILE1/FILE2 go first, then --set1/--set2.
    // Positional args are required unless both --set1 and --set2 are non-empty.
    let both_sets = !args.set1.is_empty() && !args.set2.is_empty();
    let mut set1_paths: Vec<PathBuf> = Vec::new();
    let mut set2_paths: Vec<PathBuf> = Vec::new();

    match (&args.file1, &args.file2) {
        (Some(f1), Some(f2)) => {
            set1_paths.push(f1.clone());
            set2_paths.push(f2.clone());
        }
        (Some(_), None) => {
            return Err("FILE2 is required when FILE1 is provided".to_string());
        }
        (None, _) if !both_sets => {
            return Err(
                "FILE1 and FILE2 are required unless both --set1 and --set2 are provided"
                    .to_string(),
            );
        }
        _ => {}
    }

    set1_paths.extend(args.set1.iter().cloned());
    set2_paths.extend(args.set2.iter().cloned());

    let name_options = NameOptions::default();
    let patterns = parse_filter_patterns(&args.filter)?;
    let paths1: Vec<&std::path::Path> = set1_paths.iter().map(PathBuf::as_path).collect();
    let paths2: Vec<&std::path::Path> = set2_paths.iter().map(PathBuf::as_path).collect();

    let mut sets = open_and_read_wave_sets(&paths1, &paths2, &name_options)?;
    apply_filter(&mut sets.hier1, &patterns);
    apply_filter(&mut sets.hier2, &patterns);

    // For name-mismatch reporting, use FILE1/FILE2 if given, else first --set file
    let label1 = args.file1.as_deref().unwrap_or(set1_paths[0].as_path());
    let label2 = args.file2.as_deref().unwrap_or(set2_paths[0].as_path());

    let mut has_differences = false;
    if args.ignore_missing {
        retain_common_signals(&mut sets.hier1, &mut sets.hier2);
    } else {
        let (only_in_1, only_in_2) = compare_signal_names(&sets.hier1, &sets.hier2);
        has_differences = report_name_mismatch(label1, &only_in_1, label2, &only_in_2);
    }

    if !args.no_attrs {
        has_differences |= report_meta_diffs(&sets.hier1, &sets.hier2);
    }

    let diff_options = DiffOptions {
        start: args.start.unwrap_or(0),
        end: args.end,
        real_epsilon: args.epsilon,
    };
    let fst_diff = args.fst_diff.as_ref().map(|path| {
        let mut side1 = side_label(label1, "file1");
        let mut side2 = side_label(label2, "file2");
        if side1 == side2 {
            side1.push_str("_file1");
            side2.push_str("_file2");
        }
        FstDiffOutput {
            path: path.clone(),
            sides: SidePair {
                side1: FstDiffSide {
                    label: side1,
                    paths: set1_paths.clone(),
                },
                side2: FstDiffSide {
                    label: side2,
                    paths: set2_paths.clone(),
                },
            },
            name_options: name_options.clone(),
        }
    });
    let mut stdout = std::io::stdout();
    let diff_report = diff_wave_sets_with_report(
        &mut stdout,
        sets,
        &diff_options,
        &DiffOutputOptions {
            ignore_xz: args.ignore_xz,
            fst_diff,
        },
    )
    .map_err(|e| format!("Failed to diff files: {}", e))?;

    if let Some(trim) = &diff_report.trim {
        let label = match trim.side {
            DiffSide::First => label1.display(),
            DiffSide::Second => label2.display(),
        };
        eprintln!(
            "Ignored trailing samples in {} after time {} to match the shorter input",
            label, trim.trim_time
        );
    }

    Ok(has_differences || diff_report.has_differences)
}
