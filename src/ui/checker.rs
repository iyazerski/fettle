//! Underlining mistakes as the user types in other apps: read the focused
//! text field whenever the app reports a change, and on a timer for apps that
//! don't; once the user pauses, ask the model to fix each paragraph they
//! edited, as the Fix action does; and underline what its fix changes. A click
//! on an underline opens the paragraph in the window with that fix.

use std::collections::HashMap;
use std::ops::Range;
use std::time::{Duration, Instant};

use objc2_core_foundation::CGRect;
use rune::AppHandle;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use super::pending::Pending;
use super::state::State;
use crate::ax::{Apps, Field, Focus, Runs, Watch};
use crate::diff::{self, Piece};
use crate::document::{Document, paragraph};
use crate::underlines::{Clicks, Line, Underlines};

/// How often the focused field is read even if its app reports nothing, to
/// catch scrolling and apps that don't report changes.
const TICK: Duration = Duration::from_millis(300);
/// How long the user must stop typing before an edited paragraph goes to the
/// model, so a request covers a finished thought rather than each keystroke.
const IDLE: Duration = Duration::from_millis(1200);
/// Longer paragraphs, in UTF-16 units, aren't sent: each edit would resend them.
const MAX_PARAGRAPH: usize = 4000;
/// How many paragraphs' fixes are kept, so text seen before isn't sent again.
const MAX_FIXES: usize = 256;
/// Longer texts, in UTF-16 units, aren't checked: reading them on every
/// change would cost too much.
const MAX_TEXT: usize = 100_000;
/// At most this many words are underlined, as each costs a call to the app.
const MAX_LINES: usize = 40;

#[derive(Default)]
pub struct Checker {
    running: Option<Running>,
    apps: Apps,
    underlines: Underlines,
    /// Reused for each read of the field's text.
    buffer: Vec<u16>,
    /// The last check's number, unique across fields, so an answer about an
    /// earlier field can't land in a later one.
    checks: u64,
    /// The model's fix of each paragraph it was asked about, by its text.
    fixes: HashMap<Vec<u16>, Fix>,
}

/// The model's fix of a paragraph, as the Fix action shows it.
#[derive(Debug, Clone)]
pub struct Fix {
    pub text: String,
    /// The word diff from the paragraph to `text`.
    pub changes: Vec<Piece>,
}

/// A paragraph to ask the model to fix, as check `id`.
#[derive(Debug)]
pub struct Check {
    pub id: u64,
    /// The paragraph as the field holds it, which its fix is kept by.
    pub units: Vec<u16>,
    pub text: String,
}

/// What runs while underlining is on.
struct Running {
    app: AppHandle<State>,
    rt: Handle,
    ticker: JoinHandle<()>,
    /// Reads the field once the user pauses, so the request doesn't wait for a tick.
    wake: Option<JoinHandle<()>>,
    _clicks: Option<Clicks>,
    /// The watch on the app in front, with its pid.
    watch: Option<(i32, Watch)>,
    /// The field being underlined, if any.
    session: Option<Session>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.ticker.abort();
        if let Some(wake) = &self.wake {
            wake.abort();
        }
    }
}

impl Running {
    /// Read the field again once [`IDLE`] passes with no newer edit.
    fn wake_after_idle(&mut self) {
        if let Some(wake) = self.wake.take() {
            wake.abort();
        }
        let app = self.app.clone();
        self.wake = Some(self.rt.spawn(async move {
            tokio::time::sleep(IDLE).await;
            check_field(&app);
        }));
    }
}

/// Read the focused field on the UI thread.
fn check_field(app: &AppHandle<State>) {
    app.update_if(|s, _| {
        s.check_field();
        false
    });
}

/// The field being underlined.
struct Session {
    field: Field,
    doc: Document,
    edited: Option<Instant>,
    layout: Layout,
    /// The model request for check `.0`; dropping the session stops it.
    request: Option<(u64, Pending)>,
}

/// Where words showed, while the field kept its place, scroll and text
/// before them, so a redraw asks the app only about new ones.
#[derive(Default)]
struct Layout {
    frame: Option<CGRect>,
    visible: Option<Range<usize>>,
    /// Each word asked about, and its bounds unless it doesn't show.
    words: Vec<(Range<usize>, Option<CGRect>)>,
    /// The field's runs, found the first time the field can't place a word.
    runs: Option<Runs>,
}

/// A paragraph in another app's text field, to fix and paste back over.
pub struct Target {
    field: Field,
    range: Range<usize>,
    units: Vec<u16>,
    pub text: String,
    /// The underlined word clicked, in points from the primary screen's top-left.
    pub word: CGRect,
}

impl Checker {
    /// Start reading the focused field and watching clicks, unless already started.
    pub fn start(&mut self, app: &AppHandle<State>, rt: &Handle) {
        if self.running.is_some() {
            return;
        }
        let ticks = app.clone();
        let ticker = rt.spawn(async move {
            let mut every = tokio::time::interval(TICK);
            every.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                every.tick().await;
                check_field(&ticks);
            }
        });
        let clicks = app.clone();
        self.running = Some(Running {
            app: app.clone(),
            rt: rt.clone(),
            ticker,
            wake: None,
            _clicks: Clicks::watch(move |at| clicks.update(move |s, _| s.underline_clicked(at))),
            watch: None,
            session: None,
        });
    }

    pub fn stop(&mut self) {
        self.running = None;
        self.underlines.hide();
    }

    /// Hide the underlines until the next `refresh`.
    pub fn pause(&mut self) {
        self.underlines.hide();
        if let Some(running) = &mut self.running {
            running.session = None;
        }
    }

    /// Read the focused field, underline the mistakes known, and return the
    /// paragraph to ask the model about next, if it's time to.
    pub fn refresh(&mut self) -> Option<Check> {
        let running = self.running.as_mut()?;
        let Some(Focus { pid, field }) = self.apps.focused() else {
            self.pause();
            return None;
        };
        if running
            .watch
            .as_ref()
            .is_none_or(|(watched, _)| *watched != pid)
        {
            let app = running.app.clone();
            let watch = Watch::new(pid, move || check_field(&app));
            running.watch = watch.map(|w| (pid, w));
        }
        let same = running.session.as_ref().is_some_and(|s| s.field == field);
        if (!same && field.is_secure()) || !field.read(MAX_TEXT, &mut self.buffer) {
            self.pause();
            return None;
        }
        match &mut running.session {
            Some(session) if same => {
                if let Some(start) = session.doc.edit(&self.buffer) {
                    session.edited = Some(Instant::now());
                    // Text from the edited paragraph on may have moved.
                    session.layout.words.retain(|(word, _)| word.end <= start);
                    session.layout.runs = None;
                    running.wake_after_idle();
                }
            }
            _ => {
                let fixes = &self.fixes;
                let doc = Document::new(self.buffer.clone(), |paragraph| {
                    fixes.get(paragraph).map(|f| diff::marks(&f.changes))
                });
                running.session = Some(Session {
                    field,
                    doc,
                    edited: None,
                    layout: Layout::default(),
                    request: None,
                });
            }
        }
        let check = self.request();
        self.draw();
        check
    }

    /// Keep `request`, which asks the model about check `id`, until it
    /// finishes or the field's session ends.
    pub fn asking(&mut self, id: u64, request: Pending) {
        if let Some(session) = self.session_mut() {
            session.request = Some((id, request));
        }
    }

    /// Take the model's `fix` of paragraph `units`, asked as check `id`, or
    /// `None` if the request failed, and return the paragraph to ask about next.
    pub fn finished(&mut self, id: u64, units: Vec<u16>, fix: Option<Fix>) -> Option<Check> {
        let found = fix.as_ref().map(|f| diff::marks(&f.changes));
        if let Some(fix) = fix {
            self.remember(units, fix);
        }
        let session = self.session_mut()?;
        if session
            .request
            .as_ref()
            .is_some_and(|(asked, _)| *asked == id)
        {
            session.request = None;
        }
        session.doc.finish(id, found);
        let check = self.request();
        self.draw();
        check
    }

    /// Leave the field's edits unchecked: there's no model to ask.
    pub fn skip(&mut self) {
        if let Some(session) = self.session_mut() {
            session.request = None;
            session.doc.skip();
        }
    }

    /// Forget the fixes known: they came from another model.
    pub fn forget(&mut self) {
        self.fixes.clear();
    }

    /// Keep the model's `fix` of `target`'s paragraph, so it isn't sent again.
    pub fn learn(&mut self, target: &Target, fix: Fix) {
        self.remember(target.units.clone(), fix);
    }

    fn remember(&mut self, paragraph: Vec<u16>, fix: Fix) {
        if self.fixes.len() >= MAX_FIXES {
            self.fixes.clear();
        }
        self.fixes.insert(paragraph, fix);
    }

    fn session_mut(&mut self) -> Option<&mut Session> {
        self.running.as_mut()?.session.as_mut()
    }

    /// Return the paragraph with the underlined mistake at `point`, in
    /// AppKit's coordinates, and the model's fix of it if known.
    pub fn clicked(&self, point: (f64, f64)) -> Option<(Target, Option<Fix>)> {
        let line = self.underlines.hit(point)?;
        let session = self.running.as_ref()?.session.as_ref()?;
        let text = session.doc.text();
        let range = paragraph(text, line.at);
        let units = text.get(range.clone())?.to_vec();
        let fix = self.fixes.get(&units).cloned();
        let target = Target {
            field: session.field.clone(),
            text: String::from_utf16_lossy(&units),
            word: line.word,
            units,
            range,
        };
        Some((target, fix))
    }

    /// Return the next edited paragraph to ask the model about, once the
    /// user stopped typing and no request is running. Paragraphs it fixed
    /// before take their known fix, and long ones are skipped.
    fn request(&mut self) -> Option<Check> {
        let session = self.running.as_mut()?.session.as_mut()?;
        if session.edited.is_some_and(|at| at.elapsed() < IDLE) {
            return None;
        }
        loop {
            let request = session.doc.next(self.checks + 1)?;
            self.checks = request.id;
            if request.text.len() > MAX_PARAGRAPH {
                session.doc.finish(request.id, None);
            } else if let Some(fix) = self.fixes.get(&request.text) {
                session
                    .doc
                    .finish(request.id, Some(diff::marks(&fix.changes)));
            } else {
                return Some(Check {
                    id: request.id,
                    text: String::from_utf16_lossy(&request.text),
                    units: request.text,
                });
            }
        }
    }

    /// Underline the known mistakes that show, asking the app where only for
    /// words whose place may have changed.
    fn draw(&mut self) {
        let Some(session) = self.running.as_mut().and_then(|r| r.session.as_mut()) else {
            return;
        };
        let field = &session.field;
        let (frame, visible) = (field.frame(), field.visible());
        let layout = &mut session.layout;
        if layout.frame != frame || layout.visible != visible {
            *layout = Layout {
                frame,
                visible,
                ..Layout::default()
            };
        }
        let typing = session.edited.is_some_and(|at| at.elapsed() < IDLE);
        let caret = typing
            .then(|| field.selection())
            .flatten()
            .filter(Range::is_empty)
            .map(|s| s.start);
        let text = session.doc.text();
        let mistakes = session.doc.mistakes();
        let (from, to) = match &layout.visible {
            Some(visible) => (
                mistakes.partition_point(|m| m.end <= visible.start),
                mistakes.partition_point(|m| m.start < visible.end),
            ),
            None => (0, mistakes.len()),
        };
        let mut lines = Vec::new();
        let mut asked = Vec::new();
        'mistakes: for range in &mistakes[from..to.max(from)] {
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
                    None => field
                        .bounds(word.clone())
                        .or_else(|| {
                            let runs = layout.runs.get_or_insert_with(|| field.runs(text));
                            runs.bounds(word.clone())
                        })
                        .filter(|r| inside(*r, frame)),
                };
                asked.push((word, rect));
                if let Some(rect) = rect {
                    lines.push(Line {
                        word: rect,
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
