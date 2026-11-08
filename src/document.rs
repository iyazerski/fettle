//! A text field's text as the user edits it, and the mistakes found in it.
//!
//! An edit shifts the mistakes around it and marks the paragraphs it touched
//! for checking, so typing re-checks only the paragraph typed in. Checks run
//! one at a time, a span of whole paragraphs at most [`CHUNK`] long each, so
//! a long text shows its first mistakes early and an edit spoils little work.
//!
//! Ranges count UTF-16 code units, as the Accessibility API and `NSString` do.

use std::ops::Range;

use crate::spelling::Mistake;

/// How many units a check covers at most, unless one paragraph is longer:
/// about 7 ms of checking.
const CHUNK: usize = 2_000;

#[derive(Debug)]
pub struct Document {
    text: Vec<u16>,
    /// Sorted by start.
    mistakes: Vec<Mistake>,
    /// Spans still to check: sorted, apart, and whole paragraphs.
    unchecked: Vec<Range<usize>>,
    check: Check,
}

#[derive(Debug)]
enum Check {
    Idle,
    Running {
        id: u64,
        /// Where the span checked is in the text now.
        span: Range<usize>,
        /// Whether an edit changed the span since, so the answer is stale.
        outdated: bool,
    },
}

/// A span to check: its text, and where it starts.
#[derive(Debug)]
pub struct Request {
    pub id: u64,
    pub text: Vec<u16>,
}

impl Document {
    /// Start on `text`, all of it unchecked.
    pub fn new(text: Vec<u16>) -> Self {
        let mut doc = Self {
            text,
            mistakes: Vec::new(),
            unchecked: Vec::new(),
            check: Check::Idle,
        };
        if !doc.text.is_empty() {
            doc.mark(0..doc.text.len());
        }
        doc
    }

    pub fn text(&self) -> &[u16] {
        &self.text
    }

    pub fn mistakes(&self) -> &[Mistake] {
        &self.mistakes
    }

    /// Take `new` as the text. Returns where the paragraph the edit starts in
    /// begins, as far as layout before it is unchanged, or `None` if the text
    /// is the same.
    pub fn edit(&mut self, new: &[u16]) -> Option<usize> {
        if self.text == new {
            return None;
        }
        let old = &self.text;
        let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
        let room = old.len().min(new.len()) - prefix;
        let suffix = old
            .iter()
            .rev()
            .zip(new.iter().rev())
            .take(room)
            .take_while(|(a, b)| a == b)
            .count();
        let edit = Edit {
            start: prefix,
            old_end: old.len() - suffix,
            new_end: new.len() - suffix,
        };

        self.mistakes.retain_mut(|m| {
            let kept = !edit.overlaps(&m.range);
            m.range = edit.map(&m.range);
            kept
        });
        for span in &mut self.unchecked {
            *span = edit.map(span);
        }
        let mut redo = None;
        if let Check::Running { span, outdated, .. } = &mut self.check {
            *outdated |= edit.touches(span);
            *span = edit.map(span);
            if *outdated {
                redo = Some(span.clone());
            }
        }
        if let Some(span) = redo {
            self.mark(span);
        }
        let span = paragraphs(new, edit.start..edit.new_end);
        let start = span.start;
        self.mark(span);
        self.text = new.to_vec();
        Some(start)
    }

    /// Return the next span to check as `id`, unless one is being checked or
    /// none is left.
    pub fn next(&mut self, id: u64) -> Option<Request> {
        if matches!(self.check, Check::Running { .. }) || self.unchecked.is_empty() {
            return None;
        }
        let first = self.unchecked.remove(0);
        let end = if first.len() > CHUNK {
            // Through the break ending the paragraph the chunk ends in, so the
            // rest starts a paragraph.
            let end = paragraph(&self.text, first.start + CHUNK - 1).end;
            (end + 1).min(first.end)
        } else {
            first.end
        };
        if end < first.end {
            self.unchecked.insert(0, end..first.end);
        }
        let span = first.start..end;
        let text = self.text[span.clone()].to_vec();
        self.check = Check::Running {
            id,
            span,
            outdated: false,
        };
        Some(Request { id, text })
    }

    /// Take the mistakes check `id` found. Returns where they changed, if
    /// they did: not for a stale or unknown check.
    pub fn finish(&mut self, id: u64, found: Vec<Mistake>) -> Option<Range<usize>> {
        let Check::Running {
            id: running,
            span,
            outdated,
        } = &self.check
        else {
            return None;
        };
        if *running != id {
            return None;
        }
        let (span, outdated) = (span.clone(), *outdated);
        self.check = Check::Idle;
        if outdated {
            return None;
        }
        let from = self
            .mistakes
            .partition_point(|m| m.range.start < span.start);
        let to = self.mistakes.partition_point(|m| m.range.start < span.end);
        let found = found.into_iter().map(|m| Mistake {
            range: m.range.start + span.start..m.range.end + span.start,
            kind: m.kind,
        });
        self.mistakes.splice(from..to, found);
        Some(span)
    }

    /// Add `span` to the unchecked spans, merging it with those it meets.
    fn mark(&mut self, mut span: Range<usize>) {
        let from = self.unchecked.partition_point(|s| s.end < span.start);
        let to = self.unchecked.partition_point(|s| s.start <= span.end);
        if from < to {
            span.start = span.start.min(self.unchecked[from].start);
            span.end = span.end.max(self.unchecked[to - 1].end);
        }
        self.unchecked.splice(from..to, [span]);
    }
}

/// An edit that replaced `start..old_end` of the old text with `start..new_end` of the new.
struct Edit {
    start: usize,
    old_end: usize,
    new_end: usize,
}

impl Edit {
    /// Whether the edit changed text inside `range`.
    fn overlaps(&self, range: &Range<usize>) -> bool {
        range.start < self.old_end && range.end > self.start
    }

    /// Whether the edit changed text inside or right next to `range`.
    fn touches(&self, range: &Range<usize>) -> bool {
        range.start <= self.old_end && range.end >= self.start
    }

    /// Where `range` of the old text is in the new one, stretched over the
    /// edit if it covers part of it.
    fn map(&self, range: &Range<usize>) -> Range<usize> {
        self.position(range.start, self.start)..self.position(range.end, self.new_end)
    }

    fn position(&self, at: usize, inside: usize) -> usize {
        if at <= self.start {
            at
        } else if at >= self.old_end {
            at - self.old_end + self.new_end
        } else {
            inside
        }
    }
}

/// Whether `unit` ends a paragraph.
fn breaks(unit: u16) -> bool {
    matches!(unit, 0x0A | 0x0D | 0x2028 | 0x2029)
}

/// Return the paragraph around `at` in `text`, without its line break.
pub fn paragraph(text: &[u16], at: usize) -> Range<usize> {
    let at = at.min(text.len());
    let start = text[..at]
        .iter()
        .rposition(|u| breaks(*u))
        .map_or(0, |i| i + 1);
    let end = text[at..]
        .iter()
        .position(|u| breaks(*u))
        .map_or(text.len(), |i| at + i);
    start..end
}

/// Return the whole paragraphs `range` of `text` falls in.
fn paragraphs(text: &[u16], range: Range<usize>) -> Range<usize> {
    paragraph(text, range.start).start..paragraph(text, range.end).end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spelling::Kind;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn show(text: &[u16], range: Range<usize>) -> String {
        String::from_utf16_lossy(&text[range])
    }

    /// Run every pending check with a checker that flags words starting with "x".
    fn check_all(doc: &mut Document) {
        let mut id = 100;
        while let Some(request) = doc.next(id) {
            let mut found = Vec::new();
            let mut start = None;
            let sentinel = u16::from(b' ');
            for (i, unit) in request.text.iter().chain([&sentinel]).enumerate() {
                let space = !char::from_u32(u32::from(*unit)).is_some_and(char::is_alphanumeric);
                match (start, space) {
                    (None, false) => start = Some(i),
                    (Some(s), true) => {
                        if request.text[s] == u16::from(b'x') {
                            found.push(Mistake {
                                range: s..i,
                                kind: Kind::Spelling,
                            });
                        }
                        start = None;
                    }
                    _ => {}
                }
            }
            doc.finish(id, found);
            id += 1;
        }
    }

    fn marked(doc: &Document) -> Vec<String> {
        doc.mistakes()
            .iter()
            .map(|m| show(doc.text(), m.range.clone()))
            .collect()
    }

    #[test]
    fn edits_recheck_only_their_paragraphs_and_keep_the_rest_in_place() {
        let mut doc = Document::new(units("a xone\nb xtwo\nc"));
        check_all(&mut doc);
        assert_eq!(marked(&doc), ["xone", "xtwo"]);

        // Typing in the first paragraph re-checks just that one.
        let start = doc.edit(&units("a xone xnew\nb xtwo\nc"));
        assert_eq!(start, Some(0));
        assert_eq!(marked(&doc), ["xone", "xtwo"]);
        let request = doc.next(1).unwrap();
        assert_eq!(String::from_utf16_lossy(&request.text), "a xone xnew");
        doc.finish(1, Vec::new());
        assert_eq!(marked(&doc), ["xtwo"]);

        // An edit inside a marked word drops it at once.
        doc.edit(&units("a xone xnew\nb xtwXo\nc"));
        assert_eq!(marked(&doc), Vec::<String>::new());
        check_all(&mut doc);
        assert_eq!(marked(&doc), ["xtwXo"]);
        assert_eq!(doc.edit(&units("a xone xnew\nb xtwXo\nc")), None);
    }

    #[test]
    fn an_edit_during_a_check_makes_it_stale_and_checks_again() {
        let mut doc = Document::new(units("xa\nxb"));
        let request = doc.next(1).unwrap();
        assert_eq!(request.text.len(), 5);
        doc.edit(&units("xa\nxbc"));
        assert!(doc.next(2).is_none(), "one check at a time");
        assert_eq!(doc.finish(1, Vec::new()), None);
        check_all(&mut doc);
        assert_eq!(marked(&doc), ["xa", "xbc"]);
    }

    #[test]
    fn long_texts_are_checked_in_chunks_of_whole_paragraphs() {
        let line = format!("{}\n", "y".repeat(999));
        let mut doc = Document::new(units(&line.repeat(5)));
        let mut lengths = Vec::new();
        for id in 0.. {
            let Some(request) = doc.next(id) else { break };
            assert_eq!(request.text.last(), Some(&u16::from(b'\n')));
            lengths.push(request.text.len());
            doc.finish(id, Vec::new());
        }
        assert_eq!(lengths, [2000, 2000, 1000]);
    }

    #[test]
    fn finds_the_paragraph_around_a_position() {
        let text = units("Hi.\nThey was here.\r\nBye");
        let cases = [
            (0, "Hi."),
            (9, "They was here."),
            (4, "They was here."),
            (21, "Bye"),
        ];
        for (at, want) in cases {
            assert_eq!(show(&text, paragraph(&text, at)), want, "at {at}");
        }
    }
}
