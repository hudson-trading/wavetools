//------------------------------------------------------------------------------
// diff.rs
// Waveform diffing: channel-based signal comparison with cross-format support
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::{self, BufRead, Seek, Write};
use std::path::Path;

use crossbeam_channel as channel;
use fst_reader::{FstFilter, FstSignalHandle, FstSignalValue};

use crate::{next_vcd_change, NameOptions, NameTree, SignalMap, WaveHierarchy, WaveReader};

/// Options controlling diff behavior (time range, epsilon).
pub struct DiffOptions {
    pub start: u64,
    pub end: Option<u64>,
    pub real_epsilon: Option<f64>,
    /// Compare only the last change of each signal at each time, ignoring
    /// the changes before it (delta cycles).
    pub no_delta: bool,
}

/// Result of opening two wave file sets for comparison.
pub struct WaveSets {
    pub readers1: Vec<WaveReader>,
    pub hier1: WaveHierarchy,
    pub offsets1: Vec<usize>,
    pub readers2: Vec<WaveReader>,
    pub hier2: WaveHierarchy,
    pub offsets2: Vec<usize>,
}

/// Max queued batches per channel. Each batch holds up to BATCH_SIZE changes.
/// Limit read-ahead when the two input formats decode at different speeds.
/// Four batches allow 8,192 queued changes per input; the comparison also
/// holds a whole time step of each input.
const CHANNEL_BOUND: usize = 4;

/// Number of signal changes collected before sending a batch through the channel.
const BATCH_SIZE: usize = 2048;

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
    fn approx_eq(&self, other: &Self, real_epsilon: Option<f64>, use_real_epsilon: bool) -> bool {
        match (self, other, real_epsilon.filter(|_| use_real_epsilon)) {
            (OwnedSignalValue::Real(a), OwnedSignalValue::Real(b), Some(eps)) => {
                a == b || (a - b).abs() <= eps
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

fn is_real_var_type(var_type: &str) -> bool {
    matches!(
        var_type,
        "real" | "realtime" | "shortreal" | "real_parameter"
    )
}

fn handle_is_real(map: &SignalMap, handle: usize) -> bool {
    map.get(&handle)
        .is_some_and(|info| info.vars.iter().any(|v| is_real_var_type(v.meta.var_type)))
}

/// Whether every variable on `handle` is a four-state vector, not a real or a string.
fn handle_is_vector(map: &SignalMap, handle: usize) -> bool {
    map.get(&handle).is_some_and(|info| {
        info.vars
            .iter()
            .all(|v| !is_real_var_type(v.meta.var_type) && v.meta.var_type != "string")
    })
}

/// Compare vector values of different lengths as the VCD format defines them:
/// the shorter one is extended on the left, with 0 if it starts with 0 or 1
/// and with x or z if it starts with that (as the FST writer does). Values of
/// the same length are left to the plain comparison.
fn extended_vectors_eq(a: &OwnedSignalValue, b: &OwnedSignalValue) -> bool {
    let (OwnedSignalValue::String(a), OwnedSignalValue::String(b)) = (a, b) else {
        return false;
    };
    if a.len() == b.len() {
        return false;
    }
    let (short, long) = if a.len() < b.len() { (a, b) } else { (b, a) };
    let Some(&first) = short.first() else {
        return false;
    };
    let fill = if matches!(first, b'x' | b'z') {
        first
    } else {
        b'0'
    };
    let (extension, rest) = long.split_at(long.len() - short.len());
    rest == short.as_slice() && extension.iter().all(|&c| c == fill)
}

fn handles_are_real(
    hier1: &WaveHierarchy,
    handle1: usize,
    hier2: &WaveHierarchy,
    handle2: usize,
) -> bool {
    handle_is_real(&hier1.signal_map, handle1) && handle_is_real(&hier2.signal_map, handle2)
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
                    for &handle_b in b_handles {
                        if !handles_b.contains(&handle_b) {
                            handles_b.push(handle_b);
                        }
                    }
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
    /// Set if no batch at `time` follows, so the consumer doesn't need to read
    /// ahead to know it has seen the whole time step.
    step_end: bool,
}

/// Flush the current batch through `tx`, replacing it with a fresh empty vec.
fn flush_batch(
    tx: &channel::Sender<TimeBatch>,
    time: u64,
    changes: &mut Vec<SignalChange>,
    step_end: bool,
) {
    let full = std::mem::replace(changes, Vec::with_capacity(BATCH_SIZE));
    let _ = tx.send(TimeBatch {
        time,
        changes: full,
        step_end,
    });
}

/// Build per-reader local handle include sets from a filtered merged signal map.
/// Returns `None` when every source handle is still present so the readers can
/// keep their unfiltered fast path.
fn reader_include_sets(
    signal_map: &SignalMap,
    offsets: &[usize],
    source_signal_count: usize,
) -> Option<Vec<HashSet<usize>>> {
    if signal_map.len() == source_signal_count {
        return None;
    }

    let mut include_sets: Vec<HashSet<usize>> =
        (0..offsets.len()).map(|_| HashSet::new()).collect();

    for &handle in signal_map.keys() {
        let reader_idx = match offsets.binary_search(&handle) {
            Ok(idx) => idx,
            Err(0) => continue,
            Err(idx) => idx - 1,
        };
        include_sets[reader_idx].insert(handle - offsets[reader_idx]);
    }

    Some(include_sets)
}

fn reader_signal_count(reader: &WaveReader) -> usize {
    match reader {
        WaveReader::Fst(reader) => reader.get_header().max_handle as usize,
        WaveReader::Vcd(vcd_data) => vcd_data.id_to_idx.len(),
    }
}

/// Read signals from an FST reader and send same-time batches to a channel.
fn read_and_send_signals<R: BufRead + Seek>(
    mut fst_reader: fst_reader::FstReader<R>,
    filter: fst_reader::FstFilter,
    handle_offset: usize,
    tx: channel::Sender<TimeBatch>,
) -> io::Result<()> {
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    let mut batch_time: u64 = 0;
    let read_result = fst_reader.read_signals(&filter, |time, handle, value| {
        if time < filter.start {
            return;
        }
        if let Some(e) = filter.end {
            if time > e {
                return;
            }
        }
        if !batch.is_empty() && (time != batch_time || batch.len() >= BATCH_SIZE) {
            flush_batch(&tx, batch_time, &mut batch, time != batch_time);
        }
        batch_time = time;
        batch.push(SignalChange {
            handle: handle.get_index() + handle_offset,
            value: OwnedSignalValue::from_fst_value(value),
        });
    });
    read_result.map_err(|e| io::Error::other(format!("Failed to read signals: {}", e)))?;
    if !batch.is_empty() {
        let _ = tx.send(TimeBatch {
            time: batch_time,
            changes: batch,
            step_end: true,
        });
    }
    Ok(())
}

/// Format signal names for a handle on-demand from SignalMap + NameTree.
/// Only called on diff output lines, so the cost is proportional to mismatches.
fn format_handle_names(handle: usize, map: &SignalMap, tree: &NameTree) -> Vec<String> {
    match map.get(&handle) {
        Some(info) => info.vars.iter().map(|v| tree.format_path(v.name)).collect(),
        None => Vec::new(),
    }
}

/// A time in diff output. The last change of a signal at a time is shown at
/// `TIME`, and the changes before it (delta cycles) at `TIME(DELTA)`, where
/// `DELTA` counts the changes from 0.
#[derive(Clone, Copy, Debug)]
struct DeltaTime {
    time: u64,
    delta: Option<usize>,
}

impl DeltaTime {
    /// The time of change `n` of the `count` changes of a signal at `time`.
    fn new(time: u64, n: usize, count: usize) -> Self {
        DeltaTime {
            time,
            delta: (n + 1 < count).then_some(n),
        }
    }
}

impl fmt::Display for DeltaTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.delta {
            Some(delta) => write!(f, "{}({})", self.time, delta),
            None => write!(f, "{}", self.time),
        }
    }
}

/// Write a `value1 != value2` row for every name `handle1` resolves to in file1.
fn emit_value_diff<W: Write>(
    writer: &mut W,
    handle1: usize,
    hier1: &WaveHierarchy,
    time: DeltaTime,
    value1: &OwnedSignalValue,
    value2: &OwnedSignalValue,
) -> std::io::Result<()> {
    for name in format_handle_names(handle1, &hier1.signal_map, &hier1.names) {
        writeln!(writer, "{} {} {} != {}", time, name, value1, value2)?;
    }
    Ok(())
}

/// Write a `value message` row for every name `handle1` resolves to in file1.
fn emit_file1_only<W: Write>(
    writer: &mut W,
    handle1: usize,
    hier1: &WaveHierarchy,
    time: DeltaTime,
    value: &OwnedSignalValue,
    msg: &str,
) -> std::io::Result<()> {
    for name in format_handle_names(handle1, &hier1.signal_map, &hier1.names) {
        writeln!(writer, "{} {} {} {}", time, name, value, msg)?;
    }
    Ok(())
}

/// Write a single `value (only in file2)` row for an already-resolved name.
fn emit_file2_only<W: Write>(
    writer: &mut W,
    name: &str,
    time: DeltaTime,
    value: &OwnedSignalValue,
) -> std::io::Result<()> {
    writeln!(writer, "{} {} {} (only in file2)", time, name, value)
}

/// The number of changes of each signal in a time step, indexed by handle,
/// and for each change, how many changes of the same signal follow it.
#[derive(Default)]
struct StepCounts {
    counts: Vec<u32>,
    remaining: Vec<u32>,
}

impl StepCounts {
    fn count<'a>(&mut self, changes: impl DoubleEndedIterator<Item = &'a SignalChange>) {
        self.remaining.clear();
        // Walk backwards so that the last change of each signal is the first
        // one seen: most signals change once per time step.
        for change in changes.rev() {
            if change.handle >= self.counts.len() {
                self.counts.resize(change.handle + 1, 0);
            }
            let count = &mut self.counts[change.handle];
            self.remaining.push(*count);
            *count += 1;
        }
        self.remaining.reverse();
    }

    fn get(&self, handle: usize) -> usize {
        self.counts[handle] as usize
    }

    fn reset(&mut self, handle: usize) {
        self.counts[handle] = 0;
    }

    fn clear<'a>(&mut self, changes: impl Iterator<Item = &'a SignalChange>) {
        for change in changes {
            self.counts[change.handle] = 0;
        }
    }
}

/// File2 changes read from the channel but not compared yet, by (time, handle).
#[derive(Default)]
struct Buffered2 {
    /// The first change of a signal at a time.
    first: HashMap<(u64, usize), OwnedSignalValue>,
    /// Its later changes at the same time, in order. Most signals change at
    /// most once per time step, so this is usually empty.
    later: HashMap<(u64, usize), Vec<OwnedSignalValue>>,
}

impl Buffered2 {
    fn push(&mut self, key: (u64, usize), value: OwnedSignalValue) {
        match self.first.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(_) => self.later.entry(key).or_default().push(value),
        }
    }

    fn has_later(&self, key: (u64, usize)) -> bool {
        !self.later.is_empty() && self.later.contains_key(&key)
    }

    /// The position and value of the last change for `key`.
    fn last(&self, key: (u64, usize)) -> Option<(usize, &OwnedSignalValue)> {
        let first = self.first.get(&key)?;
        if !self.later.is_empty() {
            if let Some(later) = self.later.get(&key) {
                if let Some(value) = later.last() {
                    return Some((later.len(), value));
                }
            }
        }
        Some((0, first))
    }

    /// Change `n` for `key`.
    fn get(&self, key: (u64, usize), n: usize) -> Option<&OwnedSignalValue> {
        match n {
            0 => self.first.get(&key),
            _ => self.later.get(&key)?.get(n - 1),
        }
    }

    /// Remove the changes for `key` once they have been compared with a
    /// signal that had `compared_before_last` changes before its last one.
    /// File2's changes before its last that weren't compared are moved to
    /// `leftovers`, to report at the end.
    fn evict(
        &mut self,
        key: (u64, usize),
        compared_before_last: usize,
        leftovers: Option<&mut Vec<(DeltaTime, usize, OwnedSignalValue)>>,
    ) {
        if !self.has_later(key) {
            self.first.remove(&key);
            return;
        }
        let values = self.take(key);
        let count = values.len();
        if let Some(leftovers) = leftovers {
            for (n, value) in values
                .into_iter()
                .enumerate()
                .take(count - 1)
                .skip(compared_before_last)
            {
                leftovers.push((DeltaTime::new(key.0, n, count), key.1, value));
            }
        }
    }

    /// Remove and return all changes for `key`, in order.
    fn take(&mut self, key: (u64, usize)) -> Vec<OwnedSignalValue> {
        let Some(first) = self.first.remove(&key) else {
            return Vec::new();
        };
        let mut values = vec![first];
        if let Some(later) = self.later.remove(&key) {
            values.extend(later);
        }
        values
    }
}

/// Consumes batches from `rx1` (file1) and `rx2` (file2), buffering as needed to
/// match signals across potentially different orderings. Returns `true` if differences
/// were found.
///
/// File1 is compared one time step at a time, once file2 has been read past
/// that time. The last change of a signal at a time is compared with its last
/// change at that time in file2, and the changes before it (delta cycles) with
/// the changes at the same position in file2, unless `no_delta` is set.
#[allow(clippy::too_many_arguments)]
fn compare_signal_channels<W: Write>(
    writer: &mut W,
    rx1: channel::Receiver<TimeBatch>,
    rx2: channel::Receiver<TimeBatch>,
    handle_mapping: &HashMap<usize, Vec<usize>>,
    hier1: &WaveHierarchy,
    hier2: &WaveHierarchy,
    real_epsilon: Option<f64>,
    no_delta: bool,
) -> std::io::Result<bool> {
    let mut has_differences = false;
    // File2 changes we've read from the channel but haven't matched yet.
    let mut buffered2 = Buffered2::default();
    // File2 changes before the last at a time that file1 had no counterpart
    // for, reported with the rest of file2's unmatched changes at the end.
    let mut leftovers2: Vec<(DeltaTime, usize, OwnedSignalValue)> = Vec::new();
    // The time of the last batch read from file2.
    let mut latest_time2: Option<u64> = None;
    let mut source2_ended = false;

    // Whether the last batch read from file2 ended its time step.
    let mut step_end2 = false;

    // File1's changes at the current time step, which may span batches.
    let mut step: Vec<Vec<SignalChange>> = Vec::new();
    let mut counts1 = StepCounts::default();
    // File2 handles that several file1 handles map to. Their buffer entries are
    // evicted when the time step is done; the others as soon as compared.
    let mut shared2 = HashSet::new();
    {
        let mut seen = HashSet::new();
        for &handle2 in handle_mapping.values().flatten() {
            if !seen.insert(handle2) {
                shared2.insert(handle2);
            }
        }
    }
    // Handles of `shared2` compared at the current time step, with the number
    // of changes before the last of the file1 signal compared with them.
    let mut compared: Vec<(usize, usize)> = Vec::new();

    // A file1 batch read past the end of the previous time step.
    let mut pending1: Option<TimeBatch> = None;
    while let Some(first) = pending1.take().or_else(|| rx1.recv().ok()) {
        let time = first.time;
        let mut step_end = first.step_end;
        let mut file2_has_time = latest_time2 == Some(time);
        // File1 and file2 batches at this time read so far. They are read in
        // turn, so that neither input's reader gets far ahead of the other.
        let mut batches1 = 0;
        let mut batches2 = usize::from(file2_has_time);
        let mut next1 = Some(first);
        loop {
            if let Some(batch) = next1.take() {
                step.push(batch.changes);
                batches1 += 1;
            }
            // Read file2 up to this time, and as many batches at it as file1.
            while !source2_ended
                && latest_time2
                    .is_none_or(|t2| t2 < time || (t2 == time && !step_end2 && batches2 < batches1))
            {
                match rx2.recv() {
                    Ok(batch2) => {
                        if batch2.time == time {
                            file2_has_time = true;
                            batches2 += 1;
                        }
                        latest_time2 = Some(batch2.time);
                        step_end2 = batch2.step_end;
                        for c in batch2.changes {
                            buffered2.push((batch2.time, c.handle), c.value);
                        }
                    }
                    Err(_) => source2_ended = true,
                }
            }
            if step_end {
                break;
            }
            match rx1.recv() {
                Ok(batch) if batch.time == time => {
                    step_end = batch.step_end;
                    next1 = Some(batch);
                }
                Ok(batch) => {
                    pending1 = Some(batch);
                    break;
                }
                Err(_) => break,
            }
        }

        {
            // Read file2 until every change at this time is buffered.
            while !source2_ended
                && !latest_time2.is_some_and(|t2| t2 > time || (t2 == time && step_end2))
            {
                match rx2.recv() {
                    Ok(batch2) => {
                        file2_has_time |= batch2.time == time;
                        latest_time2 = Some(batch2.time);
                        step_end2 = batch2.step_end;
                        for c in batch2.changes {
                            buffered2.push((batch2.time, c.handle), c.value);
                        }
                    }
                    Err(_) => source2_ended = true,
                }
            }

            counts1.count(step.iter().flatten());
            let remaining1 = std::mem::take(&mut counts1.remaining);
            // Consume the changes, so that their values are freed as they are
            // compared rather than all at the end of the step.
            for (change1, &remaining) in step.drain(..).flatten().zip(&remaining1) {
                let remaining = remaining as usize;
                let before_last = counts1.get(change1.handle) - 1;
                if remaining == 0 {
                    counts1.reset(change1.handle);
                }
                if remaining > 0 && no_delta {
                    continue;
                }
                let Some(handles2) = handle_mapping.get(&change1.handle) else {
                    continue;
                };
                for &handle2 in handles2 {
                    let key = (time, handle2);
                    {
                        let last2 = buffered2.last(key);
                        let (delta_time, value2) = if remaining == 0 {
                            (
                                DeltaTime { time, delta: None },
                                last2.map(|(_, value)| value),
                            )
                        } else {
                            let n = before_last - remaining;
                            let value2 = match last2 {
                                Some((n2, _)) if n < n2 => buffered2.get(key, n),
                                _ => None,
                            };
                            (
                                DeltaTime {
                                    time,
                                    delta: Some(n),
                                },
                                value2,
                            )
                        };
                        if let Some(value2) = value2 {
                            let use_real_epsilon =
                                handles_are_real(hier1, change1.handle, hier2, handle2);
                            if !change1
                                .value
                                .approx_eq(value2, real_epsilon, use_real_epsilon)
                                && !(extended_vectors_eq(&change1.value, value2)
                                    && handle_is_vector(&hier1.signal_map, change1.handle)
                                    && handle_is_vector(&hier2.signal_map, handle2))
                            {
                                has_differences = true;
                                emit_value_diff(
                                    writer,
                                    change1.handle,
                                    hier1,
                                    delta_time,
                                    &change1.value,
                                    value2,
                                )?;
                            }
                        } else {
                            has_differences = true;
                            let msg = if file2_has_time {
                                "(not in file2)"
                            } else {
                                "(missing time in file2)"
                            };
                            emit_file1_only(
                                writer,
                                change1.handle,
                                hier1,
                                delta_time,
                                &change1.value,
                                msg,
                            )?;
                        }
                    }
                    // After its last change, this file1 signal is done with
                    // file2's changes at this time.
                    if remaining == 0 {
                        if shared2.is_empty() || !shared2.contains(&handle2) {
                            let leftovers = (!no_delta).then_some(&mut leftovers2);
                            buffered2.evict(key, before_last, leftovers);
                        } else {
                            compared.push((handle2, before_last));
                        }
                    }
                }
            }

            // Several file1 handles can map to one file2 handle; the one with
            // the most changes decides how many were compared.
            counts1.remaining = remaining1;
            let mut repeated: Vec<(usize, usize)> = std::mem::take(&mut compared);
            repeated.sort_unstable();
            repeated.reverse();
            repeated.dedup_by_key(|&mut (handle2, _)| handle2);
            for (handle2, compared_before_last) in repeated {
                let leftovers = (!no_delta).then_some(&mut leftovers2);
                buffered2.evict((time, handle2), compared_before_last, leftovers);
            }
        }
    }

    // Anything still in the buffer was in file2 but never matched by file1.
    // Drop entries whose handles were filtered out of hier2 -- the streaming
    // reader still emits them but they're not part of the comparison set.
    // Sort rows before printing so diff output does not depend on HashMap
    // iteration order.
    let mut owned = leftovers2;
    let keys: Vec<(u64, usize)> = buffered2.first.keys().copied().collect();
    for key in keys {
        let values = buffered2.take(key);
        let count = values.len();
        for (n, value) in values.into_iter().enumerate() {
            if n + 1 == count || !no_delta {
                owned.push((DeltaTime::new(key.0, n, count), key.1, value));
            }
        }
    }
    owned.retain(|(_, handle, _)| hier2.signal_map.contains_key(handle));
    if !owned.is_empty() {
        has_differences = true;
    }
    let mut file2_only_rows: Vec<(u64, String, usize, usize)> = Vec::new();
    for (idx, (time, handle, _)) in owned.iter().enumerate() {
        for name in format_handle_names(*handle, &hier2.signal_map, &hier2.names) {
            // The last change at a time sorts after the ones before it.
            file2_only_rows.push((time.time, name, time.delta.unwrap_or(usize::MAX), idx));
        }
    }
    file2_only_rows.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
    for (_, name, _, idx) in file2_only_rows {
        emit_file2_only(writer, &name, owned[idx].0, &owned[idx].2)?;
    }

    // Drain any remaining file2 batches we never read from the channel, one
    // time step at a time. Only changes for handles still in hier2 count --
    // filtered-out handles are streamed by the reader but excluded from the
    // comparison set.
    if !source2_ended {
        let mut step2: Vec<Vec<SignalChange>> = Vec::new();
        let mut step2_time: Option<u64> = None;
        let mut counts2 = StepCounts::default();
        loop {
            let batch2 = rx2.recv().ok();
            if matches!((&batch2, step2_time), (Some(batch), Some(time)) if batch.time == time) {
                step2.push(batch2.unwrap().changes);
                continue;
            }
            if let Some(time) = step2_time {
                counts2.count(step2.iter().flatten());
                for (change2, &remaining) in step2.iter().flatten().zip(&counts2.remaining) {
                    let remaining = remaining as usize;
                    if (remaining > 0 && no_delta)
                        || !hier2.signal_map.contains_key(&change2.handle)
                    {
                        continue;
                    }
                    has_differences = true;
                    let count = counts2.get(change2.handle);
                    let delta_time = DeltaTime::new(time, count - 1 - remaining, count);
                    for name in format_handle_names(change2.handle, &hier2.signal_map, &hier2.names)
                    {
                        emit_file2_only(writer, &name, delta_time, &change2.value)?;
                    }
                }
            }
            counts2.clear(step2.iter().flatten());
            step2.clear();
            match batch2 {
                Some(batch) => {
                    step2_time = Some(batch.time);
                    step2.push(batch.changes);
                }
                None => break,
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
    include: Option<HashSet<usize>>,
    start: u64,
    end: Option<u64>,
    tx: channel::Sender<TimeBatch>,
) -> io::Result<()> {
    match reader {
        WaveReader::Fst(fst_reader) => {
            let include = include.map(|handles| {
                handles
                    .into_iter()
                    .map(FstSignalHandle::from_index)
                    .collect()
            });
            let filter = FstFilter {
                start,
                end,
                include,
            };
            read_and_send_signals(*fst_reader, filter, handle_offset, tx)?;
        }
        WaveReader::Vcd(mut vcd_data) => {
            let mut batch = Vec::with_capacity(BATCH_SIZE);
            let mut batch_time: u64 = 0;
            while let Some((time, handle, value_str)) = next_vcd_change(&mut vcd_data)? {
                if time < start {
                    continue;
                }
                if let Some(e) = end {
                    if time > e {
                        break;
                    }
                }
                if include
                    .as_ref()
                    .is_some_and(|handles| !handles.contains(&handle))
                {
                    continue;
                }
                if !batch.is_empty() && (time != batch_time || batch.len() >= BATCH_SIZE) {
                    flush_batch(&tx, batch_time, &mut batch, time != batch_time);
                }
                batch_time = time;
                batch.push(SignalChange {
                    handle: handle + handle_offset,
                    value: OwnedSignalValue::String(value_str.into_bytes()),
                });
            }
            if !batch.is_empty() {
                let _ = tx.send(TimeBatch {
                    time: batch_time,
                    changes: batch,
                    step_end: true,
                });
            }
        }
    }
    Ok(())
}

/// Send changes from multiple WaveReaders through a single channel, merging in time order.
///
/// Each inner reader produces same-time `TimeBatch`es. The k-way merge picks the
/// batch with the smallest time and forwards it directly -- no re-batching needed.
fn send_merged_wave_changes(
    readers: Vec<WaveReader>,
    offsets: &[usize],
    include_sets: Option<Vec<HashSet<usize>>>,
    start: u64,
    end: Option<u64>,
    tx: channel::Sender<TimeBatch>,
) -> io::Result<()> {
    debug_assert_eq!(readers.len(), offsets.len());
    if let Some(sets) = &include_sets {
        debug_assert_eq!(readers.len(), sets.len());
    }

    if readers.len() == 1 {
        return send_wave_changes(
            readers.into_iter().next().unwrap(),
            offsets[0],
            include_sets.and_then(|sets| sets.into_iter().next()),
            start,
            end,
            tx,
        );
    }

    // Spawn a thread per reader, each sending TimeBatches to its own channel.
    let mut inner_rxs = Vec::with_capacity(readers.len());
    let mut threads = Vec::new();

    let mut include_sets = include_sets.map(Vec::into_iter);
    for (reader, &offset) in readers.into_iter().zip(offsets.iter()) {
        let include = include_sets.as_mut().and_then(|sets| sets.next());
        let (inner_tx, inner_rx) = channel::bounded(CHANNEL_BOUND);
        threads.push(std::thread::spawn(move || {
            send_wave_changes(reader, offset, include, start, end, inner_tx)
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
                let mut batch = heads[idx].take().unwrap();
                // Another reader may still have changes at this time.
                batch.step_end &= heads.iter().flatten().all(|b| b.time != batch.time);
                let _ = tx.send(batch);
                heads[idx] = inner_rxs[idx].recv().ok();
            }
            None => break,
        }
    }

    for t in threads {
        match t.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(io::Error::other("wavediff reader thread panicked")),
        }
    }

    Ok(())
}

/// Compare metadata and attributes for signals that share the same name across two files.
/// Returns a list of human-readable difference strings.
/// Direction is only compared if both sides have an explicit (non-implicit) direction.
pub fn compare_signal_meta(hier1: &WaveHierarchy, hier2: &WaveHierarchy) -> Vec<String> {
    let mut diffs = Vec::new();

    // Build name -> VarEntry lookup for both maps
    let entries1: HashMap<String, &crate::VarEntry> = hier1
        .signal_map
        .values()
        .flat_map(|info| {
            info.vars
                .iter()
                .map(|v| (hier1.names.format_path(v.name), v))
        })
        .collect();
    let entries2: HashMap<String, &crate::VarEntry> = hier2
        .signal_map
        .values()
        .flat_map(|info| {
            info.vars
                .iter()
                .map(|v| (hier2.names.format_path(v.name), v))
        })
        .collect();

    let mut common: Vec<&String> = entries1
        .keys()
        .filter(|n| entries2.contains_key(*n))
        .collect();
    common.sort();

    for name in common {
        let v1 = entries1[name];
        let v2 = entries2[name];
        if v1.meta.var_type != v2.meta.var_type {
            diffs.push(format!(
                "{}: type {} != {}",
                name, v1.meta.var_type, v2.meta.var_type
            ));
        }
        if v1.meta.size != v2.meta.size {
            diffs.push(format!(
                "{}: size {} != {}",
                name, v1.meta.size, v2.meta.size
            ));
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
            let a1 = if v1.attrs.is_empty() {
                "(none)".to_string()
            } else {
                v1.attrs.join("; ")
            };
            let a2 = if v2.attrs.is_empty() {
                "(none)".to_string()
            } else {
                v2.attrs.join("; ")
            };
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
pub fn diff_waves<W: Write>(
    writer: &mut W,
    reader1: WaveReader,
    hier1: WaveHierarchy,
    reader2: WaveReader,
    hier2: WaveHierarchy,
    options: &DiffOptions,
) -> std::io::Result<bool> {
    diff_wave_sets(
        writer,
        WaveSets {
            readers1: vec![reader1],
            hier1,
            offsets1: vec![0],
            readers2: vec![reader2],
            hier2,
            offsets2: vec![0],
        },
        options,
    )
}

/// Compare two sets of waveform files and write differences.
///
/// Each set is a vec of readers with corresponding handle offsets.
/// Returns `true` if differences were found.
pub fn diff_wave_sets<W: Write>(
    writer: &mut W,
    sets: WaveSets,
    options: &DiffOptions,
) -> std::io::Result<bool> {
    let handle_mapping = build_handle_mapping(
        &sets.hier1.signal_map,
        &sets.hier1.names,
        &sets.hier2.signal_map,
        &sets.hier2.names,
    );

    let (tx1, rx1) = channel::bounded(CHANNEL_BOUND);
    let (tx2, rx2) = channel::bounded(CHANNEL_BOUND);

    let start = options.start;
    let end = options.end;
    let WaveSets {
        readers1,
        hier1,
        offsets1,
        readers2,
        hier2,
        offsets2,
    } = sets;
    let source_signal_count1 = readers1.iter().map(reader_signal_count).sum();
    let source_signal_count2 = readers2.iter().map(reader_signal_count).sum();
    let include_sets1 = reader_include_sets(&hier1.signal_map, &offsets1, source_signal_count1);
    let include_sets2 = reader_include_sets(&hier2.signal_map, &offsets2, source_signal_count2);

    let thread1 = std::thread::spawn(move || {
        send_merged_wave_changes(readers1, &offsets1, include_sets1, start, end, tx1)
    });
    let thread2 = std::thread::spawn(move || {
        send_merged_wave_changes(readers2, &offsets2, include_sets2, start, end, tx2)
    });

    let result = compare_signal_channels(
        writer,
        rx1,
        rx2,
        &handle_mapping,
        &hier1,
        &hier2,
        options.real_epsilon,
        options.no_delta,
    );

    let thread1_result = match thread1.join() {
        Ok(r) => r,
        Err(_) => Err(io::Error::other("wavediff set1 reader thread panicked")),
    };
    let thread2_result = match thread2.join() {
        Ok(r) => r,
        Err(_) => Err(io::Error::other("wavediff set2 reader thread panicked")),
    };

    let has_differences = result?;
    thread1_result?;
    thread2_result?;

    Ok(has_differences)
}

/// Open two sets of waveform files and return readers and merged hierarchies
pub fn open_and_read_wave_sets(
    paths1: &[&Path],
    paths2: &[&Path],
    options: &NameOptions,
) -> Result<WaveSets, String> {
    let (readers1, hier1, offsets1) = crate::open_wave_files(paths1, options, None)?;
    let (readers2, hier2, offsets2) = crate::open_wave_files(paths2, options, None)?;
    Ok(WaveSets {
        readers1,
        hier1,
        offsets1,
        readers2,
        hier2,
        offsets2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SignalInfo, VarEntry, VarMeta, IMPLICIT_DIRECTION};

    #[test]
    fn equal_infinite_reals_match_with_epsilon() {
        for value in [f64::INFINITY, f64::NEG_INFINITY] {
            let left = OwnedSignalValue::from_fst_value(FstSignalValue::Real(value));
            let right = OwnedSignalValue::from_fst_value(FstSignalValue::Real(value));
            assert_eq!(left, right);
            for epsilon in [0.0, 0.001] {
                assert!(left.approx_eq(&right, Some(epsilon), true));
            }
        }
    }

    #[test]
    fn real_epsilon_keeps_distinct_non_finite_values_distinct() {
        for (left, right) in [
            (f64::INFINITY, f64::NEG_INFINITY),
            (f64::INFINITY, 0.0),
            (f64::NAN, f64::NAN),
            (f64::NAN, f64::INFINITY),
        ] {
            assert!(!OwnedSignalValue::Real(left).approx_eq(
                &OwnedSignalValue::Real(right),
                Some(0.001),
                true,
            ));
        }
    }

    #[test]
    fn channel_comparison_preserves_equal_infinite_reals() {
        let (_, hierarchy) = crate::open_wave_file(
            Path::new("tests/data/real_base.fst"),
            &NameOptions::default(),
        )
        .unwrap();
        for value in [f64::INFINITY, f64::NEG_INFINITY] {
            let (tx1, rx1) = channel::unbounded();
            let (tx2, rx2) = channel::unbounded();
            for tx in [tx1, tx2] {
                tx.send(TimeBatch {
                    step_end: false,
                    time: 0,
                    changes: vec![SignalChange {
                        handle: 0,
                        value: OwnedSignalValue::from_fst_value(FstSignalValue::Real(value)),
                    }],
                })
                .unwrap();
            }
            let mut output = Vec::new();
            assert!(!compare_signal_channels(
                &mut output,
                rx1,
                rx2,
                &HashMap::from([(0, vec![0])]),
                &hierarchy,
                &hierarchy,
                Some(0.001),
                false,
            )
            .unwrap());
            assert!(output.is_empty());
        }
    }

    fn signal_map(handles: &[usize]) -> SignalMap {
        handles
            .iter()
            .copied()
            .map(|handle| {
                (
                    handle,
                    SignalInfo {
                        vars: vec![VarEntry {
                            name: 0,
                            meta: VarMeta {
                                var_type: "wire",
                                size: 1,
                                direction: IMPLICIT_DIRECTION,
                            },
                            attrs: Vec::new(),
                        }],
                    },
                )
            })
            .collect()
    }

    #[test]
    fn comparison_matches_repeated_values_in_order() {
        let (_, hierarchy) =
            crate::open_wave_file(Path::new("tests/data/counter.vcd"), &NameOptions::default())
                .unwrap();
        let name = hierarchy
            .names
            .format_path(hierarchy.signal_map[&0].vars[0].name);
        for (values1, values2, no_delta, expected) in [
            ("01", "01", false, String::new()),
            ("010", "010", false, String::new()),
            (
                "0",
                "01",
                false,
                format!("0 {name} 0 != 1\n0(0) {name} 0 (only in file2)\n"),
            ),
            (
                "01",
                "0",
                false,
                format!("0(0) {name} 0 (not in file2)\n0 {name} 1 != 0\n"),
            ),
            ("011", "001", false, format!("0(1) {name} 1 != 0\n")),
            ("011", "001", true, String::new()),
            ("01", "0", true, format!("0 {name} 1 != 0\n")),
            ("0", "01", true, format!("0 {name} 0 != 1\n")),
        ] {
            let (tx1, rx1) = channel::unbounded();
            let (tx2, rx2) = channel::unbounded();
            for (tx, values) in [(tx1, values1), (tx2, values2)] {
                tx.send(TimeBatch {
                    step_end: false,
                    time: 0,
                    changes: values
                        .bytes()
                        .map(|value| SignalChange {
                            handle: 0,
                            value: OwnedSignalValue::String(vec![value]),
                        })
                        .collect(),
                })
                .unwrap();
            }
            let mut output = Vec::new();
            let has_differences = compare_signal_channels(
                &mut output,
                rx1,
                rx2,
                &HashMap::from([(0, vec![0])]),
                &hierarchy,
                &hierarchy,
                None,
                no_delta,
            )
            .unwrap();
            assert_eq!(has_differences, !expected.is_empty());
            assert_eq!(String::from_utf8(output).unwrap(), expected);
        }
    }

    #[test]
    fn signal_missing_at_a_time_file2_has_is_not_in_file2() {
        let (_, hierarchy) =
            crate::open_wave_file(Path::new("tests/data/counter.vcd"), &NameOptions::default())
                .unwrap();
        let name = |handle: usize| {
            hierarchy
                .names
                .format_path(hierarchy.signal_map[&handle].vars[0].name)
        };
        let change = |handle, value: &[u8]| SignalChange {
            handle,
            value: OwnedSignalValue::String(value.to_vec()),
        };
        let (tx1, rx1) = channel::unbounded();
        let (tx2, rx2) = channel::unbounded();
        tx1.send(TimeBatch {
            step_end: false,
            time: 10,
            changes: vec![change(0, b"1"), change(1, b"1")],
        })
        .unwrap();
        for (time, value) in [(10, b"1"), (20, b"0")] {
            tx2.send(TimeBatch {
                step_end: false,
                time,
                changes: vec![change(0, value)],
            })
            .unwrap();
        }
        drop((tx1, tx2));
        let mut output = Vec::new();
        assert!(compare_signal_channels(
            &mut output,
            rx1,
            rx2,
            &HashMap::from([(0, vec![0]), (1, vec![1])]),
            &hierarchy,
            &hierarchy,
            None,
            false,
        )
        .unwrap());
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!(
                "10 {} 1 (not in file2)\n20 {} 0 (only in file2)\n",
                name(1),
                name(0)
            )
        );
    }

    #[test]
    fn leftover_repeated_values_keep_their_order_across_batches() {
        let (_, hierarchy) =
            crate::open_wave_file(Path::new("tests/data/counter.vcd"), &NameOptions::default())
                .unwrap();
        let name = hierarchy
            .names
            .format_path(hierarchy.signal_map[&0].vars[0].name);
        let change = |value: &[u8]| SignalChange {
            handle: 0,
            value: OwnedSignalValue::String(value.to_vec()),
        };
        let (tx1, rx1) = channel::unbounded();
        let (tx2, rx2) = channel::unbounded();
        for time in [0, 1] {
            tx1.send(TimeBatch {
                step_end: false,
                time,
                changes: vec![change(b"0")],
            })
            .unwrap();
        }
        // A full batch of changes at time 0, then one more at time 0 in the next batch.
        let mut first = vec![change(b"0")];
        first.extend((1..BATCH_SIZE).map(|_| change(b"1")));
        for (time, changes) in [(0, first), (0, vec![change(b"x")]), (1, vec![change(b"0")])] {
            tx2.send(TimeBatch {
                step_end: false,
                time,
                changes,
            })
            .unwrap();
        }
        drop((tx1, tx2));
        let mut output = Vec::new();
        assert!(compare_signal_channels(
            &mut output,
            rx1,
            rx2,
            &HashMap::from([(0, vec![0])]),
            &hierarchy,
            &hierarchy,
            None,
            false,
        )
        .unwrap());
        let mut expected = format!("0 {name} 0 != x\n0(0) {name} 0 (only in file2)\n");
        for n in 1..BATCH_SIZE {
            expected.push_str(&format!("0({n}) {name} 1 (only in file2)\n"));
        }
        assert_eq!(String::from_utf8(output).unwrap(), expected);
    }

    #[test]
    fn short_vectors_match_their_extension() {
        let v = |s: &str| OwnedSignalValue::String(s.as_bytes().to_vec());
        assert!(extended_vectors_eq(&v("10"), &v("0010")));
        assert!(extended_vectors_eq(&v("0010"), &v("10")));
        assert!(extended_vectors_eq(&v("x1"), &v("xxx1")));
        assert!(extended_vectors_eq(&v("z"), &v("zzzz")));
        assert!(!extended_vectors_eq(&v("10"), &v("1010")));
        assert!(!extended_vectors_eq(&v("x1"), &v("00x1")));
        assert!(!extended_vectors_eq(&v("10"), &v("10")));
        assert!(!extended_vectors_eq(&v(""), &v("0")));
        assert!(!extended_vectors_eq(&v("1"), &OwnedSignalValue::Real(1.0)));
    }

    #[test]
    fn reader_include_sets_split_merged_handles_by_reader_offset() {
        let map = signal_map(&[0, 1, 4, 6, 9]);

        let include_sets = reader_include_sets(&map, &[0, 4, 9], 12).unwrap();

        assert_eq!(include_sets.len(), 3);
        assert_eq!(include_sets[0], HashSet::from([0, 1]));
        assert_eq!(include_sets[1], HashSet::from([0, 2]));
        assert_eq!(include_sets[2], HashSet::from([0]));
    }

    #[test]
    fn reader_include_sets_preserve_empty_filtered_readers() {
        let map = signal_map(&[5]);

        let include_sets = reader_include_sets(&map, &[0, 5], 10).unwrap();

        assert_eq!(include_sets.len(), 2);
        assert!(include_sets[0].is_empty());
        assert_eq!(include_sets[1], HashSet::from([0]));
    }

    #[test]
    fn reader_include_sets_skip_unfiltered_sources() {
        let map = signal_map(&[0, 1, 2, 3, 4]);

        assert!(reader_include_sets(&map, &[0, 3], 5).is_none());
    }

    #[test]
    fn native_reader_counts_match_unfiltered_hierarchies() {
        for path in ["tests/data/counter.fst", "tests/data/counter.vcd"] {
            let (reader, hierarchy) =
                crate::open_wave_file(Path::new(path), &NameOptions::default()).unwrap();
            let source_signal_count = reader_signal_count(&reader);

            assert_eq!(source_signal_count, hierarchy.signal_map.len(), "{path}");
            assert!(
                reader_include_sets(&hierarchy.signal_map, &[0], source_signal_count).is_none()
            );
        }

        let paths = [
            Path::new("tests/data/set_clk.vcd"),
            Path::new("tests/data/set_counter.vcd"),
        ];
        let (readers, hierarchy, offsets) =
            crate::open_wave_files(&paths, &NameOptions::default(), None).unwrap();
        let source_signal_count = readers.iter().map(reader_signal_count).sum();

        assert_eq!(source_signal_count, hierarchy.signal_map.len());
        assert!(
            reader_include_sets(&hierarchy.signal_map, &offsets, source_signal_count).is_none()
        );
    }
}
