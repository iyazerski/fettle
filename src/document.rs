//! Check only edited paragraphs, one at a time; retain known mistakes in untouched text.
//! All ranges count UTF-16 code units, matching Accessibility.

use std::ops::Range;

#[derive(Debug, Default)]
pub struct Document {
    text: Vec<u16>,
    /// What's wrong, sorted and apart.
    mistakes: Vec<Range<usize>>,
    /// Spans still to check: sorted, apart, and whole paragraphs.
    unchecked: Vec<Range<usize>>,
    check: Check,
}

#[derive(Debug, Default)]
enum Check {
    #[default]
    Idle,
    Running {
        id: u64,
        span: Range<usize>,
        outdated: bool,
    },
}

#[derive(Debug)]
pub struct Request {
    pub id: u64,
    pub text: Vec<u16>,
}

impl Document {
    /// Treat the initial text as checked, restoring only mistakes already known. `known`
    /// returns ranges within the paragraph it's given.
    pub fn new(text: Vec<u16>, mut known: impl FnMut(&[u16]) -> Option<Vec<Range<usize>>>) -> Self {
        let mut mistakes = Vec::new();
        let mut start = 0;
        while start < text.len() {
            let span = paragraph(&text, start);
            if let Some(found) = known(&text[span.clone()]) {
                mistakes.extend(found.into_iter().map(|m| m.start + start..m.end + start));
            }
            start = span.end + 1;
        }
        Self {
            text,
            mistakes,
            ..Self::default()
        }
    }

    pub fn text(&self) -> &[u16] {
        &self.text
    }

    pub fn mistakes(&self) -> &[Range<usize>] {
        &self.mistakes
    }

    pub fn checking(&self) -> Option<u64> {
        match self.check {
            Check::Running { id, .. } => Some(id),
            Check::Idle => None,
        }
    }

    /// Return the start of the first affected paragraph; layout before it remains valid.
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
            let kept = !edit.overlaps(m);
            *m = edit.map(m);
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
        let span = paragraph(new, edit.start).start..paragraph(new, edit.new_end).end;
        let start = span.start;
        self.mark(span);
        self.text.clear();
        self.text.extend_from_slice(new);
        Some(start)
    }

    pub fn next(&mut self, id: u64) -> Option<Request> {
        if matches!(self.check, Check::Running { .. }) {
            return None;
        }
        while let Some(first) = self.unchecked.first().cloned() {
            let span = paragraph(&self.text, first.start);
            // Past the break, so the rest starts a paragraph.
            let rest = (span.end + 1).min(first.end)..first.end;
            if rest.is_empty() {
                self.unchecked.remove(0);
            } else {
                self.unchecked[0] = rest;
            }
            let text = &self.text[span.clone()];
            if char::decode_utf16(text.iter().copied()).any(|c| c.is_ok_and(char::is_alphabetic)) {
                let text = text.to_vec();
                self.check = Check::Running {
                    id,
                    span,
                    outdated: false,
                };
                return Some(Request { id, text });
            }
        }
        None
    }

    /// `found` holds ranges within the checked paragraph. Keep existing marks on failure
    /// (`None`), and ignore stale or unknown checks.
    pub fn finish(&mut self, id: u64, found: Option<Vec<Range<usize>>>) -> bool {
        let Check::Running {
            id: running,
            span,
            outdated,
        } = &self.check
        else {
            return false;
        };
        if *running != id {
            return false;
        }
        let (span, outdated) = (span.clone(), *outdated);
        self.check = Check::Idle;
        let Some(found) = found.filter(|_| !outdated) else {
            return false;
        };
        let from = self.mistakes.partition_point(|m| m.start < span.start);
        let to = self.mistakes.partition_point(|m| m.start < span.end);
        let found = found
            .into_iter()
            .map(|m| m.start + span.start..m.end + span.start);
        self.mistakes.splice(from..to, found);
        true
    }

    pub fn skip(&mut self) {
        self.unchecked.clear();
        self.check = Check::Idle;
    }

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
    fn overlaps(&self, range: &Range<usize>) -> bool {
        range.start < self.old_end && range.end > self.start
    }

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

fn breaks(unit: u16) -> bool {
    matches!(unit, 0x0A | 0x0D | 0x2028 | 0x2029)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn show(text: &[u16], range: Range<usize>) -> String {
        String::from_utf16_lossy(&text[range])
    }

    fn check_all(doc: &mut Document) -> Vec<String> {
        let mut checked = Vec::new();
        let mut id = 100;
        while let Some(request) = doc.next(id) {
            let text = String::from_utf16_lossy(&request.text);
            let mut found = Vec::new();
            let mut at = 0;
            for word in text.split(' ') {
                let len = word.encode_utf16().count();
                if word.starts_with('x') {
                    found.push(at..at + len);
                }
                at += len + 1;
            }
            checked.push(text);
            doc.finish(id, Some(found));
            id += 1;
        }
        checked
    }

    fn marked(doc: &Document) -> Vec<String> {
        doc.mistakes()
            .iter()
            .map(|m| show(doc.text(), m.clone()))
            .collect()
    }

    #[test]
    fn checks_only_edited_paragraphs_and_keeps_the_rest_in_place() {
        let known = |p: &[u16]| (p == units("c")).then(|| std::iter::once(0..1).collect());
        let mut doc = Document::new(units("a xold\nb xtwo\r\nc"), known);
        assert!(doc.next(1).is_none());
        assert_eq!(marked(&doc), ["c"]);

        doc.edit(&units("a xold\nb xtwo xmore\r\nc xnew\n\n42"));
        assert_eq!(check_all(&mut doc), ["b xtwo xmore", "c xnew"]);
        assert_eq!(marked(&doc), ["xtwo", "xmore", "xnew"]);

        assert_eq!(
            doc.edit(&units("a xold!\nb xtwo xmore\r\nc xnew\n\n42")),
            Some(0)
        );
        assert_eq!(marked(&doc), ["xtwo", "xmore", "xnew"]);
        assert_eq!(
            doc.edit(&units("a xold!\nb xtwo xmXore\r\nc xnew\n\n42")),
            Some(8)
        );
        assert_eq!(marked(&doc), ["xtwo", "xnew"]);
        assert_eq!(check_all(&mut doc), ["a xold!", "b xtwo xmXore"]);
        assert_eq!(marked(&doc), ["xold!", "xtwo", "xmXore", "xnew"]);
        assert_eq!(
            doc.edit(&units("a xold!\nb xtwo xmXore\r\nc xnew\n\n42")),
            None
        );
    }

    #[test]
    fn an_edit_during_a_check_makes_it_stale_and_checks_again() {
        let mut doc = Document::new(units("xa\nxb"), |_| None);
        doc.edit(&units("xa\nxbc"));
        let request = doc.next(1).unwrap();
        assert_eq!(String::from_utf16_lossy(&request.text), "xbc");
        doc.edit(&units("xa\nxbcd"));
        assert!(doc.next(2).is_none(), "one check at a time");
        assert!(!doc.finish(1, Some(Vec::new())));
        assert_eq!(check_all(&mut doc), ["xbcd"]);
        assert_eq!(marked(&doc), ["xbcd"]);
        doc.edit(&units("xa\nxbcde"));
        let request = doc.next(3).unwrap();
        assert!(!doc.finish(request.id, None));
        assert_eq!(marked(&doc), ["xbcd"]);
        doc.edit(&units("xa\nxbcdef"));
        let request = doc.next(4).unwrap();
        doc.skip();
        assert!(doc.next(5).is_none());
        assert!(!doc.finish(request.id, Some(Vec::new())));
        assert_eq!(marked(&doc), ["xbcd"]);
    }
}
