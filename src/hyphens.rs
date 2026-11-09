//! Hyphens in model answers: models put dashes where a text had hyphens,
//! even when told not to.

/// Dashes models put in place of a hyphen, often dropping its spaces too.
const DASHES: [char; 2] = ['—', '–'];

/// Replace the [`DASHES`] in `text` that `original` lacks with spaced
/// hyphens, or bare ones between digits, as in ranges.
pub fn restore(original: &str, text: &str) -> String {
    let dashes: Vec<char> = DASHES
        .into_iter()
        .filter(|d| !original.contains(*d))
        .collect();
    if !text.contains(dashes.as_slice()) {
        return text.to_owned();
    }
    let blank = |c: char| c == ' ' || c == '\t' || c == '\u{a0}';
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if !dashes.contains(&c) {
            out.push(c);
            continue;
        }
        let range = out.ends_with(|c: char| c.is_ascii_digit())
            && chars.peek().is_some_and(char::is_ascii_digit);
        if range {
            out.push('-');
            continue;
        }
        out.truncate(out.trim_end_matches(blank).len());
        if !out.is_empty() && !out.ends_with('\n') {
            out.push(' ');
        }
        out.push('-');
        while chars.next_if(|&c| blank(c)).is_some() {}
        if chars.peek().is_some_and(|&c| c != '\n') {
            out.push(' ');
        }
    }
    out
}
