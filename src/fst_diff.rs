//------------------------------------------------------------------------------
// fst_diff.rs
// FST synthesis for waveform diffs
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crossbeam_channel as channel;

use crate::diff::{send_merged_wave_changes, OwnedSignalValue};
use crate::{vcd, NameOptions, VarEntry, VarMeta, WaveHierarchy, WaveReader};

type FstDiffSample = SidePair<Option<OwnedSignalValue>>;
type FstDiffTimeline = BTreeMap<u64, FstDiffSample>;
type FstDiffSingleTimeline = BTreeMap<u64, OwnedSignalValue>;

/// A value held independently for each of the two diff sides.
#[derive(Default)]
pub struct SidePair<T> {
    pub side1: T,
    pub side2: T,
}

impl<T> SidePair<T> {
    fn get(&self, is_side1: bool) -> &T {
        if is_side1 {
            &self.side1
        } else {
            &self.side2
        }
    }

    fn get_mut(&mut self, is_side1: bool) -> &mut T {
        if is_side1 {
            &mut self.side1
        } else {
            &mut self.side2
        }
    }
}

/// One side's inputs for an FST diff.
#[derive(Default)]
pub struct FstDiffSide {
    pub label: String,
    pub paths: Vec<PathBuf>,
}

/// Options for writing a waveform containing only differing signals.
pub struct FstDiffOutput {
    pub path: PathBuf,
    pub sides: SidePair<FstDiffSide>,
    pub name_options: NameOptions,
}

#[derive(Default)]
pub(crate) struct FstDiffRecorder {
    has_diffs: bool,
    side_signals: SidePair<BTreeSet<Vec<String>>>,
    matching_signals: BTreeSet<Vec<String>>,
    // Which side held X-like bits in masked matches. The comparison sets this so
    // shared signal values are copied from the clean side under --ignore-xz.
    masked_x_sides: SidePair<bool>,
}

impl FstDiffRecorder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record_diff(&mut self, segments: &[&str]) {
        if segments.is_empty() {
            return;
        }
        self.has_diffs = true;
        let segments: Vec<String> = segments.iter().map(|s| (*s).to_string()).collect();
        self.side_signals.side1.insert(segments.clone());
        self.side_signals.side2.insert(segments);
    }

    /// Record which side(s) held X-like bits across masked matches, so shared
    /// signal values are copied from the X-holding side under --ignore-xz.
    pub(crate) fn set_masked_x_sides(&mut self, sides: &SidePair<bool>) {
        self.masked_x_sides.side1 = sides.side1;
        self.masked_x_sides.side2 = sides.side2;
    }

    /// Which side to copy shared (matching) signal values from. Prefers the
    /// side that held the X-masked bits; defaults to side1.
    fn prefer_side1_for_shared(&self) -> bool {
        self.masked_x_sides.side1 || !self.masked_x_sides.side2
    }

    pub(crate) fn record_side1_only(&mut self, segments: &[&str]) {
        self.record_side_only(segments, true);
    }

    pub(crate) fn record_side2_only(&mut self, segments: &[&str]) {
        self.record_side_only(segments, false);
    }

    fn record_side_only(&mut self, segments: &[&str], is_side1: bool) {
        if segments.is_empty() {
            return;
        }
        self.has_diffs = true;
        self.side_signals
            .get_mut(is_side1)
            .insert(segments.iter().map(|s| (*s).to_string()).collect());
    }

    pub(crate) fn record_matching_candidates(
        &mut self,
        hier1: &WaveHierarchy,
        hier2: &WaveHierarchy,
    ) {
        for info in hier1.signal_map.values() {
            for var in &info.vars {
                let segments = hier1.names.segments(var.name);
                if segments.is_empty() {
                    continue;
                }
                let owned: Vec<String> = segments
                    .iter()
                    .map(|segment| (*segment).to_string())
                    .collect();
                if handle_var_for_segments(hier2, &owned).is_some() {
                    self.matching_signals.insert(owned);
                }
            }
        }
    }

    pub(crate) fn record_asymmetric_candidates(
        &mut self,
        hier1: &WaveHierarchy,
        hier2: &WaveHierarchy,
    ) {
        for info in hier1.signal_map.values() {
            for var in &info.vars {
                let segments = hier1.names.segments(var.name);
                if !segments.is_empty() && hier2.names.find(&segments).is_none() {
                    self.record_side1_only(&segments);
                }
            }
        }
        for info in hier2.signal_map.values() {
            for var in &info.vars {
                let segments = hier2.names.segments(var.name);
                if !segments.is_empty() && hier1.names.find(&segments).is_none() {
                    self.record_side2_only(&segments);
                }
            }
        }
    }

    pub(crate) fn signal_count(&self) -> usize {
        if !self.has_diffs {
            0
        } else {
            self.side_signals.side1.len()
                + self.side_signals.side2.len()
                + self.matching_signal_count()
        }
    }

    fn matching_signal_count(&self) -> usize {
        self.matching_signals
            .iter()
            .filter(|segments| {
                !self.side_signals.side1.contains(*segments)
                    && !self.side_signals.side2.contains(*segments)
            })
            .count()
    }

    pub(crate) fn write_fst(
        &self,
        output: &FstDiffOutput,
        start: u64,
        end: Option<u64>,
    ) -> io::Result<()> {
        let paths1: Vec<&Path> = output
            .sides
            .side1
            .paths
            .iter()
            .map(PathBuf::as_path)
            .collect();
        let paths2: Vec<&Path> = output
            .sides
            .side2
            .paths
            .iter()
            .map(PathBuf::as_path)
            .collect();
        let (readers1, hier1, offsets1) =
            crate::open_wave_files(&paths1, &output.name_options, None)
                .map_err(io::Error::other)?;
        let (readers2, hier2, offsets2) =
            crate::open_wave_files(&paths2, &output.name_options, None)
                .map_err(io::Error::other)?;

        let mut synth = FstDiffSynthesizer::new(output, self, &hier1, &hier2);
        synth.collect_side(readers1, offsets1, true, start, end)?;
        synth.collect_side(readers2, offsets2, false, start, end)?;
        synth.write_fst(&output.path)
    }
}

#[derive(Default)]
struct FstDiffSignal {
    single_meta: Option<VarMeta>,
    single_timeline: FstDiffSingleTimeline,
    side_meta: SidePair<Option<VarMeta>>,
    timeline: FstDiffTimeline,
}

type FstDiffSignals = BTreeMap<Vec<String>, FstDiffSignal>;

#[derive(Clone, Copy)]
enum Track {
    Single,
    Side1,
    Side2,
}

/// Per-side state accumulated while collecting changes for one input.
#[derive(Default)]
struct SideCollector {
    label: String,
    handles: HashSet<usize>,
    updates: HashMap<usize, Vec<(Vec<String>, Track)>>,
}

struct FstDiffSynthesizer {
    signals: FstDiffSignals,
    sides: SidePair<SideCollector>,
}

impl FstDiffSynthesizer {
    fn new(
        output: &FstDiffOutput,
        recorder: &FstDiffRecorder,
        hier1: &WaveHierarchy,
        hier2: &WaveHierarchy,
    ) -> Self {
        let mut synth = Self {
            signals: BTreeMap::new(),
            sides: SidePair {
                side1: SideCollector {
                    label: sanitize_vcd_identifier(&output.sides.side1.label),
                    ..Default::default()
                },
                side2: SideCollector {
                    label: sanitize_vcd_identifier(&output.sides.side2.label),
                    ..Default::default()
                },
            },
        };

        for segments in &recorder.side_signals.side1 {
            synth.add_side_signal(hier1, segments, true);
        }
        for segments in &recorder.side_signals.side2 {
            synth.add_side_signal(hier2, segments, false);
        }
        if recorder.has_diffs {
            // Copy shared (matching) signals from the side that held the
            // X-like masked bits, so --ignore-xz shared values keep the Xs.
            let prefer_side1 = recorder.prefer_side1_for_shared();
            let hier = if prefer_side1 { hier1 } else { hier2 };
            for segments in recorder.matching_signals.iter().filter(|segments| {
                !recorder.side_signals.side1.contains(*segments)
                    && !recorder.side_signals.side2.contains(*segments)
            }) {
                synth.add_single_signal(hier, segments, prefer_side1);
            }
        }

        synth
    }

    fn add_side_signal(&mut self, hier: &WaveHierarchy, segments: &[String], is_side1: bool) {
        let Some((handle, var)) = handle_var_for_segments(hier, segments) else {
            return;
        };
        let signal = self.signals.entry(segments.to_vec()).or_default();
        *signal.side_meta.get_mut(is_side1) = Some(var.meta.clone());
        let track = if is_side1 { Track::Side1 } else { Track::Side2 };
        let side = self.sides.get_mut(is_side1);
        side.handles.insert(handle);
        side.updates
            .entry(handle)
            .or_default()
            .push((segments.to_vec(), track));
    }

    fn add_single_signal(&mut self, hier: &WaveHierarchy, segments: &[String], is_side1: bool) {
        let Some((handle, var)) = handle_var_for_segments(hier, segments) else {
            return;
        };
        let signal = self.signals.entry(segments.to_vec()).or_default();
        signal.single_meta = Some(var.meta.clone());
        let side = self.sides.get_mut(is_side1);
        side.handles.insert(handle);
        side.updates
            .entry(handle)
            .or_default()
            .push((segments.to_vec(), Track::Single));
    }

    fn collect_side(
        &mut self,
        readers: Vec<WaveReader>,
        offsets: Vec<usize>,
        is_side1: bool,
        start: u64,
        end: Option<u64>,
    ) -> io::Result<()> {
        let side = self.sides.get(is_side1);
        if side.handles.is_empty() {
            return Ok(());
        }
        let include_sets = include_sets_for_handles(&side.handles, &offsets);
        let updates = &side.updates;

        let (tx, rx) = channel::bounded(64);
        let thread = std::thread::spawn(move || {
            send_merged_wave_changes(readers, &offsets, Some(include_sets), start, end, tx)
        });

        for batch in &rx {
            for change in batch.changes {
                let Some(handle_updates) = updates.get(&change.handle) else {
                    continue;
                };
                for (segments, track) in handle_updates {
                    let signal = self.signals.entry(segments.clone()).or_default();
                    match track {
                        Track::Single => {
                            signal
                                .single_timeline
                                .insert(batch.time, change.value.clone());
                        }
                        Track::Side1 => {
                            signal.timeline.entry(batch.time).or_default().side1 =
                                Some(change.value.clone());
                        }
                        Track::Side2 => {
                            signal.timeline.entry(batch.time).or_default().side2 =
                                Some(change.value.clone());
                        }
                    }
                }
            }
        }

        match thread.join() {
            Ok(result) => result,
            Err(_) => Err(io::Error::other("wavediff fst-diff reader thread panicked")),
        }
    }

    fn write_fst(&self, path: &Path) -> io::Result<()> {
        let tmp_vcd = std::env::temp_dir().join(format!(
            "wavediff-{}-{}.vcd",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));

        self.write_vcd(&tmp_vcd)?;
        let converter = std::env::var_os("VCD2FST").unwrap_or_else(|| "vcd2fst".into());
        let status = std::process::Command::new(converter)
            .arg(&tmp_vcd)
            .arg(path)
            .status()
            .map_err(|e| io::Error::other(format!("failed to run vcd2fst: {}", e)))?;
        let _ = std::fs::remove_file(&tmp_vcd);
        if !status.success() {
            return Err(io::Error::other(format!("vcd2fst exited with {}", status)));
        }
        Ok(())
    }

    fn write_vcd(&self, path: &Path) -> io::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut writer = vcd::Writer::new(BufWriter::new(file));
        writer.version("wavediff")?;
        writer.timescale(1, vcd::TimescaleUnit::NS)?;

        let mut ids = BTreeMap::new();
        let mut current_scope: Vec<String> = Vec::new();
        for (segments, signal) in &self.signals {
            let Some((leaf, scope)) = segments.split_last() else {
                continue;
            };
            let common = common_prefix_len(&current_scope, scope);
            while current_scope.len() > common {
                writer.upscope()?;
                current_scope.pop();
            }
            for segment in &scope[common..] {
                writer.add_module(&sanitize_vcd_identifier(segment))?;
                current_scope.push(segment.clone());
            }

            let single_id = if let Some(meta) = &signal.single_meta {
                let (reference, index) = vcd_reference_for_leaf(leaf);
                Some(writer.add_var(
                    vcd_var_type_from_meta(meta),
                    meta.size.max(1),
                    &reference,
                    index,
                )?)
            } else {
                None
            };
            let mut side_ids: SidePair<Option<vcd::IdCode>> = SidePair::default();
            for is_side1 in [true, false] {
                if let Some(meta) = signal.side_meta.get(is_side1) {
                    let label = &self.sides.get(is_side1).label;
                    let (reference, index) = vcd_side_reference_for_leaf(leaf, label);
                    *side_ids.get_mut(is_side1) = Some(writer.add_var(
                        vcd_var_type_from_meta(meta),
                        meta.size.max(1),
                        &reference,
                        index,
                    )?);
                }
            }
            ids.insert(segments.clone(), (single_id, side_ids));
        }
        while !current_scope.is_empty() {
            writer.upscope()?;
            current_scope.pop();
        }
        writer.enddefinitions()?;

        let mut times = BTreeSet::new();
        for signal in self.signals.values() {
            times.extend(signal.single_timeline.keys().copied());
            times.extend(signal.timeline.keys().copied());
        }

        let mut last_written: HashMap<vcd::IdCode, OwnedSignalValue> = HashMap::new();
        for time in times {
            writer.timestamp(time)?;
            for (segments, signal) in &self.signals {
                let (single_id, side_ids) = &ids[segments];
                if let (Some(id), Some(meta), Some(value)) = (
                    single_id,
                    signal.single_meta.as_ref(),
                    signal.single_timeline.get(&time),
                ) {
                    write_raw_change_if_changed(&mut writer, &mut last_written, *id, meta, value)?;
                }
                if let Some(sample) = signal.timeline.get(&time) {
                    for is_side1 in [true, false] {
                        if let (Some(id), Some(meta), Some(value)) = (
                            side_ids.get(is_side1),
                            signal.side_meta.get(is_side1).as_ref(),
                            sample.get(is_side1).as_ref(),
                        ) {
                            write_raw_change_if_changed(
                                &mut writer,
                                &mut last_written,
                                *id,
                                meta,
                                value,
                            )?;
                        }
                    }
                }
            }
        }

        writer.flush()
    }
}

fn include_sets_for_handles(handles: &HashSet<usize>, offsets: &[usize]) -> Vec<HashSet<usize>> {
    let mut include_sets: Vec<HashSet<usize>> =
        (0..offsets.len()).map(|_| HashSet::new()).collect();
    for &handle in handles {
        let reader_idx = match offsets.binary_search(&handle) {
            Ok(idx) => idx,
            Err(0) => continue,
            Err(idx) => idx - 1,
        };
        include_sets[reader_idx].insert(handle - offsets[reader_idx]);
    }
    include_sets
}

fn handle_var_for_segments<'a>(
    hier: &'a WaveHierarchy,
    segments: &[String],
) -> Option<(usize, &'a VarEntry)> {
    let refs: Vec<&str> = segments.iter().map(String::as_str).collect();
    let name = hier.names.find(&refs)?;
    hier.signal_map.iter().find_map(|(&handle, info)| {
        info.vars
            .iter()
            .find(|var| var.name == name)
            .map(|var| (handle, var))
    })
}

fn common_prefix_len(left: &[String], right: &[String]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

fn vcd_var_type_from_meta(meta: &VarMeta) -> vcd::VarType {
    vcd::VarType::from_str_ext(meta.var_type, true).unwrap_or(vcd::VarType::Wire)
}

fn vcd_reference_for_leaf(leaf: &str) -> (String, Option<vcd::ReferenceIndex>) {
    let (base, index) = split_numeric_range_suffix(leaf);
    (sanitize_vcd_identifier(base), index)
}

fn vcd_side_reference_for_leaf(
    leaf: &str,
    side_label: &str,
) -> (String, Option<vcd::ReferenceIndex>) {
    let (base, index) = split_numeric_range_suffix(leaf);
    (
        format!("{}__{}", sanitize_vcd_identifier(base), side_label),
        index,
    )
}

fn split_numeric_range_suffix(s: &str) -> (&str, Option<vcd::ReferenceIndex>) {
    if !s.ends_with(']') {
        return (s, None);
    }
    let Some(pos) = s
        .rfind(" [")
        .or_else(|| s.rfind('[').filter(|&pos| pos > 0))
    else {
        return (s, None);
    };
    let bracket_start = s[pos..].find('[').unwrap() + pos + 1;
    let content = &s[bracket_start..s.len() - 1];
    if content.is_empty() || !content.chars().all(|c| c.is_ascii_digit() || c == ':') {
        return (s, None);
    }
    let index = if let Some((hi, lo)) = content.split_once(':') {
        let Ok(hi) = hi.parse() else {
            return (s, None);
        };
        let Ok(lo) = lo.parse() else {
            return (s, None);
        };
        vcd::ReferenceIndex::Range(hi, lo)
    } else {
        let Ok(bit) = content.parse() else {
            return (s, None);
        };
        vcd::ReferenceIndex::BitSelect(bit)
    };
    (&s[..pos], Some(index))
}

fn write_raw_change_if_changed<W: Write>(
    writer: &mut vcd::Writer<W>,
    last_written: &mut HashMap<vcd::IdCode, OwnedSignalValue>,
    id: vcd::IdCode,
    meta: &VarMeta,
    value: &OwnedSignalValue,
) -> io::Result<()> {
    if last_written.get(&id).is_some_and(|last| last == value) {
        return Ok(());
    }
    write_raw_change(writer, id, meta, value)?;
    last_written.insert(id, value.clone());
    Ok(())
}

fn write_raw_change<W: Write>(
    writer: &mut vcd::Writer<W>,
    id: vcd::IdCode,
    meta: &VarMeta,
    value: &OwnedSignalValue,
) -> io::Result<()> {
    if is_real_var_type(meta.var_type) {
        let real = match value {
            OwnedSignalValue::Real(real) => *real,
            OwnedSignalValue::String(bytes) => std::str::from_utf8(bytes)
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(f64::NAN),
        };
        return writer.change_real(id, real);
    }

    let OwnedSignalValue::String(bytes) = value else {
        return writer.change_real(
            id,
            match value {
                OwnedSignalValue::Real(real) => *real,
                OwnedSignalValue::String(_) => unreachable!(),
            },
        );
    };
    if meta.var_type == "string" {
        let text = std::str::from_utf8(bytes).unwrap_or("x");
        return writer.change_string(id, text);
    }
    if let Some(values) = digital_value_bits(bytes, meta.size) {
        if meta.size <= 1 {
            return writer.change_scalar(id, values[0]);
        }
        return writer.change_vector(id, values);
    }

    let text = std::str::from_utf8(bytes).unwrap_or("x");
    writer.change_string(id, text)
}

fn digital_value_bits(bytes: &[u8], width: u32) -> Option<Vec<vcd::Value>> {
    let width = width.max(1) as usize;
    if !bytes.is_empty() && bytes.len() <= width {
        if let Some(mut values) = bytes
            .iter()
            .map(|byte| digital_char_value(*byte))
            .collect::<Option<Vec<_>>>()
        {
            if values.len() < width {
                let extension = match values[0] {
                    vcd::Value::X => vcd::Value::X,
                    vcd::Value::Z => vcd::Value::Z,
                    _ => vcd::Value::V0,
                };
                let mut padded = vec![extension; width - values.len()];
                padded.append(&mut values);
                return Some(padded);
            }
            return Some(values);
        }
    }

    let packed_len = width.div_ceil(8);
    if bytes.len() == packed_len {
        let mut values = Vec::with_capacity(width);
        for bit_index in 0..width {
            let byte = bytes[bit_index / 8];
            let bit = (byte >> (7 - (bit_index & 7))) & 1;
            values.push(if bit == 0 {
                vcd::Value::V0
            } else {
                vcd::Value::V1
            });
        }
        return Some(values);
    }

    None
}

fn digital_char_value(byte: u8) -> Option<vcd::Value> {
    match byte {
        b'0' => Some(vcd::Value::V0),
        b'1' => Some(vcd::Value::V1),
        b'x' | b'X' | b'u' | b'U' | b'w' | b'W' | b'-' | b'?' => Some(vcd::Value::X),
        b'z' | b'Z' => Some(vcd::Value::Z),
        b'h' | b'H' => Some(vcd::Value::V1),
        b'l' | b'L' => Some(vcd::Value::V0),
        _ => None,
    }
}

fn sanitize_vcd_identifier(s: &str) -> String {
    let mut out = String::with_capacity(s.len().max(1));
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    if out
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || c == '$')
    {
        out.insert(0, '_');
    }
    out
}

fn is_real_var_type(var_type: &str) -> bool {
    matches!(
        var_type,
        "real" | "realtime" | "shortreal" | "real_parameter"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digital_value_bits_accepts_packed_bytes() {
        let values = digital_value_bits(&[0b1010_0000], 4).unwrap();

        assert_eq!(
            values,
            vec![
                vcd::Value::V1,
                vcd::Value::V0,
                vcd::Value::V1,
                vcd::Value::V0
            ]
        );
    }

    #[test]
    fn digital_value_bits_accepts_logic_chars() {
        let values = digital_value_bits(b"10xzHLUW-?", 10).unwrap();

        assert_eq!(
            values,
            vec![
                vcd::Value::V1,
                vcd::Value::V0,
                vcd::Value::X,
                vcd::Value::Z,
                vcd::Value::V1,
                vcd::Value::V0,
                vcd::Value::X,
                vcd::Value::X,
                vcd::Value::X,
                vcd::Value::X,
            ]
        );
    }

    #[test]
    fn digital_value_bits_pads_short_vectors() {
        let values = digital_value_bits(b"1010", 8).unwrap();

        assert_eq!(
            values,
            vec![
                vcd::Value::V0,
                vcd::Value::V0,
                vcd::Value::V0,
                vcd::Value::V0,
                vcd::Value::V1,
                vcd::Value::V0,
                vcd::Value::V1,
                vcd::Value::V0,
            ]
        );
    }
}
