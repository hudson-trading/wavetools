//------------------------------------------------------------------------------
// diff_report.rs
// Sorted text report rows for waveform diffs
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use std::io::{self, Write};

struct DiffOutputRow {
    time: u64,
    name: String,
    sequence: usize,
    text: String,
}

#[derive(Default)]
pub(crate) struct DiffReportRows {
    rows: Vec<DiffOutputRow>,
    sequence: usize,
}

impl DiffReportRows {
    pub(crate) fn push(&mut self, time: u64, name: String, text: String) {
        self.rows.push(DiffOutputRow {
            time,
            name,
            sequence: self.sequence,
            text,
        });
        self.sequence += 1;
    }

    pub(crate) fn write<W: Write>(mut self, writer: &mut W) -> io::Result<()> {
        self.rows.sort_by(|a, b| {
            a.time
                .cmp(&b.time)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.sequence.cmp(&b.sequence))
        });
        for row in self.rows {
            writeln!(writer, "{}", row.text)?;
        }
        Ok(())
    }
}
