use std::ops::Range;
use std::time::Duration;

use similar::{ChangeTag, TextDiff};
use unicode_segmentation::UnicodeSegmentation;

/// Bound expensive searches on long rewrites; fall back to a coarser diff.
const DIFF_TIMEOUT: Duration = Duration::from_secs(1);

/// Unicode boundaries separate punctuation from words, unlike whitespace splitting.
fn tokens(text: &str) -> Vec<&str> {
    text.split_word_bounds().collect()
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

/// Group changes separated only by whitespace into a single phrase replacement.
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

/// Number contiguous removed/added groups from zero; unchanged pieces have no number.
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

pub fn decline(pieces: Vec<Piece>, n: usize) -> Vec<Piece> {
    let changes: Vec<Option<usize>> = numbered(&pieces).map(|(change, _)| change).collect();
    let mut out: Vec<Piece> = Vec::with_capacity(pieces.len());
    for (change, mut piece) in changes.into_iter().zip(pieces) {
        if change == Some(n) {
            if piece.kind == Kind::Added {
                continue;
            }
            piece.kind = Kind::Same;
        }
        match out.last_mut() {
            Some(last) if last.kind == piece.kind => last.text.push_str(&piece.text),
            _ => out.push(piece),
        }
    }
    out
}

pub fn result(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .filter(|p| p.kind != Kind::Removed)
        .map(|p| p.text.as_str())
        .collect()
}

/// Character-indexed edits, last first, so applying one doesn't shift the remaining ranges.
pub fn edits(pieces: &[Piece]) -> Vec<(Range<usize>, String)> {
    let mut out: Vec<(Range<usize>, String)> = Vec::new();
    let mut at = 0;
    let mut previous = Kind::Same;
    for piece in pieces {
        if piece.kind != Kind::Same && previous == Kind::Same {
            out.push((at..at, String::new()));
        }
        match (piece.kind, out.last_mut()) {
            (Kind::Same, _) => at += piece.text.chars().count(),
            (Kind::Removed, Some((range, _))) => {
                at += piece.text.chars().count();
                range.end = at;
            }
            (Kind::Added, Some((_, text))) => text.push_str(&piece.text),
            (Kind::Removed | Kind::Added, None) => unreachable!("a change starts an edit"),
        }
        previous = piece.kind;
    }
    out.reverse();
    out
}

pub fn changed(pieces: &[Piece]) -> bool {
    pieces.iter().any(|p| p.kind != Kind::Same)
}

/// Sorted, merged UTF-16 marks in the old text. Insertions mark the adjoining word:
/// the preceding word for a comma after it, or the following word for an insertion before it.
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
            (
                "我喜欢苹果",
                "我喜欢香蕉",
                "我喜欢[-苹果-]{+香蕉+}",
                &["苹果"],
            ),
        ];
        for (old, new, want, marked) in cases {
            let pieces = words(old, new);
            assert_eq!(show(&pieces), want, "{old:?} -> {new:?}");
            assert_eq!(side(&pieces, Kind::Added), old);
            assert_eq!(side(&pieces, Kind::Removed), new);
            assert_eq!(changed(&pieces), old != new);
            let mut edited: Vec<char> = old.chars().collect();
            for (range, text) in edits(&pieces) {
                edited.splice(range, text.chars());
            }
            assert_eq!(edited.into_iter().collect::<String>(), new);
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
            let declined = decline(pieces.clone(), n);
            assert_eq!(show(&declined), want, "change {n}");
            assert_eq!(result(&declined), text);
            assert_eq!(side(&declined, Kind::Added), old);
        }
        let none = decline(decline(pieces, 0), 0);
        assert_eq!(result(&none), old);
        assert!(!changed(&none));
    }
}
