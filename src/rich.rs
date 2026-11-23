use std::ops::Range;
use std::time::Duration;

use serde_json::Value;
use similar::{DiffTag, TextDiff};

use crate::diff;
use crate::mac::{self, Clip, PLAIN};

const HTML: &str = "public.html";
const RTF: &str = "public.rtf";
/// Chrome's type for the types a web page copies itself, pickled together.
const WEB_CUSTOM: &str = "org.chromium.web-custom-data";
/// Slack's copy of its message box's text: a Quill delta.
const TEXTY: &str = "slack/texty";
const WEB_PLAIN: &str = "text/plain";
const HIDDEN: [&str; 4] = ["head", "script", "style", "title"];
/// Bound rich/plain alignment work; a coarser match may leave more edits out.
const ALIGN_TIMEOUT: Duration = Duration::from_millis(200);

/// Drop formats that can't take the same edits: they still hold the old text.
pub fn replacement(clip: &Clip, old: &str, new: &str) -> Clip {
    let edits = diff::edits(&diff::words(old, new));
    let edit = |texts: &mut [String]| apply(texts, old, &edits);
    let mut out = Clip::plain(new);
    for (kind, data) in &clip.0 {
        let changed = match kind.as_str() {
            HTML => html(data, edit),
            RTF => mac::edit_rtf(data, edit),
            WEB_CUSTOM => web_custom(data, new, edit),
            _ => None,
        };
        if let Some(data) = changed {
            out.0.push((kind.clone(), data));
        }
    }
    out
}

/// Reject all edits if any falls outside text shared by the plain and rich copies.
fn apply(texts: &mut [String], old: &str, edits: &[(Range<usize>, String)]) -> bool {
    let joined = texts.concat();
    let diff = TextDiff::configure()
        .timeout(ALIGN_TIMEOUT)
        .diff_chars(old, joined.as_str());
    let same: Vec<(Range<usize>, usize)> = diff
        .ops()
        .iter()
        .filter_map(|op| {
            let (tag, old, new) = op.as_tag_tuple();
            (tag == DiffTag::Equal).then_some((old, new.start))
        })
        .collect();
    let mut moved = Vec::new();
    for (range, text) in edits {
        let Some((old, new)) = same
            .iter()
            .find(|(old, _)| old.start <= range.start && range.end <= old.end)
        else {
            return false;
        };
        let start = new + range.start - old.start;
        moved.push((start..start + range.len(), text));
    }
    for (range, text) in moved {
        splice(texts, range, text);
    }
    true
}

/// Replacement text inherits the first replaced character's style; insertions use the preceding one.
fn splice(texts: &mut [String], range: Range<usize>, text: &str) {
    let owner_char = if range.is_empty() {
        range.start.checked_sub(1)
    } else {
        Some(range.start)
    };
    let mut owner = None;
    let mut start = 0;
    for (i, piece) in texts.iter_mut().enumerate() {
        let end = start + piece.chars().count();
        if owner.is_none() && start < end && owner_char.is_none_or(|c| start <= c && c < end) {
            owner = Some((i, range.start - start));
        }
        let cut = range.start.max(start)..range.end.min(end);
        if !cut.is_empty() {
            let bytes = byte(piece, cut.start - start)..byte(piece, cut.end - start);
            piece.replace_range(bytes, "");
        }
        start = end;
    }
    if let Some((i, at)) = owner {
        let at = byte(&texts[i], at);
        texts[i].insert_str(at, text);
    }
}

fn byte(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map_or(text.len(), |(i, _)| i)
}

fn html(data: &[u8], edit: impl FnOnce(&mut [String]) -> bool) -> Option<Vec<u8>> {
    let source = std::str::from_utf8(data).ok()?;
    let nodes = text_nodes(source);
    let mut texts: Vec<String> = nodes
        .iter()
        .map(|r| html_escape::decode_html_entities(&source[r.clone()]).into_owned())
        .collect();
    if !edit(&mut texts) {
        return None;
    }
    let mut out = String::with_capacity(source.len());
    let mut at = 0;
    for (range, text) in nodes.iter().zip(&texts) {
        out.push_str(&source[at..range.start]);
        if text.as_str() == html_escape::decode_html_entities(&source[range.clone()]) {
            out.push_str(&source[range.clone()]);
        } else {
            out.push_str(&html_escape::encode_text(text));
        }
        at = range.end;
    }
    out.push_str(&source[at..]);
    Some(out.into_bytes())
}

fn text_nodes(html: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut hidden: Option<String> = None;
    let mut at = 0;
    while at < html.len() {
        let rest = &html[at..];
        if rest.starts_with("<!--") {
            at += rest.find("-->").map_or(rest.len(), |i| i + 3);
        } else if let Some(tag) = rest.strip_prefix('<') {
            let end = tag.find('>').map_or(tag.len(), |i| i + 1);
            let closing = tag.starts_with('/');
            let name = tag
                .trim_start_matches('/')
                .split(|c: char| !c.is_ascii_alphanumeric())
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            match &hidden {
                Some(element) if closing && *element == name => hidden = None,
                None if !closing && HIDDEN.contains(&name.as_str()) => hidden = Some(name),
                _ => {}
            }
            at += 1 + end;
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            if hidden.is_none() {
                out.push(at..at + end);
            }
            at += end;
        }
    }
    out
}

fn web_custom(data: &[u8], new: &str, edit: impl FnOnce(&mut [String]) -> bool) -> Option<Vec<u8>> {
    let mut types = unpickle(data)?;
    let (_, texty) = types.iter_mut().find(|(kind, _)| kind == TEXTY)?;
    *texty = edit_texty(texty, edit)?;
    types.retain_mut(|(kind, value)| match kind.as_str() {
        TEXTY => true,
        PLAIN | WEB_PLAIN => {
            new.clone_into(value);
            true
        }
        _ => false,
    });
    pickle(&types)
}

fn edit_texty(texty: &str, edit: impl FnOnce(&mut [String]) -> bool) -> Option<String> {
    let mut delta: Value = serde_json::from_str(texty).ok()?;
    let ops = delta.get_mut("ops")?.as_array_mut()?;
    // Taken out, leaving empty strings that mark where each goes back.
    let mut texts: Vec<String> = ops
        .iter_mut()
        .filter_map(|op| match op.get_mut("insert")? {
            Value::String(text) => Some(std::mem::take(text)),
            _ => None,
        })
        .collect();
    if !edit(&mut texts) {
        return None;
    }
    let mut texts = texts.into_iter();
    for op in ops.iter_mut() {
        if op["insert"].is_string() {
            op["insert"] = texts.next()?.into();
        }
    }
    ops.retain(|op| op["insert"].as_str() != Some(""));
    Some(delta.to_string())
}

/// Chrome pickle: payload size, type count, then UTF-16 key/value strings.
/// Integers are little-endian u32; strings count code units and pad to 4-byte boundaries.
fn unpickle(data: &[u8]) -> Option<Vec<(String, String)>> {
    let mut pickle = Unpickler { data, at: 4 };
    let count = pickle.number()?;
    (0..count)
        .map(|_| Some((pickle.string()?, pickle.string()?)))
        .collect()
}

struct Unpickler<'a> {
    data: &'a [u8],
    at: usize,
}

impl Unpickler<'_> {
    fn take(&mut self, len: usize) -> Option<&[u8]> {
        let bytes = self.data.get(self.at..self.at + len)?;
        self.at += len.next_multiple_of(4);
        Some(bytes)
    }

    fn number(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn string(&mut self) -> Option<String> {
        let len = usize::try_from(self.number()?).ok()?;
        let units: Vec<u16> = self
            .take(len * 2)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16(&units).ok()
    }
}

fn pickle(types: &[(String, String)]) -> Option<Vec<u8>> {
    let mut body = u32::try_from(types.len()).ok()?.to_le_bytes().to_vec();
    for text in types.iter().flat_map(|(kind, value)| [kind, value]) {
        let units: Vec<u16> = text.encode_utf16().collect();
        body.extend(u32::try_from(units.len()).ok()?.to_le_bytes());
        body.extend(units.iter().flat_map(|u| u.to_le_bytes()));
        body.resize(body.len().next_multiple_of(4), 0);
    }
    let mut out = u32::try_from(body.len()).ok()?.to_le_bytes().to_vec();
    out.extend(body);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "Ths is a bold wrod and a link here.";
    const NEW: &str = "This is a bold word and a link here.";

    fn clip(types: &[(&str, Vec<u8>)]) -> Clip {
        Clip(
            types
                .iter()
                .map(|(kind, data)| ((*kind).to_owned(), data.clone()))
                .collect(),
        )
    }

    fn get<'a>(clip: &'a Clip, kind: &str) -> Option<&'a [u8]> {
        clip.0.iter().find(|(k, _)| k == kind).map(|(_, d)| &d[..])
    }

    #[test]
    fn html_keeps_its_tags_around_the_edits() {
        let cases = [
            (
                "<meta charset='utf-8'><p>Ths is a <b>bold wrod</b> and a <a href=\"x\">link</a> here.</p>",
                OLD,
                NEW,
                Some(
                    "<meta charset='utf-8'><p>This is a <b>bold word</b> and a <a href=\"x\">link</a> here.</p>",
                ),
            ),
            (
                "<p>Fish &amp; <i>chips</i> and peas<!-- note --></p>",
                "Fish & chips and peas",
                "Fish & chips, and peas.",
                Some("<p>Fish &amp; <i>chips,</i> and peas.<!-- note --></p>"),
            ),
            (
                "<head><title>Ths</title></head><p>Ths is</p>",
                "Ths is",
                "This is",
                Some("<head><title>Ths</title></head><p>This is</p>"),
            ),
            ("<p>Something else</p>", OLD, NEW, None),
        ];
        for (html, old, new, want) in cases {
            let pasted = replacement(
                &clip(&[(HTML, html.into()), ("com.example.private", vec![1])]),
                old,
                new,
            );
            assert_eq!(get(&pasted, PLAIN), Some(new.as_bytes()));
            assert_eq!(get(&pasted, "com.example.private"), None);
            assert_eq!(get(&pasted, HTML), want.map(str::as_bytes), "{html}");
        }
    }

    #[test]
    fn slacks_copy_keeps_its_styles_and_embeds() {
        let texty = r#"{"ops":[{"insert":"Ths is a "},{"attributes":{"bold":true},"insert":"bold wrod"},{"insert":{"emoji":"x"}},{"insert":" and a "},{"attributes":{"link":"x"},"insert":"link"},{"insert":" here."}]}"#;
        let types = [
            (PLAIN.to_owned(), OLD.to_owned()),
            (TEXTY.to_owned(), texty.to_owned()),
            ("other/type".to_owned(), OLD.to_owned()),
        ];
        let data = pickle(&types).unwrap();
        let pasted = replacement(&clip(&[(WEB_CUSTOM, data)]), OLD, NEW);
        let types = unpickle(get(&pasted, WEB_CUSTOM).unwrap()).unwrap();
        let want = r#"{"ops":[{"insert":"This is a "},{"attributes":{"bold":true},"insert":"bold word"},{"insert":{"emoji":"x"}},{"insert":" and a "},{"attributes":{"link":"x"},"insert":"link"},{"insert":" here."}]}"#;
        let want: Value = serde_json::from_str(want).unwrap();
        assert_eq!(types[0], (PLAIN.to_owned(), NEW.to_owned()));
        assert_eq!(types[1].0, TEXTY);
        assert_eq!(serde_json::from_str::<Value>(&types[1].1).unwrap(), want);
        assert_eq!(types.len(), 2);
    }

    #[test]
    fn rtf_keeps_each_runs_style() {
        let rtf = br"{\rtf1\ansi{\fonttbl\f0 Helvetica;}\f0 Ths is a {\b bold wrod} here.}";
        let pasted = replacement(
            &clip(&[(RTF, rtf.to_vec())]),
            "Ths is a bold wrod here.",
            "This is a bold word here.",
        );
        let mut runs = Vec::new();
        mac::edit_rtf(get(&pasted, RTF).unwrap(), |texts| {
            runs = texts.to_vec();
            false
        });
        assert_eq!(runs, ["This is a ", "bold word", " here."]);
    }
}
