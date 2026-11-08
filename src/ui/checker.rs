//! Underlining mistakes as the user types in other apps: read the focused
//! text field whenever the app reports a change, and on a timer for apps that
//! don't; check the paragraphs that changed with macOS's spell and grammar
//! checker in the background; and underline what it finds. A click on an
//! underline opens the paragraph in the window to fix it with the model.

use std::ops::Range;
use std::time::{Duration, Instant};

use objc2_core_foundation::CGRect;
use rune::AppHandle;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use super::state::State;
use crate::ax::{Apps, Field, Focus, Watch};
use crate::document::{Document, paragraph};
use crate::spelling::{self, Mistake};
use crate::underlines::{Clicks, Line, Underlines};

/// How often the focused field is read even if its app reports nothing, to
/// catch scrolling and apps that don't report changes.
const TICK: Duration = Duration::from_millis(300);
/// For how long after an edit the word at the caret goes unmarked, as the
/// user may still be typing it.
const TYPING: Duration = Duration::from_millis(1500);
/// Longer texts, in UTF-16 units, aren't checked: reading them on every
/// change would cost too much.
const MAX_TEXT: usize = 100_000;
/// At most this many words are underlined, as each costs a call to the app.
const MAX_LINES: usize = 40;

#[derive(Default)]
pub struct Checker {
    running: Option<Running>,
    apps: Apps,
    session: Option<Session>,
    underlines: Underlines,
    /// Reused for each read of the field's text.
    buffer: Vec<u16>,
    /// The last check's number, unique across fields, so an answer about an
    /// earlier field can't land in a later one.
    checks: u64,
}

/// What runs while underlining is on.
struct Running {
    app: AppHandle<State>,
    ticker: JoinHandle<()>,
    _clicks: Option<Clicks>,
    /// The watch on the app in front, with its pid.
    watch: Option<(i32, Watch)>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.ticker.abort();
    }
}

/// The field being underlined.
struct Session {
    field: Field,
    doc: Document,
    edited: Option<Instant>,
    layout: Layout,
}

/// Where words showed, while the field kept its place, scroll and text
/// before them, so a redraw asks the app only about new ones.
#[derive(Default)]
struct Layout {
    frame: Option<CGRect>,
    visible: Option<Range<usize>>,
    /// Each word asked about, and its bounds unless it doesn't show.
    words: Vec<(Range<usize>, Option<CGRect>)>,
}

/// A paragraph in another app's text field, to fix and paste back over.
pub struct Target {
    field: Field,
    range: Range<usize>,
    units: Vec<u16>,
    pub text: String,
}

impl Checker {
    /// Start reading the focused field and watching clicks, unless already started.
    pub fn start(&mut self, app: &AppHandle<State>, rt: &Handle) {
        if self.running.is_some() {
            return;
        }
        // The checker's first answer takes about 200 ms; later ones, a few.
        spelling::request(&[u16::from(b'a')], |_| ());
        let ticks = app.clone();
        let ticker = rt.spawn(async move {
            let mut every = tokio::time::interval(TICK);
            every.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                every.tick().await;
                ticks.update_if(|s, _| {
                    s.check_field();
                    false
                });
            }
        });
        let clicks = app.clone();
        self.running = Some(Running {
            app: app.clone(),
            ticker,
            _clicks: Clicks::watch(move |at| clicks.update(move |s, _| s.underline_clicked(at))),
            watch: None,
        });
    }

    pub fn stop(&mut self) {
        self.running = None;
        self.pause();
    }

    /// Hide the underlines until the next `refresh`.
    pub fn pause(&mut self) {
        self.underlines.hide();
        self.session = None;
    }

    /// Read the focused field, check what changed, and underline the mistakes known.
    pub fn refresh(&mut self) {
        let Some(running) = &mut self.running else {
            return;
        };
        let Some(Focus { pid, field }) = self.apps.focused() else {
            self.pause();
            return;
        };
        if running
            .watch
            .as_ref()
            .is_none_or(|(watched, _)| *watched != pid)
        {
            let app = running.app.clone();
            let watch = Watch::new(pid, move || {
                app.update_if(|s, _| {
                    s.check_field();
                    false
                })
            });
            running.watch = watch.map(|w| (pid, w));
        }
        let same = self.session.as_ref().is_some_and(|s| s.field == field);
        if !same && field.is_secure() || !field.read(MAX_TEXT, &mut self.buffer) {
            self.pause();
            return;
        }
        match &mut self.session {
            Some(session) if same => {
                if let Some(start) = session.doc.edit(&self.buffer) {
                    session.edited = Some(Instant::now());
                    // Text from the edited paragraph on may have moved.
                    session.layout.words.retain(|(word, _)| word.end <= start);
                }
            }
            _ => {
                self.session = Some(Session {
                    field,
                    doc: Document::new(self.buffer.clone()),
                    edited: None,
                    layout: Layout::default(),
                });
            }
        }
        self.request();
        self.draw();
    }

    /// Take the mistakes check `id` found, and start the next check.
    pub fn finished(&mut self, id: u64, found: Vec<Mistake>) {
        let Some(session) = &mut self.session else {
            return;
        };
        if session.doc.finish(id, found).is_some() {
            self.draw();
        }
        self.request();
    }

    /// Return the paragraph with the underlined mistake at `point`, in AppKit's coordinates.
    pub fn clicked(&self, point: (f64, f64)) -> Option<Target> {
        let line = self.underlines.hit(point)?;
        let session = self.session.as_ref()?;
        let text = session.doc.text();
        let range = paragraph(text, line.at);
        let units = text.get(range.clone())?.to_vec();
        Some(Target {
            field: session.field.clone(),
            text: String::from_utf16_lossy(&units),
            units,
            range,
        })
    }

    /// Check the next unchecked span, if no check is running.
    fn request(&mut self) {
        let (Some(running), Some(session)) = (&self.running, &mut self.session) else {
            return;
        };
        let Some(request) = session.doc.next(self.checks + 1) else {
            return;
        };
        self.checks = request.id;
        let (app, id) = (running.app.clone(), request.id);
        spelling::request(&request.text, move |found| {
            app.update_if(move |s, _| {
                s.field_checked(id, found);
                false
            });
        });
    }

    /// Underline the known mistakes that show, asking the app where only for
    /// words whose place may have changed.
    fn draw(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        let field = &session.field;
        let (frame, visible) = (field.frame(), field.visible());
        let layout = &mut session.layout;
        if layout.frame != frame || layout.visible != visible {
            *layout = Layout {
                frame,
                visible,
                words: Vec::new(),
            };
        }
        let typing = session.edited.is_some_and(|at| at.elapsed() < TYPING);
        let caret = typing
            .then(|| field.selection())
            .flatten()
            .filter(Range::is_empty)
            .map(|s| s.start);
        let text = session.doc.text();
        let mistakes = session.doc.mistakes();
        let (from, to) = match &layout.visible {
            Some(visible) => (
                mistakes.partition_point(|m| m.range.end <= visible.start),
                mistakes.partition_point(|m| m.range.start < visible.end),
            ),
            None => (0, mistakes.len()),
        };
        let mut lines = Vec::new();
        let mut asked = Vec::new();
        'mistakes: for mistake in &mistakes[from..to.max(from)] {
            let range = &mistake.range;
            if caret.is_some_and(|c| range.start <= c && c <= range.end) {
                continue;
            }
            for word in words(text, range.clone()) {
                if lines.len() == MAX_LINES {
                    break 'mistakes;
                }
                let known = layout.words.iter().find(|(w, _)| *w == word);
                let rect = match known {
                    Some((_, rect)) => *rect,
                    // Text scrolled out of the field still has bounds; leave it out.
                    None => field.bounds(word.clone()).filter(|r| inside(*r, frame)),
                };
                asked.push((word, rect));
                if let Some(rect) = rect {
                    lines.push(Line {
                        word: rect,
                        kind: mistake.kind,
                        at: range.start,
                    });
                }
            }
        }
        layout.words = asked;
        self.underlines.show(lines);
    }
}

impl Target {
    /// Select the paragraph in its field, so a paste replaces it. Returns
    /// false, selecting nothing, if the field no longer holds it there.
    pub fn select(&self) -> bool {
        let mut text = Vec::new();
        self.field.read(MAX_TEXT, &mut text)
            && text.get(self.range.clone()) == Some(&self.units[..])
            && self.field.select(self.range.clone())
    }
}

/// Whether the middle of `rect` is inside `frame`, if known.
fn inside(rect: CGRect, frame: Option<CGRect>) -> bool {
    let (x, y) = (
        rect.origin.x + rect.size.width / 2.0,
        rect.origin.y + rect.size.height / 2.0,
    );
    frame.is_none_or(|f| {
        (f.origin.x..=f.origin.x + f.size.width).contains(&x)
            && (f.origin.y..=f.origin.y + f.size.height).contains(&y)
    })
}

/// Split `range` of `text` into its words, so a phrase over two lines gets an
/// underline on each rather than one box around both.
fn words(text: &[u16], range: Range<usize>) -> Vec<Range<usize>> {
    let space = |u: u16| char::from_u32(u32::from(u)).is_some_and(char::is_whitespace);
    let mut out = Vec::new();
    let mut start = None;
    for i in range.clone() {
        match (start, text.get(i).copied().is_some_and(space)) {
            (None, false) => start = Some(i),
            (Some(s), true) => {
                out.push(s..i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(s..range.end.min(text.len()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_mistake_into_words() {
        let text: Vec<u16> = "Hi.\nThey was here.".encode_utf16().collect();
        let words: Vec<String> = words(&text, 4..13)
            .into_iter()
            .map(|r| String::from_utf16_lossy(&text[r]))
            .collect();
        assert_eq!(words, ["They", "was"]);
    }
}
