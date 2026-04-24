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

use crossbeam_channel as channel;
use fst_reader::{FstFilter, FstSignalValue};

use crate::{next_vcd_change, NameOptions, NameTree, SignalMap, WaveHierarchy, WaveReader};

/// Max queued batches per channel. Each batch holds up to BATCH_SIZE changes.
/// With 64 batches of 4096 changes at ~23 bytes/value, each channel holds
/// ~30 MB of backlog at most.
const CHANNEL_BOUND: usize = 64;

/// Number of signal changes collected before sending a batch through the channel.
const BATCH_SIZE: usize = 4096;

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
    hier1: &WaveHierarchy,
    hier2: &WaveHierarchy,
) -> (HashSet<String>, HashSet<String>) {
    let set1: HashSet<String> = hier1
        .signal_map
        .values()
        .flat_map(|info| info.vars.iter().map(|v| hier1.names.format_path(v.name)))
        .collect();
    let set2: HashSet<String> = hier2
        .signal_map
        .values()
        .flat_map(|info| info.vars.iter().map(|v| hier2.names.format_path(v.name)))
        .collect();

    let only_in_1: HashSet<String> = set1.difference(&set2).cloned().collect();
    let only_in_2: HashSet<String> = set2.difference(&set1).cloned().collect();

    (only_in_1, only_in_2)
}

/// Build a reverse index from NameId to the handles that use it.
fn build_name_id_to_handles(map: &SignalMap) -> HashMap<crate::NameId, Vec<usize>> {
    let mut result: HashMap<crate::NameId, Vec<usize>> = HashMap::new();
    for (&handle, info) in map {
        for var in &info.vars {
            result.entry(var.name).or_default().push(handle);
        }
    }
    result
}

/// Build mapping from handles in file A to handles in file B using tree-based lookup.
///
/// For each signal in A, walks A's tree to get segments, then looks up those
/// segments in B's tree to find the matching NameId. This avoids materializing
/// flat name strings as HashMap keys.
fn build_handle_mapping(
    map_a: &SignalMap,
    tree_a: &NameTree,
    map_b: &SignalMap,
    tree_b: &NameTree,
) -> HashMap<usize, Vec<usize>> {
    let name_id_to_handles_b = build_name_id_to_handles(map_b);
    let mut handle_mapping: HashMap<usize, Vec<usize>> = HashMap::new();

    for (&handle_a, info) in map_a {
        let mut handles_b = Vec::new();
        for var in &info.vars {
            let segments = tree_a.segments(var.name);
            if let Some(b_name_id) = tree_b.find(&segments) {
                if let Some(b_handles) = name_id_to_handles_b.get(&b_name_id) {
                    handles_b.extend(b_handles);
                }
            }
        }
        if !handles_b.is_empty() {
            handle_mapping.insert(handle_a, handles_b);
        }
    }

    handle_mapping
}

/// A single signal value change (handle + value; time is on the enclosing TimeBatch).
#[derive(Debug)]
struct SignalChange {
    handle: usize,
    value: OwnedSignalValue,
}

/// A batch of signal changes all at the same simulation time.
/// Batches are capped at BATCH_SIZE; a single time step may span multiple batches.
#[derive(Debug)]
struct TimeBatch {
    time: u64,
    changes: Vec<SignalChange>,
}

/// Flush the current batch through `tx`, replacing it with a fresh empty vec.
fn flush_batch(tx: &channel::Sender<TimeBatch>, time: u64, changes: &mut Vec<SignalChange>) {
    let full = std::mem::replace(changes, Vec::with_capacity(BATCH_SIZE));
    let _ = tx.send(TimeBatch { time, changes: full });
}

/// Read signals from an FST reader and send same-time batches to a channel.
fn read_and_send_signals<R: BufRead + Seek>(
    mut fst_reader: fst_reader::FstReader<R>,
    filter: fst_reader::FstFilter,
    handle_offset: usize,
    tx: channel::Sender<TimeBatch>,
) {
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    let mut batch_time: u64 = 0;
    let _ = fst_reader.read_signals(&filter, |time, handle, value| {
        if time < filter.start {
            return;
        }
        if let Some(e) = filter.end {
            if time > e {
                return;
            }
        }
        if !batch.is_empty() && (time != batch_time || batch.len() >= BATCH_SIZE) {
            flush_batch(&tx, batch_time, &mut batch);
        }
        batch_time = time;
        batch.push(SignalChange {
            handle: handle.get_index() + handle_offset,
            value: OwnedSignalValue::from_fst_value(value),
        });
    });
    if !batch.is_empty() {
        let _ = tx.send(TimeBatch { time: batch_time, changes: batch });
    }
}

/// Format signal names for a handle on-demand from SignalMap + NameTree.
/// Only called on diff output lines, so the cost is proportional to mismatches.
fn format_handle_names(handle: usize, map: &SignalMap, tree: &NameTree) -> Vec<String> {
    match map.get(&handle) {
        Some(info) => info.vars.iter().map(|v| tree.format_path(v.name)).collect(),
        None => Vec::new(),
    }
}

/// Consumes batches from `rx1` (file1) and `rx2` (file2), buffering as needed to
/// match signals across potentially different orderings. Returns `true` if differences
/// were found.
#[allow(clippy::too_many_arguments)]
fn compare_signal_channels<W: Write>(
    writer: &mut W,
    rx1: channel::Receiver<TimeBatch>,
    rx2: channel::Receiver<TimeBatch>,
    handle_mapping: &HashMap<usize, Vec<usize>>,
    map1: &SignalMap,
    tree1: &NameTree,
    map2: &SignalMap,
    tree2: &NameTree,
    real_epsilon: Option<f64>,
) -> std::io::Result<bool> {
    let mut has_differences = false;
    // File2 changes we've read from the channel but haven't matched yet,
    // keyed by (time, handle) so we can look them up when file1 catches up.
    let mut buffered2: HashMap<(u64, usize), OwnedSignalValue> = HashMap::new();
    // File2 handles that were successfully matched at the current time step,
    // so we can evict their buffer entries when we advance to the next time.
    let mut matched_at_current_time: HashSet<usize> = HashSet::new();
    let mut prev_time: Option<u64> = None;
    let mut source2_ended = false;

    // Drive the comparison from file1's batch stream.
    for batch1 in &rx1 {
        // When we move to a new time step, evict buffer entries for file2
        // handles that were already matched -- they won't be needed again.
        if let Some(pt) = prev_time {
            if pt != batch1.time {
                for &handle in &matched_at_current_time {
                    buffered2.remove(&(pt, handle));
                }
                matched_at_current_time.clear();
            }
        }
        prev_time = Some(batch1.time);
        let t1 = batch1.time;

        for change1 in batch1.changes {
            if let Some(handles2) = handle_mapping.get(&change1.handle) {
                for &handle2 in handles2 {
                    let mut found = buffered2.get(&(t1, handle2)).cloned();
                    let mut saw_time_in_source2 = false;

                    // Read ahead on file2's channel until we find the target
                    // (time, handle) or pass it.  Entire batches are drained
                    // into the buffer at once.
                    if found.is_none() && !source2_ended {
                        loop {
                            match rx2.recv() {
                                Ok(batch2) => {
                                    let t2 = batch2.time;
                                    if t2 == t1 {
                                        saw_time_in_source2 = true;
                                    }
                                    let mut found_in_batch = false;
                                    for c in batch2.changes {
                                        if t2 == t1 && c.handle == handle2 && found.is_none() {
                                            found = Some(c.value.clone());
                                            found_in_batch = true;
                                        }
                                        buffered2.insert((t2, c.handle), c.value);
                                    }
                                    if found_in_batch || t2 > t1 {
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
                        if !change1.value.approx_eq(&value2, real_epsilon) {
                            has_differences = true;
                            for name in format_handle_names(change1.handle, map1, tree1) {
                                writeln!(writer, "{} {} {} != {}", t1, name, change1.value, value2)?;
                            }
                        }
                    } else {
                        has_differences = true;
                        let msg = if saw_time_in_source2 {
                            "(not in file2)"
                        } else {
                            "(missing time in file2)"
                        };
                        for name in format_handle_names(change1.handle, map1, tree1) {
                            writeln!(writer, "{} {} {} {}", t1, name, change1.value, msg)?;
                        }
                    }
                }
            }
        }
    }

    // Evict the last time step's matched entries before reporting leftovers.
    if let Some(pt) = prev_time {
        for &handle in &matched_at_current_time {
            buffered2.remove(&(pt, handle));
        }
    }

    // Anything still in the buffer was in file2 but never matched by file1.
    for ((time, handle), value) in &buffered2 {
        has_differences = true;
        for name in format_handle_names(*handle, map2, tree2) {
            writeln!(writer, "{} {} {} (only in file2)", time, name, value)?;
        }
    }

    // Drain any remaining file2 batches we never read from the channel.
    if !source2_ended {
        for batch2 in &rx2 {
            has_differences = true;
            for change2 in &batch2.changes {
                for name in format_handle_names(change2.handle, map2, tree2) {
                    writeln!(
                        writer,
                        "{} {} {} (only in file2)",
                        batch2.time, name, change2.value
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
    tx: channel::Sender<TimeBatch>,
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
            let mut batch = Vec::with_capacity(BATCH_SIZE);
            let mut batch_time: u64 = 0;
            while let Some((time, handle, value_str)) = next_vcd_change(&mut vcd_data) {
                if time < start {
                    continue;
                }
                if let Some(e) = end {
                    if time > e {
                        break;
                    }
                }
                if !batch.is_empty() && (time != batch_time || batch.len() >= BATCH_SIZE) {
                    flush_batch(&tx, batch_time, &mut batch);
                }
                batch_time = time;
                batch.push(SignalChange {
                    handle: handle + handle_offset,
                    value: OwnedSignalValue::String(value_str.into_bytes()),
                });
            }
            if !batch.is_empty() {
                let _ = tx.send(TimeBatch { time: batch_time, changes: batch });
            }
        }
    }
}

/// Send changes from multiple WaveReaders through a single channel, merging in time order.
///
/// Each inner reader produces same-time `TimeBatch`es. The k-way merge picks the
/// batch with the smallest time and forwards it directly -- no re-batching needed.
fn send_merged_wave_changes(
    readers: Vec<WaveReader>,
    offsets: &[usize],
    start: u64,
    end: Option<u64>,
    tx: channel::Sender<TimeBatch>,
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

    // Spawn a thread per reader, each sending TimeBatches to its own channel.
    let mut inner_rxs = Vec::with_capacity(readers.len());
    let mut threads = Vec::new();

    for (reader, &offset) in readers.into_iter().zip(offsets.iter()) {
        let (inner_tx, inner_rx) = channel::bounded(CHANNEL_BOUND);
        threads.push(std::thread::spawn(move || {
            send_wave_changes(reader, offset, start, end, inner_tx);
        }));
        inner_rxs.push(inner_rx);
    }

    // K-way merge: maintain one head batch per reader, forward the smallest time.
    let mut heads: Vec<Option<TimeBatch>> = inner_rxs.iter().map(|rx| rx.recv().ok()).collect();
    loop {
        let min_idx = heads
            .iter()
            .enumerate()
            .filter_map(|(i, opt)| opt.as_ref().map(|b| (i, b.time)))
            .min_by_key(|&(_, t)| t)
            .map(|(i, _)| i);
        match min_idx {
            Some(idx) => {
                let _ = tx.send(heads[idx].take().unwrap());
                heads[idx] = inner_rxs[idx].recv().ok();
            }
            None => break,
        }
    }

    for t in threads {
        t.join().unwrap();
    }
}

/// Compare metadata and attributes for signals that share the same name across two files.
/// Returns a list of human-readable difference strings.
/// Direction is only compared if both sides have an explicit (non-implicit) direction.
pub fn compare_signal_meta(
    hier1: &WaveHierarchy,
    hier2: &WaveHierarchy,
) -> Vec<String> {
    let mut diffs = Vec::new();

    // Build name -> VarEntry lookup for both maps
    let entries1: HashMap<String, &crate::VarEntry> = hier1
        .signal_map
        .values()
        .flat_map(|info| info.vars.iter().map(|v| (hier1.names.format_path(v.name), v)))
        .collect();
    let entries2: HashMap<String, &crate::VarEntry> = hier2
        .signal_map
        .values()
        .flat_map(|info| info.vars.iter().map(|v| (hier2.names.format_path(v.name), v)))
        .collect();

    let mut common: Vec<&String> = entries1.keys().filter(|n| entries2.contains_key(*n)).collect();
    common.sort();

    for name in common {
        let v1 = entries1[name];
        let v2 = entries2[name];
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
) -> Result<(WaveReader, WaveHierarchy, WaveReader, WaveHierarchy), String> {
    let (r1, h1) = crate::open_wave_file(path1.as_ref(), options)?;
    let (r2, h2) = crate::open_wave_file(path2.as_ref(), options)?;
    Ok((r1, h1, r2, h2))
}

/// Compare two waveform files (any mix of FST/VCD) and write differences
///
/// Returns `true` if differences were found.
#[allow(clippy::too_many_arguments)]
pub fn diff_waves<W: Write>(
    writer: &mut W,
    reader1: WaveReader,
    hier1: &WaveHierarchy,
    reader2: WaveReader,
    hier2: &WaveHierarchy,
    start: u64,
    end: Option<u64>,
    real_epsilon: Option<f64>,
) -> std::io::Result<bool> {
    diff_wave_sets(
        writer,
        vec![reader1],
        hier1,
        &[0],
        vec![reader2],
        hier2,
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
    hier1: &WaveHierarchy,
    offsets1: &[usize],
    readers2: Vec<WaveReader>,
    hier2: &WaveHierarchy,
    offsets2: &[usize],
    start: u64,
    end: Option<u64>,
    real_epsilon: Option<f64>,
) -> std::io::Result<bool> {
    let handle_mapping = build_handle_mapping(
        &hier1.signal_map,
        &hier1.names,
        &hier2.signal_map,
        &hier2.names,
    );

    let (tx1, rx1) = channel::bounded(CHANNEL_BOUND);
    let (tx2, rx2) = channel::bounded(CHANNEL_BOUND);

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
        &hier1.signal_map,
        &hier1.names,
        &hier2.signal_map,
        &hier2.names,
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
        WaveHierarchy,
        Vec<usize>,
        Vec<WaveReader>,
        WaveHierarchy,
        Vec<usize>,
    ),
    String,
> {
    let (r1, h1, o1) = crate::open_wave_files(paths1, options, None)?;
    let (r2, h2, o2) = crate::open_wave_files(paths2, options, None)?;
    Ok((r1, h1, o1, r2, h2, o2))
}
