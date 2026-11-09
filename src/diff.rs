//! The word diff between the selected text and its fixed version, as
//! Grammarly shows it: removed words, then the words that replace them.

use std::ops::Range;
use std::time::Duration;

use similar::{ChangeTag, TextDiff};

/// How long the diff may search for the smallest edit before it settles for
/// a coarser one: a long, heavily rewritten text could take far longer.
const DIFF_TIMEOUT: Duration = Duration::from_secs(1);

/// Split `text` into words (letters, digits and inner apostrophes), runs of
/// whitespace, and single other characters, so punctuation diffs on its own.
fn tokens(text: &str) -> Vec<&str> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Class {
        Word,
        Space,
        Other,
    }
    let class = |c: char| {
        if c.is_alphanumeric() || c == '\'' || c == '’' {
            Class::Word
        } else if c.is_whitespace() {
            Class::Space
        } else {
            Class::Other
        }
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut last = None;
    for (i, c) in text.char_indices() {
        let kind = class(c);
        if i > start && (last != Some(kind) || kind == Class::Other) {
            out.push(&text[start..i]);
            start = i;
        }
        last = Some(kind);
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Same,
    Removed,
    Added,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub kind: Kind,
    pub text: String,
}

/// Diff `old` against `new` by words. Changes split only by spaces read as
/// one edit, so a rewritten phrase shows as the old phrase, then the new one,
/// rather than word against word.
pub fn words(old: &str, new: &str) -> Vec<Piece> {
    let (old, new) = (tokens(old), tokens(new));
    let diff = TextDiff::configure()
        .timeout(DIFF_TIMEOUT)
        .diff_slices(&old, &new);
    let tokens: Vec<(ChangeTag, &str)> = diff
        .iter_all_changes()
        .map(|c| (c.tag(), c.value()))
        .collect();
    let mut out: Vec<Piece> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let (tag, value) = tokens[i];
        if tag == ChangeTag::Equal {
            push(&mut out, Kind::Same, value);
            i += 1;
            continue;
        }
        let (mut removed, mut added) = (String::new(), String::new());
        while let Some(&(tag, value)) = tokens.get(i) {
            let bridges = tag == ChangeTag::Equal
                && value.trim().is_empty()
                && tokens
                    .get(i + 1)
                    .is_some_and(|(next, _)| *next != ChangeTag::Equal);
            match tag {
                ChangeTag::Delete => removed.push_str(value),
                ChangeTag::Insert => added.push_str(value),
                ChangeTag::Equal if bridges => {
                    removed.push_str(value);
                    added.push_str(value);
                }
                ChangeTag::Equal => break,
            }
            i += 1;
        }
        push(&mut out, Kind::Removed, &removed);
        push(&mut out, Kind::Added, &added);
    }
    out
}

/// Append `text` as `kind`, joining it to the last piece when that has the same kind.
fn push(out: &mut Vec<Piece>, kind: Kind, text: &str) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(last) if last.kind == kind => last.text.push_str(text),
        _ => out.push(Piece {
            kind,
            text: text.to_owned(),
        }),
    }
}

/// Pair each piece with the number of the change it's part of, counting
/// from 0, or `None` if it's the same on both sides. A change is a run of
/// removed and added pieces: the words removed and those replacing them.
pub fn numbered(pieces: &[Piece]) -> impl Iterator<Item = (Option<usize>, &Piece)> {
    let mut next = 0;
    let mut previous = Kind::Same;
    pieces.iter().map(move |piece| {
        let change = match piece.kind {
            Kind::Same => None,
            _ if previous == Kind::Same => {
                next += 1;
                Some(next - 1)
            }
            _ => Some(next - 1),
        };
        previous = piece.kind;
        (change, piece)
    })
}

/// Return the diff with change `n` (see [`numbered`]) undone: its removed
/// words kept and its added ones dropped.
pub fn decline(pieces: &[Piece], n: usize) -> Vec<Piece> {
    let mut out = Vec::new();
    for (change, piece) in numbered(pieces) {
        match piece.kind {
            Kind::Removed if change == Some(n) => push(&mut out, Kind::Same, &piece.text),
            Kind::Added if change == Some(n) => {}
            kind => push(&mut out, kind, &piece.text),
        }
    }
    out
}

/// Return the new text: what the diff changes the old one into.
pub fn result(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .filter(|p| p.kind != Kind::Removed)
        .map(|p| p.text.as_str())
        .collect()
}

/// Whether the diff changes anything.
pub fn changed(pieces: &[Piece]) -> bool {
    pieces.iter().any(|p| p.kind != Kind::Same)
}

/// Return what the diff changes in the old text, as UTF-16 ranges, sorted
/// and apart: removed words, and for a word added between two, the word it
/// joins (`tasty` for a comma after it) or else the word after it (`store`
/// for `the` before it).
pub fn marks(pieces: &[Piece]) -> Vec<Range<usize>> {
    let old: Vec<u16> = pieces
        .iter()
        .filter(|p| p.kind != Kind::Added)
        .flat_map(|p| p.text.encode_utf16())
        .collect();
    let word = |u: u16| char::from_u32(u32::from(u)).is_none_or(char::is_alphanumeric);
    let mut marks: Vec<Range<usize>> = Vec::new();
    let mut at = 0;
    let mut after_removed = false;
    for piece in pieces {
        let len = piece.text.encode_utf16().count();
        match piece.kind {
            Kind::Same => at += len,
            Kind::Removed => {
                let words = piece.text.trim();
                if !words.is_empty() {
                    let space = &piece.text[..piece.text.len() - piece.text.trim_start().len()];
                    let start = at + space.encode_utf16().count();
                    marks.push(start..start + words.encode_utf16().count());
                }
                at += len;
            }
            Kind::Added if after_removed => {}
            Kind::Added => {
                let joined = at > 0 && word(old[at - 1]);
                let (start, end) = if joined {
                    let start = old[..at]
                        .iter()
                        .rposition(|u| !word(*u))
                        .map_or(0, |i| i + 1);
                    (start, at)
                } else {
                    let end = old[at..]
                        .iter()
                        .position(|u| !word(*u))
                        .map_or(old.len(), |i| at + i);
                    (at, end)
                };
                if start < end {
                    marks.push(start..end);
                }
            }
        }
        after_removed = piece.kind == Kind::Removed;
    }
    marks.sort_by_key(|m| m.start);
    marks.dedup_by(|next, kept| {
        let meets = next.start <= kept.end;
        if meets {
            kept.end = kept.end.max(next.end);
        }
        meets
    });
    marks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(pieces: &[Piece], skip: Kind) -> String {
        pieces
            .iter()
            .filter(|p| p.kind != skip)
            .map(|p| p.text.as_str())
            .collect()
    }

    fn show(pieces: &[Piece]) -> String {
        pieces
            .iter()
            .map(|p| match p.kind {
                Kind::Same => p.text.clone(),
                Kind::Removed => format!("[-{}-]", p.text),
                Kind::Added => format!("{{+{}+}}", p.text),
            })
            .collect()
    }

    #[test]
    fn diffs_by_words_and_groups_phrases() {
        let cases = [
            (
                "I has went to store yesterday.",
                "I went to the store yesterday.",
                "I [-has -]went to {+the +}store yesterday.",
                &["has", "store"][..],
            ),
            (
                "We discussed about it on the meeting.",
                "We discussed it at the meeting.",
                "We discussed [-about -]it [-on-]{+at+} the meeting.",
                &["about", "on"],
            ),
            (
                "They was very tasty and I don't",
                "They were very tasty, and I don't",
                "They [-was-]{+were+} very tasty{+,+} and I don't",
                &["was", "tasty"],
            ),
            ("a b c d", "x y z d", "[-a b c-]{+x y z+} d", &["a b c"]),
            ("Fine as is.\n", "Fine as is.\n", "Fine as is.\n", &[]),
            ("Это хорошо", "Это хорошо.", "Это хорошо{+.+}", &["хорошо"]),
        ];
        for (old, new, want, marked) in cases {
            let pieces = words(old, new);
            assert_eq!(show(&pieces), want, "{old:?} -> {new:?}");
            assert_eq!(side(&pieces, Kind::Added), old);
            assert_eq!(side(&pieces, Kind::Removed), new);
            assert_eq!(changed(&pieces), old != new);
            let units: Vec<u16> = old.encode_utf16().collect();
            let got: Vec<String> = marks(&pieces)
                .into_iter()
                .map(|r| String::from_utf16_lossy(&units[r]))
                .collect();
            assert_eq!(got, marked, "{old:?} -> {new:?}");
        }
    }

    #[test]
    fn declining_a_change_keeps_the_old_words_there() {
        let old = "We discussed about it on the meeting.";
        let pieces = words(old, "We discussed it at the meeting.");
        let cases = [
            (
                0,
                "We discussed about it [-on-]{+at+} the meeting.",
                "We discussed about it at the meeting.",
            ),
            (
                1,
                "We discussed [-about -]it on the meeting.",
                "We discussed it on the meeting.",
            ),
        ];
        for (n, want, text) in cases {
            let declined = decline(&pieces, n);
            assert_eq!(show(&declined), want, "change {n}");
            assert_eq!(result(&declined), text);
            assert_eq!(side(&declined, Kind::Added), old);
        }
        let none = decline(&decline(&pieces, 0), 0);
        assert_eq!(result(&none), old);
        assert!(!changed(&none));
    }
}
