//------------------------------------------------------------------------------
// diff.rs
// Waveform diffing: channel-based signal comparison with cross-format support
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::{BufRead, Seek, Write};
use std::path::Path;
use std::sync::mpsc;

use fst_reader::{FstFilter, FstSignalValue};

use crate::{names_only, next_vcd_change, NameOptions, SignalMap, SignalNames, WaveReader};

/// Owned version of `FstSignalValue` for sending through channels
#[derive(Debug, Clone)]
enum OwnedSignalValue {
    String(Vec<u8>),
    Real(f64),
}

impl PartialEq for OwnedSignalValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (OwnedSignalValue::String(a), OwnedSignalValue::String(b)) => a == b,
            (OwnedSignalValue::Real(a), OwnedSignalValue::Real(b)) => a == b,
            // Cross-variant: FST files produce Real values while VCD files
            // store the same data as strings.  Fall back to string comparison
            // so that an FST real matches its VCD string equivalent.
            _ => self.to_string() == other.to_string(),
        }
    }
}

impl fmt::Display for OwnedSignalValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OwnedSignalValue::String(s) => {
                write!(f, "{}", std::str::from_utf8(s).unwrap_or("<invalid utf8>"))
            }
            OwnedSignalValue::Real(r) => write!(f, "{r}"),
        }
    }
}

impl OwnedSignalValue {
    fn from_fst_value(value: FstSignalValue) -> Self {
        match value {
            FstSignalValue::String(s) => OwnedSignalValue::String(s.to_vec()),
            FstSignalValue::Real(r) => OwnedSignalValue::Real(r),
        }
    }

    /// Compare two values, using epsilon for real-valued signals when provided.
    fn approx_eq(&self, other: &Self, real_epsilon: Option<f64>) -> bool {
        match (self, other, real_epsilon) {
            (OwnedSignalValue::Real(a), OwnedSignalValue::Real(b), Some(eps)) => {
                (a - b).abs() <= eps
            }
            (_, _, Some(eps)) => {
                // VCD stores reals as strings; try numeric comparison.
                // Short-circuit on exact match to avoid float parsing of
                // non-real values (e.g. large bit-vectors that happen to
                // parse as floats with precision loss).
                if self == other {
                    return true;
                }
                if let (Ok(a), Ok(b)) = (
                    self.to_string().parse::<f64>(),
                    other.to_string().parse::<f64>(),
                ) {
                    (a - b).abs() <= eps
                } else {
                    false
                }
            }
            _ => self == other,
        }
    }
}

/// Compare signal names between two waveform files and return the differences
pub fn compare_signal_names(
    map1: &SignalMap,
    map2: &SignalMap,
) -> (HashSet<String>, HashSet<String>) {
    let set1: HashSet<String> = map1
        .values()
        .flat_map(|info| info.vars.iter().map(|v| v.name.clone()))
        .collect();
    let set2: HashSet<String> = map2
        .values()
        .flat_map(|info| info.vars.iter().map(|v| v.name.clone()))
        .collect();

    let only_in_1: HashSet<String> = set1.difference(&set2).cloned().collect();
    let only_in_2: HashSet<String> = set2.difference(&set1).cloned().collect();

    (only_in_1, only_in_2)
}

/// Build mapping from signal names to handle indices
fn build_name_to_handles(handle_to_names: &SignalNames) -> HashMap<String, Vec<usize>> {
    let mut name_to_handles: HashMap<String, Vec<usize>> = HashMap::new();

    for (&handle, names) in handle_to_names.iter() {
        for name in names {
            name_to_handles
                .entry(name.clone())
                .or_default()
                .push(handle);
        }
    }

    name_to_handles
}

/// Build mapping from handles in file A to handles in file B based on signal names
fn build_handle_mapping(
    names_a: &SignalNames,
    names_b: &SignalNames,
) -> HashMap<usize, Vec<usize>> {
    let name_to_handles_b = build_name_to_handles(names_b);
    let mut handle_mapping: HashMap<usize, Vec<usize>> = HashMap::new();

    for (&handle_a, names) in names_a.iter() {
        let mut handles_b = Vec::new();
        for name in names {
            if let Some(b_handles) = name_to_handles_b.get(name) {
                handles_b.extend(b_handles);
            }
        }
        if !handles_b.is_empty() {
            handle_mapping.insert(handle_a, handles_b);
        }
    }

    handle_mapping
}

/// Represents an individual signal change
#[derive(Debug)]
struct SignalChange {
    time: u64,
    handle: usize,
    value: OwnedSignalValue,
}

/// Read signals from an FST reader and send individual changes to a channel
fn read_and_send_signals<R: BufRead + Seek>(
    mut fst_reader: fst_reader::FstReader<R>,
    filter: fst_reader::FstFilter,
    handle_offset: usize,
    tx: mpsc::Sender<SignalChange>,
) {
    let _ = fst_reader.read_signals(&filter, |time, handle, value| {
        if time < filter.start {
            return;
        }
        if let Some(e) = filter.end {
            if time > e {
                return;
            }
        }
        let _ = tx.send(SignalChange {
            time,
            handle: handle.get_index() + handle_offset,
            value: OwnedSignalValue::from_fst_value(value),
        });
    });
}

/// Consumes changes from `rx1` (file1) and `rx2` (file2), buffering as needed to
/// match signals across potentially different orderings. Returns `true` if differences
/// were found.
fn compare_signal_channels<W: Write>(
    writer: &mut W,
    rx1: mpsc::Receiver<SignalChange>,
    rx2: mpsc::Receiver<SignalChange>,
    handle_mapping: &HashMap<usize, Vec<usize>>,
    handle_to_names1: &SignalNames,
    handle_to_names2: &SignalNames,
    real_epsilon: Option<f64>,
) -> std::io::Result<bool> {
    let mut has_differences = false;
    // File2 changes we've read from the channel but haven't matched yet,
    // keyed by (time, handle) so we can look them up when file1 catches up.
    let mut buffered2: HashMap<(u64, usize), OwnedSignalValue> = HashMap::new();
    // File2 handles that were successfully matched at the current time step,
    // so we can evict their buffer entries when we advance to the next time.
    let mut matched_at_current_time: HashSet<usize> = HashSet::new();
    let mut current_time: Option<u64> = None;
    let mut source2_ended = false;

    // Drive the comparison from file1's change stream.  For each file1
    // change we look up the corresponding file2 value—either already
    // buffered or by reading ahead on file2's channel.
    for change1 in rx1 {
        // When we move to a new time step, evict buffer entries for file2
        // handles that were already matched—they won't be needed again.
        if let Some(prev_time) = current_time {
            if prev_time != change1.time {
                for &handle in &matched_at_current_time {
                    buffered2.remove(&(prev_time, handle));
                }
                matched_at_current_time.clear();
            }
        }
        current_time = Some(change1.time);

        if let Some(handles2) = handle_mapping.get(&change1.handle) {
            for &handle2 in handles2 {
                let key = (change1.time, handle2);
                let mut found = buffered2.get(&key).cloned();
                let mut saw_time_in_source2 = false;

                // Read ahead on file2's channel until we find the target
                // (time, handle) or pass it.  Everything read along the way
                // is buffered for future lookups.
                if found.is_none() && !source2_ended {
                    loop {
                        match rx2.recv() {
                            Ok(change2) => {
                                if change2.time == change1.time {
                                    saw_time_in_source2 = true;
                                }
                                let is_match =
                                    change2.time == change1.time && change2.handle == handle2;
                                if is_match {
                                    found = Some(change2.value.clone());
                                }
                                buffered2.insert((change2.time, change2.handle), change2.value);
                                if is_match || change2.time > change1.time {
                                    break;
                                }
                            }
                            Err(_) => {
                                source2_ended = true;
                                break;
                            }
                        }
                    }
                }

                if let Some(value2) = found {
                    matched_at_current_time.insert(handle2);
                    // Both files have this signal at this time—compare values.
                    if !change1.value.approx_eq(&value2, real_epsilon) {
                        has_differences = true;
                        if let Some(names) = handle_to_names1.get(&change1.handle) {
                            for name in names {
                                writeln!(
                                    writer,
                                    "{} {} {} != {}",
                                    change1.time, name, change1.value, value2
                                )?;
                            }
                        }
                    }
                } else {
                    // File2 doesn't have this signal at this time.
                    // Distinguish "time exists but signal absent" from
                    // "entire time step missing" for a clearer message.
                    has_differences = true;
                    if let Some(names) = handle_to_names1.get(&change1.handle) {
                        for name in names {
                            let msg = if saw_time_in_source2 {
                                "(not in file2)"
                            } else {
                                "(missing time in file2)"
                            };
                            writeln!(
                                writer,
                                "{} {} {} {}",
                                change1.time, name, change1.value, msg
                            )?;
                        }
                    }
                }
            }
        }
    }

    // Evict the last time step's matched entries before reporting leftovers.
    if let Some(last_time) = current_time {
        for &handle in &matched_at_current_time {
            buffered2.remove(&(last_time, handle));
        }
    }

    // Anything still in the buffer was in file2 but never matched by file1.
    for ((time, handle), value) in buffered2 {
        has_differences = true;
        if let Some(names) = handle_to_names2.get(&handle) {
            for name in names {
                writeln!(writer, "{} {} {} (only in file2)", time, name, value)?;
            }
        }
    }

    // Drain any remaining file2 changes we never read from the channel.
    if !source2_ended {
        for change2 in rx2 {
            has_differences = true;
            if let Some(names) = handle_to_names2.get(&change2.handle) {
                for name in names {
                    writeln!(
                        writer,
                        "{} {} {} (only in file2)",
                        change2.time, name, change2.value
                    )?;
                }
            }
        }
    }

    Ok(has_differences)
}

/// Send all signal changes from a WaveReader through a channel, applying time filtering.
/// `handle_offset` is added to each handle index (used for merged sets).
fn send_wave_changes(
    reader: WaveReader,
    handle_offset: usize,
    start: u64,
    end: Option<u64>,
    tx: mpsc::Sender<SignalChange>,
) {
    match reader {
        WaveReader::Fst(fst_reader) => {
            let filter = FstFilter {
                start,
                end,
                include: None,
            };
            read_and_send_signals(*fst_reader, filter, handle_offset, tx);
        }
        WaveReader::Vcd(mut vcd_data) => {
            while let Some((time, handle, value_str)) = next_vcd_change(&mut vcd_data) {
                if time < start {
                    continue;
                }
                if let Some(e) = end {
                    if time > e {
                        break;
                    }
                }
                let _ = tx.send(SignalChange {
                    time,
                    handle: handle + handle_offset,
                    value: OwnedSignalValue::String(value_str.into_bytes()),
                });
            }
        }
    }
}

/// Send changes from multiple WaveReaders through a single channel, merging in time order.
fn send_merged_wave_changes(
    readers: Vec<WaveReader>,
    offsets: &[usize],
    start: u64,
    end: Option<u64>,
    tx: mpsc::Sender<SignalChange>,
) {
    if readers.len() == 1 {
        send_wave_changes(
            readers.into_iter().next().unwrap(),
            offsets[0],
            start,
            end,
            tx,
        );
        return;
    }

    // Spawn a thread per reader, each sending to its own channel
    let mut inner_rxs = Vec::with_capacity(readers.len());
    let mut threads = Vec::new();

    for (reader, &offset) in readers.into_iter().zip(offsets.iter()) {
        let (inner_tx, inner_rx) = mpsc::channel();
        threads.push(std::thread::spawn(move || {
            send_wave_changes(reader, offset, start, end, inner_tx);
        }));
        inner_rxs.push(inner_rx);
    }

    let _ = crate::kway_merge_channels(&inner_rxs, |c| c.time, |change| {
        let _ = tx.send(change);
        Ok(())
    });

    for t in threads {
        t.join().unwrap();
    }
}

/// Compare metadata and attributes for signals that share the same name across two files.
/// Returns a list of human-readable difference strings.
/// Direction is only compared if both sides have an explicit (non-implicit) direction.
pub fn compare_signal_meta(
    map1: &SignalMap,
    map2: &SignalMap,
) -> Vec<String> {
    let mut diffs = Vec::new();

    // Build name → VarEntry lookup for both maps
    let entries1: HashMap<&str, &crate::VarEntry> = map1
        .values()
        .flat_map(|info| info.vars.iter().map(|v| (v.name.as_str(), v)))
        .collect();
    let entries2: HashMap<&str, &crate::VarEntry> = map2
        .values()
        .flat_map(|info| info.vars.iter().map(|v| (v.name.as_str(), v)))
        .collect();

    let mut common: Vec<&&str> = entries1.keys().filter(|n| entries2.contains_key(**n)).collect();
    common.sort();

    for name in common {
        let v1 = entries1[*name];
        let v2 = entries2[*name];
        if v1.meta.var_type != v2.meta.var_type {
            diffs.push(format!("{}: type {} != {}", name, v1.meta.var_type, v2.meta.var_type));
        }
        if v1.meta.size != v2.meta.size {
            diffs.push(format!("{}: size {} != {}", name, v1.meta.size, v2.meta.size));
        }
        if v1.meta.direction != crate::IMPLICIT_DIRECTION
            && v2.meta.direction != crate::IMPLICIT_DIRECTION
            && v1.meta.direction != v2.meta.direction
        {
            diffs.push(format!(
                "{}: direction {} != {}",
                name, v1.meta.direction, v2.meta.direction
            ));
        }
        if v1.attrs != v2.attrs {
            let a1 = if v1.attrs.is_empty() { "(none)".to_string() } else { v1.attrs.join("; ") };
            let a2 = if v2.attrs.is_empty() { "(none)".to_string() } else { v2.attrs.join("; ") };
            diffs.push(format!("{}: attrs [{}] != [{}]", name, a1, a2));
        }
    }

    diffs
}

/// Open two waveform files (any mix of FST/VCD) and return both readers and hierarchies
pub fn open_and_read_waves<P1: AsRef<Path>, P2: AsRef<Path>>(
    path1: P1,
    path2: P2,
    options: &NameOptions,
) -> Result<(WaveReader, SignalMap, WaveReader, SignalMap), String> {
    let (r1, m1) = crate::open_wave_file(path1.as_ref(), options)?;
    let (r2, m2) = crate::open_wave_file(path2.as_ref(), options)?;
    Ok((r1, m1, r2, m2))
}

/// Compare two waveform files (any mix of FST/VCD) and write differences
///
/// Returns `true` if differences were found.
#[allow(clippy::too_many_arguments)]
pub fn diff_waves<W: Write>(
    writer: &mut W,
    reader1: WaveReader,
    signal_map1: &SignalMap,
    reader2: WaveReader,
    signal_map2: &SignalMap,
    start: u64,
    end: Option<u64>,
    real_epsilon: Option<f64>,
) -> std::io::Result<bool> {
    diff_wave_sets(
        writer,
        vec![reader1],
        signal_map1,
        &[0],
        vec![reader2],
        signal_map2,
        &[0],
        start,
        end,
        real_epsilon,
    )
}

/// Compare two sets of waveform files and write differences.
///
/// Each set is a vec of readers with corresponding handle offsets.
/// Returns `true` if differences were found.
#[allow(clippy::too_many_arguments)]
pub fn diff_wave_sets<W: Write>(
    writer: &mut W,
    readers1: Vec<WaveReader>,
    signal_map1: &SignalMap,
    offsets1: &[usize],
    readers2: Vec<WaveReader>,
    signal_map2: &SignalMap,
    offsets2: &[usize],
    start: u64,
    end: Option<u64>,
    real_epsilon: Option<f64>,
) -> std::io::Result<bool> {
    let handle_to_names1 = names_only(signal_map1);
    let handle_to_names2 = names_only(signal_map2);
    let handle_mapping = build_handle_mapping(&handle_to_names1, &handle_to_names2);

    let (tx1, rx1) = mpsc::channel();
    let (tx2, rx2) = mpsc::channel();

    let offsets1 = offsets1.to_vec();
    let offsets2 = offsets2.to_vec();

    let thread1 = std::thread::spawn(move || {
        send_merged_wave_changes(readers1, &offsets1, start, end, tx1);
    });
    let thread2 = std::thread::spawn(move || {
        send_merged_wave_changes(readers2, &offsets2, start, end, tx2);
    });

    let result = compare_signal_channels(
        writer,
        rx1,
        rx2,
        &handle_mapping,
        &handle_to_names1,
        &handle_to_names2,
        real_epsilon,
    );

    thread1.join().unwrap();
    thread2.join().unwrap();

    result
}

/// Open two sets of waveform files and return readers and merged hierarchies
#[allow(clippy::type_complexity)]
pub fn open_and_read_wave_sets(
    paths1: &[&Path],
    paths2: &[&Path],
    options: &NameOptions,
) -> Result<
    (
        Vec<WaveReader>,
        SignalMap,
        Vec<usize>,
        Vec<WaveReader>,
        SignalMap,
        Vec<usize>,
    ),
    String,
> {
    let (r1, m1, o1) = crate::open_wave_files(paths1, options, None)?;
    let (r2, m2, o2) = crate::open_wave_files(paths2, options, None)?;
    Ok((r1, m1, o1, r2, m2, o2))
}
