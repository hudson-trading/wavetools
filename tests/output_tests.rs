// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
mod common;
use common::{run_wavecat_cli as cat, run_wavediff_cli as diff};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wavetools-output-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().into()
    }
    fn input(&self, text: &str) -> String {
        let path = self.path("input.vcd");
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn success(out: std::process::Output) -> String {
    assert!(
        out.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn roundtrip_formats_and_aliases() {
    let temp = Temp::new();
    for input in [
        "counter.vcd",
        "counter.fst",
        "alias_types.vcd",
        "escaped_names.vcd",
        "range.vcd",
    ] {
        let input = format!("tests/data/{input}");
        for ext in ["vcd", "fst"] {
            let out = temp.path(&format!("out.{ext}"));
            assert!(success(cat(&[&input, "-o", &out])).is_empty());
            success(diff(&[&input, &out]));
        }
    }
}

#[test]
fn filtering_and_overlay() {
    let temp = Temp::new();
    for ext in ["vcd", "fst"] {
        let out = temp.path(&format!("out.{ext}"));
        success(cat(&[
            "tests/data/set_clk.vcd",
            "tests/data/set_counter.vcd",
            "--out",
            &out,
        ]));
        success(diff(&["tests/data/counter.vcd", &out]));
        success(cat(&[
            "tests/data/counter.vcd",
            "--filter",
            "*.clk",
            "-o",
            &out,
        ]));
        let names = success(cat(&["--names", &out]));
        assert_eq!(names, "t.clk\n");
    }
}

#[test]
fn output_format_selection() {
    let temp = Temp::new();
    let input = "tests/data/counter.vcd";
    for (name, args) in [
        ("out.FST", vec![]),
        ("out.dat", vec!["--out-format", "fst"]),
        ("out.fst", vec!["--out-format", "vcd"]),
    ] {
        let out = temp.path(name);
        let mut options = vec![input, "-o", &out];
        options.extend(args);
        success(cat(&options));
        success(diff(&[input, &out]));
    }
    for options in [
        vec![input, "--out-format", "vcd"],
        vec![input, "-o", "out.unknown"],
        vec![input, "-o", "out.vcd", "--names"],
        vec![input, "-o", "out.vcd", "--attrs"],
        vec![input, "-o", "out.vcd", "--out-format", "bad"],
    ] {
        assert!(!cat(&options).status.success());
    }
}

#[test]
fn slices_keep_initial_state_and_absolute_time() {
    let temp = Temp::new();
    let input = temp.input("$timescale 10 ps $end\n$scope module top $end\n$var wire 1 ! a $end\n$var wire 1 @ b $end\n$upscope $end\n$enddefinitions $end\n#0\n0!\n1@\n#10\n1!\n#20\n0!\n");
    for ext in ["vcd", "fst"] {
        let out = temp.path(&format!("slice.{ext}"));
        success(cat(&[&input, "-s", "5", "-e", "10", "-o", &out]));
        assert_eq!(
            success(cat(&[&out, "--sort", "-s", "5"])),
            "5 top.a 0\n5 top.b 1\n10 top.a 1\n"
        );
        let back = temp.path("back.vcd");
        success(cat(&[&out, "-o", &back]));
        assert!(fs::read_to_string(back)
            .unwrap()
            .contains("$timescale\n10ps\n$end"));
        success(cat(&[&input, "-s", "25", "-e", "30", "-o", &out]));
        assert_eq!(
            success(cat(&[&out, "--sort", "-s", "25"])),
            "25 top.a 0\n25 top.b 1\n"
        );
    }
}

#[test]
fn real_string_and_top_level_vcd() {
    let temp = Temp::new();
    let input = temp.input("$var real 64 ! r $end\n$var string 0 @ text $end\n$enddefinitions $end\n#0\nr1.5 !\nshello @\n#5\nr-2.25 !\nsworld @\n");
    let out = temp.path("out.vcd");
    success(cat(&["tests/data/wide_bits.vcd", "-o", &out]));
    success(diff(&["tests/data/wide_bits.vcd", &out]));
    success(cat(&[&input, "-o", &out]));
    success(diff(&[&input, &out]));
    let out = temp.path("out.fst");
    let error = cat(&[&input, "-o", &out]);
    assert!(!error.status.success());
    assert!(String::from_utf8_lossy(&error.stderr).contains("does not support"));
    assert!(!std::path::Path::new(&out).exists());
}

#[test]
fn mismatched_timescales_and_same_file_are_rejected() {
    let temp = Temp::new();
    let text = "$timescale 1 ps $end\n$var wire 1 ! other $end\n$enddefinitions $end\n#0\n1!\n";
    let input = temp.input(text);
    let out = temp.path("out.vcd");
    let result = cat(&[&input, "tests/data/counter.vcd", "-o", &out]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("different timescales"));
    assert!(!cat(&[&input, "-o", &input]).status.success());
    assert_eq!(fs::read_to_string(&input).unwrap(), text);
    let alias = temp.path("alias.vcd");
    fs::hard_link(&input, &alias).unwrap();
    assert!(!cat(&[&input, "-o", &alias]).status.success());
    assert_eq!(fs::read_to_string(&input).unwrap(), text);
}

#[test]
fn empty_and_zero_time_exports() {
    let temp = Temp::new();
    let input = temp.input("$var wire 1 ! a $end\n$enddefinitions $end\n#0\n1!\n");
    for ext in ["vcd", "fst"] {
        let out = temp.path(&format!("out.{ext}"));
        success(cat(&[&input, "-o", &out]));
        assert_eq!(success(cat(&[&out])), "0 a 1\n");
        success(cat(&[&input, "--filter", "missing", "-o", &out]));
        assert_eq!(success(cat(&[&out])), "");
    }
}

#[test]
fn malformed_data_and_output_errors() {
    let temp = Temp::new();
    let input = temp.input("$var wire 1 ! a $end\n$enddefinitions $end\n#0\n1!\n#bad\n");
    for ext in ["vcd", "fst"] {
        assert!(!cat(&[&input, "-o", &temp.path(&format!("out.{ext}"))])
            .status
            .success());
        assert!(!cat(&[
            "tests/data/counter.vcd",
            "-o",
            &temp.path(&format!("missing/out.{ext}"))
        ])
        .status
        .success());
    }
}

#[test]
fn four_state_vectors_and_final_timestamp() {
    let temp = Temp::new();
    let input = temp.input("$timescale 100 fs $end\n$var wire 8 ! bus $end\n$enddefinitions $end\n#0\nbx !\n#5\nbz !\n#10\nb10 !\n#50\n");
    for ext in ["vcd", "fst"] {
        let out = temp.path(&format!("out.{ext}"));
        success(cat(&[&input, "-o", &out]));
        let expected = if ext == "fst" {
            "0 bus xxxxxxxx\n5 bus zzzzzzzz\n10 bus 00000010\n"
        } else {
            "0 bus x\n5 bus z\n10 bus 10\n"
        };
        assert_eq!(success(cat(&[&out])), expected);
        let back = temp.path("back.vcd");
        success(cat(&[&out, "-o", &back]));
        let text = fs::read_to_string(back).unwrap();
        assert!(text.contains("$timescale\n100fs\n$end"));
        assert!(text.ends_with("#50\n"));
    }
}

#[test]
fn fst_export_preserves_annotations_and_scope_types() {
    let temp = Temp::new();
    for fixture in ["enums", "structs", "struct_arrays"] {
        let source = format!("tests/data/export/{fixture}.fst");
        let out = temp.path("metadata.vcd");
        success(cat(&[&source, "-o", &out]));
        success(diff(&[&source, &out]));
        let text = fs::read_to_string(&out).unwrap();
        match fixture {
            "enums" => {
                assert!(text.contains(
                    "$attrbegin misc 07 $unit::state_t 4 VAL_A VAL_B VAL_C VAL_D 00 01 10 11"
                ));
                assert!(text.contains("$scope interface sink $end"));
            }
            "structs" => {
                assert!(text.contains("$scope struct v_strp $end"));
                assert!(text.contains("$attrbegin pack packed members 2 $end"));
            }
            "struct_arrays" => assert!(text.contains("$scope sv_array")),
            _ => unreachable!(),
        }
        // Reading and rewriting the VCD must also retain its header metadata.
        let again = temp.path("again.vcd");
        success(cat(&[&out, "-o", &again]));
        assert_eq!(
            fs::read_to_string(again)
                .unwrap()
                .split("$enddefinitions")
                .next(),
            text.split("$enddefinitions").next()
        );
    }
}

#[test]
fn vcd_export_preserves_empty_scopes_and_attribute_boundaries() {
    let temp = Temp::new();
    let header = "$timescale\n1ns\n$end\n $scope module top $end\n  $attrbegin pack packed members 1 $end\n  $scope struct packet $end\n   $var logic 1 ! valid $end\n  $upscope $end\n  $attrend $end\n  $scope interface empty $end\n  $upscope $end\n $upscope $end\n";
    let input = temp.input(&format!("{header}$enddefinitions $end\n#0\n1!\n"));
    let out = temp.path("out.vcd");
    success(cat(&[&input, "-o", &out]));
    assert!(fs::read_to_string(&out).unwrap().contains(header));
    success(diff(&[&input, &out]));
}

#[test]
fn vcd_overlay_remaps_enum_table_ids() {
    let temp = Temp::new();
    let mut inputs = Vec::new();
    for scope in ["a", "b"] {
        let input = temp.path(&format!("{scope}.vcd"));
        fs::write(&input, format!("$scope module {scope} $end\n$attrbegin misc 07 {scope}::kind 2 ZERO ONE 0 1 1 $end\n$var logic 1 ! data $end\n$attrbegin misc 07 \"\" 1 $end\n$upscope $end\n$enddefinitions $end\n#0\n1!\n")).unwrap();
        inputs.push(input);
    }
    let out = temp.path("out.vcd");
    success(cat(&[&inputs[0], &inputs[1], "-o", &out]));
    success(diff(&[
        "--set1", &inputs[0], "--set1", &inputs[1], "--set2", &out,
    ]));
    let text = fs::read_to_string(out).unwrap();
    assert!(text.contains("$attrbegin misc 07 \"\" 2 $end"));
}

#[test]
fn export_keeps_nonzero_first_timestamp() {
    let temp = Temp::new();
    let input = temp.input("$var wire 1 ! a $end\n$enddefinitions $end\n#100\n1!\n#150\n");
    let out = temp.path("out.vcd");
    success(cat(&[&input, "-o", &out]));
    let text = fs::read_to_string(out).unwrap();
    assert!(!text.contains("#0\n"));
    assert!(text.contains("#100\n"));
    assert!(text.ends_with("#150\n"));
}

#[test]
fn filtering_keeps_attributes_with_their_variables() {
    let temp = Temp::new();
    // b shares a's value handle, but only b has these declaration attributes.
    // c still needs the enum definition that follows the removed declaration.
    let input = temp.input("$scope module top $end\n$var wire 1 ! a $end\n$var wire 1 ! b $end\n$attrbegin misc 07 kind 2 ZERO ONE 0 1 7 $end\n$attrbegin misc 07 \"\" 7 $end\n$attrbegin array unpacked bounds 2 $end\n$attrend $end\n$var wire 1 @ c $end\n$attrbegin misc 07 \"\" 7 $end\n$upscope $end\n$enddefinitions $end\n#0\n0!\n1@\n");
    for filter in ["top.a", "top.c", "top.a top.c", "top.b", "missing"] {
        let out = temp.path("filtered.vcd");
        success(cat(&[&input, "--filter", filter, "-o", &out]));
        assert_eq!(
            success(cat(&[&input, "--filter", filter, "--attrs", "--sort"])),
            success(cat(&[&out, "--attrs", "--sort"]))
        );
        success(diff(&[&input, &out, "--filter", filter]));
    }
}

#[test]
fn top_level_overlay_does_not_transfer_attributes_between_files() {
    let temp = Temp::new();
    let a = temp.input("$var wire 1 ! a $end\n$enddefinitions $end\n#0\n0!\n");
    let b = temp.path("b.vcd");
    fs::write(&b, "$attrbegin misc 07 kind 2 ZERO ONE 0 1 1 $end\n$var wire 1 ! b $end\n$attrbegin misc 07 \"\" 1 $end\n$enddefinitions $end\n#0\n1!\n").unwrap();
    for inputs in [[&a, &b], [&b, &a]] {
        let out = temp.path("overlay.vcd");
        success(cat(&[inputs[0], inputs[1], "-o", &out]));
        success(diff(&["--set1", &a, "--set1", &b, "--set2", &out]));
    }
}

#[test]
fn filtering_fst_metadata_matches_selected_source_declarations() {
    let temp = Temp::new();
    for fixture in ["enums", "structs", "struct_arrays"] {
        let source = format!("tests/data/export/{fixture}.fst");
        for filter in ["*.clk", "*.v_enumed*", "*.v_strp*"] {
            let out = temp.path("filtered.vcd");
            success(cat(&[&source, "--filter", filter, "-o", &out]));
            assert_eq!(
                success(cat(&[&source, "--filter", filter, "--names", "--sort"])),
                success(cat(&[&out, "--names", "--sort"]))
            );
            // VCD omits directions; wavediff compares the remaining metadata.
            success(diff(&[&source, &out, "--filter", filter]));
        }
    }
}

#[test]
fn vcd_export_copies_fst_file_metadata() {
    let temp = Temp::new();
    let source = "tests/data/export/enums.fst";
    let reader =
        fst_reader::FstReader::open(std::io::BufReader::new(fs::File::open(source).unwrap()))
            .unwrap();
    let header = reader.get_header();
    assert!(!header.date.is_empty());
    assert!(!header.version.is_empty());
    assert_eq!(header.timescale_exponent, -12);
    let out = temp.path("metadata.vcd");
    success(cat(&[source, "-o", &out]));
    let expected = format!(
        "$date\n{}\n$end\n$version\n{}\n$end\n$timescale\n1ps\n$end\n",
        header.date.trim(),
        header.version.trim()
    );
    assert!(fs::read_to_string(&out).unwrap().starts_with(&expected));
    success(diff(&[source, &out]));
}

#[test]
fn vcd_export_uses_first_input_metadata_without_inventing_missing_fields() {
    let temp = Temp::new();
    let first = temp.input("$date\nOriginal date\n$end\n$version\nOriginal producer\n$end\n$var wire 1 ! a $end\n$enddefinitions $end\n#0\n0!\n");
    let second = temp.path("second.vcd");
    fs::write(
        &second,
        "$var wire 1 ! b $end\n$enddefinitions $end\n#0\n1!\n",
    )
    .unwrap();
    let out = temp.path("metadata.vcd");
    for inputs in [vec![first.as_str()], vec![first.as_str(), second.as_str()]] {
        let mut args = inputs;
        args.extend(["-o", &out]);
        success(cat(&args));
        assert!(fs::read_to_string(&out)
            .unwrap()
            .starts_with("$date\nOriginal date\n$end\n$version\nOriginal producer\n$end\n"));
    }
    success(cat(&[&second, &first, "-o", &out]));
    let text = fs::read_to_string(out).unwrap();
    assert!(!text.contains("$date"));
    assert!(!text.contains("$version"));
}

#[test]
fn vcd_initial_values_are_enclosed_in_dumpvars() {
    let temp = Temp::new();
    for start in [0, 100] {
        for later_change in [false, true] {
            let later = if later_change {
                format!("#{}\n1!\n", start + 10)
            } else {
                String::new()
            };
            let input = temp.input(&format!(
                "$var wire 1 ! a $end\n$enddefinitions $end\n#{start}\n0!\n{later}"
            ));
            let out = temp.path("out.vcd");
            success(cat(&[&input, "-o", &out]));
            let text = fs::read_to_string(&out).unwrap();
            let body = text.split_once("$enddefinitions $end\n").unwrap().1;
            assert_eq!(body, format!("#{start}\n$dumpvars\n0!\n$end\n{later}"));
            success(diff(&[&input, &out]));
        }
    }
}

#[test]
fn sliced_and_empty_vcd_initial_blocks_are_closed() {
    let temp = Temp::new();
    let input = temp.input("$var wire 1 ! a $end\n$enddefinitions $end\n#0\n0!\n#10\n1!\n");
    let out = temp.path("out.vcd");
    success(cat(&[&input, "--start", "5", "--end", "10", "-o", &out]));
    assert!(fs::read_to_string(&out)
        .unwrap()
        .ends_with("#5\n$dumpvars\n0!\n$end\n#10\n1!\n"));
    success(cat(&[&input, "--filter", "missing", "-o", &out]));
    assert!(fs::read_to_string(&out)
        .unwrap()
        .ends_with("#0\n$dumpvars\n$end\n#10\n"));
}

#[test]
fn fst_to_vcd_initial_frame_uses_dumpvars() {
    let temp = Temp::new();
    let out = temp.path("out.vcd");
    let source = "tests/data/counter.fst";
    success(cat(&[source, "-o", &out]));
    let text = fs::read_to_string(&out).unwrap();
    let body = text.split_once("$enddefinitions $end\n").unwrap().1;
    assert!(body.starts_with("#0\n$dumpvars\n"));
    let (initial, later) = body.split_once("$end\n").unwrap();
    assert!(!initial
        .strip_prefix("#0\n")
        .unwrap()
        .lines()
        .any(|line| line.starts_with('#')));
    assert!(later.starts_with("#10\n"));
    assert_eq!(body.matches("$dumpvars").count(), 1);
    success(diff(&[source, &out]));
}

#[test]
fn filtering_preserves_scope_annotations() {
    let temp = Temp::new();
    let input = temp.input("$scope module top $end\n$var wire 1 ! drop $end\n$attrbegin pack packed members 1 $end\n$attrbegin array unpacked bounds 2 $end\n$scope struct kept $end\n$var wire 1 @ value $end\n$upscope $end\n$attrend $end\n$attrend $end\n$upscope $end\n$enddefinitions $end\n#0\n0!\n1@\n");
    let out = temp.path("filtered.vcd");
    success(cat(&[&input, "--filter", "top.kept.*", "-o", &out]));
    let text = fs::read_to_string(&out).unwrap();
    assert!(text.contains("$attrbegin pack packed members 1 $end\n  $attrbegin array unpacked bounds 2 $end\n  $scope struct kept $end"));
    assert_eq!(text.matches("$attrend").count(), 2);
    success(cat(&[
        "tests/data/export/structs.fst",
        "--filter",
        "top.t.v_strp.*",
        "-o",
        &out,
    ]));
    let text = fs::read_to_string(out).unwrap();
    assert!(text.contains("$attrbegin pack packed members 2 $end\n   $scope struct v_strp $end"));
}

#[test]
fn filtering_preserves_shared_source_paths() {
    let temp = Temp::new();
    let input = temp.input("$scope module top $end\n$var wire 1 ! drop $end\n$attrbegin misc 03 design.sv 7 $end\n$scope module kept $end\n$attrbegin misc 04 7 12 $end\n$attrbegin misc 05 7 20 $end\n$var wire 1 @ value $end\n$upscope $end\n$upscope $end\n$enddefinitions $end\n#0\n0!\n1@\n");
    let out = temp.path("filtered.vcd");
    success(cat(&[&input, "--filter", "top.kept.*", "-o", &out]));
    let text = fs::read_to_string(out).unwrap();
    assert!(text.contains("$attrbegin misc 03 design.sv 1 $end"));
    assert!(text.contains("$attrbegin misc 04 1 12 $end"));
    assert!(text.contains("$attrbegin misc 05 1 20 $end"));
}

#[test]
fn fst_rejects_events_without_replacing_output() {
    let temp = Temp::new();
    // The event aliases a wire: every declaration must be validated, not just
    // the representative variable for the shared value handle.
    let input = temp.input("$var wire 1 ! a $end\n$var event 1 ! tick $end\n$enddefinitions $end\n#0\n1!\n#5\n1!\n#10\n1!\n");
    let out = temp.path("out.fst");
    for existing in [false, true] {
        if existing {
            fs::write(&out, "original waveform").unwrap();
        }
        let result = cat(&[&input, "-o", &out]);
        assert!(!result.status.success());
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains("fst-writer does not support exporting 'event' signals"));
        assert!(stderr.contains("--out-format vcd"));
        if existing {
            assert_eq!(fs::read_to_string(&out).unwrap(), "original waveform");
        } else {
            assert!(!std::path::Path::new(&out).exists());
        }
    }
    // Filtering out the event still permits FST export.
    success(cat(&[&input, "--filter", "a", "-o", &out]));
    let out = temp.path("out.vcd");
    success(cat(&[&input, "--filter", "tick", "-o", &out]));
    assert_eq!(success(cat(&[&out])), "0 tick 1\n5 tick 1\n10 tick 1\n");
}

#[test]
fn no_date_omits_recording_time_without_changing_waveform() {
    let temp = Temp::new();
    let input = temp.input("$date\nThu Oct 22 08:01:32 2026\n$end\n$version\nTest producer\n$end\n$timescale 1ns $end\n$var wire 1 ! a $end\n$enddefinitions $end\n#0\n0!\n#10\n1!\n");
    assert!(!cat(&[&input, "--no-date"]).status.success());
    for source in [input.as_str(), "tests/data/counter.fst"] {
        let dated = temp.path("dated.vcd");
        success(cat(&[source, "-o", &dated]));
        let dated_text = fs::read_to_string(&dated).unwrap();
        assert!(dated_text.contains("$date\n"));
        for ext in ["vcd", "fst"] {
            let out = temp.path(&format!("no-date.{ext}"));
            success(cat(&[source, "--no-date", "-o", &out]));
            success(diff(&[source, &out]));
            if ext == "vcd" {
                let text = fs::read_to_string(&out).unwrap();
                assert!(!text.contains("$date"));
                // Removing the date must leave version, timescale, and data intact.
                let date_start = dated_text.find("$date\n").unwrap();
                let date_end =
                    date_start + dated_text[date_start..].find("$end\n").unwrap() + "$end\n".len();
                assert_eq!(
                    text,
                    format!("{}{}", &dated_text[..date_start], &dated_text[date_end..])
                );
            } else {
                let reader = fst_reader::FstReader::open(std::io::BufReader::new(
                    fs::File::open(&out).unwrap(),
                ))
                .unwrap();
                assert!(reader.get_header().date.is_empty());
            }
        }
    }
    let out = temp.path("overlay.vcd");
    success(cat(&[
        &input,
        "tests/data/counter.fst",
        "--no-date",
        "-o",
        &out,
    ]));
    assert!(!fs::read_to_string(out).unwrap().contains("$date"));
}

#[test]
fn fst_output_copies_first_input_date() {
    let temp = Temp::new();
    let date = "Thu Oct 22 08:01:32 2026";
    let input = temp.input(&format!(
        "$date\n{date}\n\n$end\n$var wire 1 ! a $end\n$enddefinitions $end\n#0\n0!\n#10\n1!\n"
    ));
    let undated = temp.path("undated.vcd");
    fs::write(
        &undated,
        "$var wire 1 ! b $end\n$enddefinitions $end\n#0\n0!\n",
    )
    .unwrap();
    let dated_fst = temp.path("dated.fst");
    success(cat(&[&input, "-o", &dated_fst]));
    let out = temp.path("out.fst");
    for (sources, expected) in [
        (vec![input.as_str()], date),
        (vec![dated_fst.as_str()], date),
        (vec![input.as_str(), undated.as_str()], date),
        (vec![undated.as_str(), input.as_str()], ""),
        (vec![undated.as_str()], ""),
    ] {
        let mut args = sources;
        args.extend(["-o", &out]);
        success(cat(&args));
        let reader =
            fst_reader::FstReader::open(std::io::BufReader::new(fs::File::open(&out).unwrap()))
                .unwrap();
        assert_eq!(reader.get_header().date, expected);
    }
    // Different recording dates do not affect waveform comparison.
    success(cat(&[&input, "--no-date", "-o", &out]));
    success(diff(&[&dated_fst, &out]));
}
