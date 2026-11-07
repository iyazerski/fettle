//! The word diff between the selected text and its proofread version, as
//! Grammarly shows it: removed words, then the words that replace them.

use similar::{ChangeTag, TextDiff};

/// Split `text` into words (letters, digits and inner apostrophes), runs of
/// whitespace, and single other characters, so punctuation diffs on its own.
fn tokens(text: &str) -> Vec<&str> {
    let class = |c: char| {
        if c.is_alphanumeric() || c == '\'' || c == '’' {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut last = None;
    for (i, c) in text.char_indices() {
        let kind = class(c);
        if i > start && (last != Some(kind) || kind == 2) {
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
    let diff = TextDiff::configure().diff_slices(&old, &new);
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

/// Whether the diff changes anything.
pub fn changed(pieces: &[Piece]) -> bool {
    pieces.iter().any(|p| p.kind != Kind::Same)
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
            ),
            (
                "We discussed about it on the meeting.",
                "We discussed it at the meeting.",
                "We discussed [-about -]it [-on-]{+at+} the meeting.",
            ),
            (
                "They was very tasty and I don't",
                "They were very tasty, and I don't",
                "They [-was-]{+were+} very tasty{+,+} and I don't",
            ),
            ("a b c d", "x y z d", "[-a b c-]{+x y z+} d"),
            ("Fine as is.\n", "Fine as is.\n", "Fine as is.\n"),
        ];
        for (old, new, want) in cases {
            let pieces = words(old, new);
            assert_eq!(show(&pieces), want, "{old:?} -> {new:?}");
            assert_eq!(side(&pieces, Kind::Added), old);
            assert_eq!(side(&pieces, Kind::Removed), new);
            assert_eq!(changed(&pieces), old != new);
        }
    }
}
