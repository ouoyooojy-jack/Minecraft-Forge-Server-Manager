//! Bounded console history.
//!
//! A modded server can emit megabytes of log per hour. An unbounded `Vec` is
//! how the manager ends up holding more memory than the JVM it supervises, so
//! history is a ring: newest `cap` lines, oldest evicted.
//!
//! Eviction is not hidden. Every line carries a monotonic `seq`, so a UI that
//! reopens the console can compare the oldest `seq` it receives against the
//! newest it already had and tell the user output was dropped, instead of
//! silently gluing two unrelated stretches of log together.

// Only the read side has a caller so far; `push` belongs to the supervisor,
// which does not exist yet. Remove this when it does.
#![allow(dead_code)]

use std::collections::VecDeque;

use crate::types::{LogLine, LogStream};

pub struct LogBuffer {
    lines: VecDeque<LogLine>,
    cap: usize,
    next_seq: u64,
}

impl LogBuffer {
    pub fn new(cap: usize) -> Self {
        assert!(cap > 0, "log buffer needs room for at least one line");
        Self {
            lines: VecDeque::with_capacity(cap.min(1024)),
            cap,
            // Sequences start at 1, not 0, so `since(0)` means "everything" for
            // a UI that holds no lines yet. With a 0-based first line there is
            // no value it could pass to ask for the whole buffer.
            next_seq: 1,
        }
    }

    /// Append one line, assigning it the next sequence number. Returns the
    /// stored line so the caller can forward exactly what was kept.
    pub fn push(&mut self, stream: LogStream, text: impl Into<String>) -> LogLine {
        let line = LogLine {
            seq: self.next_seq,
            stream,
            text: text.into(),
        };
        self.next_seq += 1;
        if self.lines.len() == self.cap {
            self.lines.pop_front();
        }
        self.lines.push_back(line.clone());
        line
    }

    /// Every line still held, oldest first.
    pub fn snapshot(&self) -> Vec<LogLine> {
        self.lines.iter().cloned().collect()
    }

    /// Lines newer than `after_seq`. Used when the console modal reopens: the
    /// UI sends the last seq it holds and gets only what it missed.
    pub fn since(&self, after_seq: u64) -> Vec<LogLine> {
        self.lines
            .iter()
            .filter(|l| l.seq > after_seq)
            .cloned()
            .collect()
    }

    /// The oldest sequence still retained, or `None` when empty. A UI holding a
    /// last-seen seq lower than this knows lines were evicted.
    pub fn oldest_seq(&self) -> Option<u64> {
        self.lines.front().map(|l| l.seq)
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Drop history but keep counting. Sequence numbers are never reused, so a
    /// stale UI cannot mistake fresh output for lines it already rendered.
    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(buf: &mut LogBuffer, n: usize) {
        for i in 0..n {
            buf.push(LogStream::Stdout, format!("line {i}"));
        }
    }

    #[test]
    fn evicts_oldest_past_capacity() {
        let mut buf = LogBuffer::new(3);
        fill(&mut buf, 5);
        assert_eq!(buf.len(), 3);
        let texts: Vec<_> = buf.snapshot().into_iter().map(|l| l.text).collect();
        assert_eq!(texts, ["line 2", "line 3", "line 4"]);
    }

    #[test]
    fn seq_survives_eviction_so_gaps_are_detectable() {
        let mut buf = LogBuffer::new(3);
        fill(&mut buf, 5);
        // Two lines were dropped: the oldest retained seq is 3, not 1.
        assert_eq!(buf.oldest_seq(), Some(3));
        assert_eq!(buf.snapshot().first().unwrap().seq, 3);
    }

    #[test]
    fn since_zero_returns_everything() {
        let mut buf = LogBuffer::new(10);
        fill(&mut buf, 3);
        assert_eq!(buf.since(0).len(), 3, "a UI with no lines asks with 0");
    }

    #[test]
    fn since_returns_only_newer_lines() {
        let mut buf = LogBuffer::new(10);
        fill(&mut buf, 5);
        let got: Vec<_> = buf.since(2).into_iter().map(|l| l.seq).collect();
        assert_eq!(got, [3, 4, 5]);
        assert!(buf.since(99).is_empty());
    }

    #[test]
    fn clear_keeps_the_sequence_counter_moving() {
        let mut buf = LogBuffer::new(10);
        fill(&mut buf, 3);
        buf.clear();
        assert!(buf.is_empty());
        assert_eq!(buf.push(LogStream::System, "after").seq, 4);
    }
}
