//! The checker's own thread: everything that asks other apps through the
//! Accessibility API, which may wait on a slow app, runs here rather than on
//! the UI thread. It reads the focused field, keeps its [`Document`], works
//! out where the underlines go, and tells the window.

use std::cell::Cell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use objc2::rc::autoreleasepool;
use objc2_core_foundation::CGRect;
use rune::AppHandle;

use super::{Check, Fix, Target};
use crate::ax::{self, Apps, Field, Focus, Runs, Waker, Watch};
use crate::diff;
use crate::document::{Document, paragraph};
use crate::ui::pending::Pending;
use crate::ui::state::State;
use crate::underlines::Line;

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
/// At most this many words' places are asked for per redraw, found or not,
/// so a field that can't place its words can't keep the checker busy; the
/// rest are asked about on the next one.
const MAX_LOOKUPS: usize = 60;
/// How many fields left keep their edits still to check and their running
/// check, so a quick switch to another field or app loses neither.
const RECENT: usize = 8;

/// What the window asks of the checker.
pub(super) enum Command {
    /// `request` asks the model about check `id`.
    Asking(u64, Pending),
    /// The model's fix of paragraph `units`, asked as check `id`, or `None`
    /// if the request failed.
    Finished {
        id: u64,
        units: Vec<u16>,
        fix: Option<Fix>,
    },
    /// Leave the edits of the field that asked for check `id` unchecked:
    /// there's no model to ask.
    Skip(u64),
    /// Forget the fixes known: they came from another model.
    Forget,
    /// Keep `fix` of paragraph `units`, so it isn't sent again.
    Learn {
        units: Vec<u16>,
        fix: Fix,
    },
    /// The user clicked the underline `line`: open its paragraph.
    Open(Line),
    /// Select the paragraph `units` opened, so a paste replaces it.
    Select(Vec<u16>),
    Stop,
}

/// The checker thread, seen from the window.
pub(super) struct Handle {
    commands: Sender<Command>,
    waker: Waker,
}

impl Handle {
    pub(super) fn send(&self, command: Command) {
        // A closed channel means the thread is gone; there's nothing to tell.
        let _ = self.commands.send(command);
        self.waker.wake();
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.send(Command::Stop);
    }
}

/// Start the checker thread, which reports to `app` as checker `id`.
pub(super) fn spawn(app: AppHandle<State>, id: u64) -> Option<Handle> {
    let (commands, received) = mpsc::channel();
    let (ready, waker) = mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("fettle-checker".into())
        .spawn(move || {
            let waker = Waker::new();
            let started = waker.is_some();
            // The window waits for this, so it always answers.
            let _ = ready.send(waker);
            if started {
                Worker::new(app, id, received).run();
            }
        });
    if let Err(e) = spawned {
        eprintln!("starting the checker: {e}");
        return None;
    }
    let waker = waker.recv().ok().flatten()?;
    Some(Handle { commands, waker })
}

/// The checker's state on its thread.
struct Worker {
    app: AppHandle<State>,
    /// Which checker this is, so the window ignores one it stopped.
    id: u64,
    commands: Receiver<Command>,
    apps: Apps,
    /// Reused for each read of the field's text.
    buffer: Vec<u16>,
    /// The last check's number, unique across fields, so an answer about an
    /// earlier field can't land in a later one.
    checks: u64,
    /// The model's fix of each paragraph it was asked about, by its text.
    fixes: HashMap<Vec<u16>, Fix>,
    /// The watch on the app in front, with its pid.
    watch: Option<(i32, Watch)>,
    /// Set by the watch when its app reports a change.
    changed: Rc<Cell<bool>>,
    /// The field being underlined, if any.
    session: Option<Session>,
    /// Fields underlined before, most recent last, to pick up again.
    recent: Vec<Session>,
    /// The underlines the window shows.
    drawn: Vec<Line>,
    /// The paragraph opened from an underline, to paste the answer over.
    opened: Option<Paragraph>,
    next_tick: Instant,
    /// When the user's pause after an edit ends, so the edit goes to the
    /// model without waiting for a tick.
    idle_at: Option<Instant>,
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

/// A paragraph in another app's text field, to paste an answer over.
struct Paragraph {
    field: Field,
    range: Range<usize>,
    units: Vec<u16>,
}

impl Worker {
    fn new(app: AppHandle<State>, id: u64, commands: Receiver<Command>) -> Self {
        Self {
            app,
            id,
            commands,
            apps: Apps::default(),
            buffer: Vec::new(),
            checks: 0,
            fixes: HashMap::new(),
            watch: None,
            changed: Rc::default(),
            session: None,
            recent: Vec::new(),
            drawn: Vec::new(),
            opened: None,
            next_tick: Instant::now(),
            idle_at: None,
        }
    }

    /// Read the field on each tick, change and pause, and take the window's
    /// commands, until it stops the checker.
    fn run(mut self) {
        loop {
            let due = self
                .idle_at
                .map_or(self.next_tick, |at| at.min(self.next_tick));
            ax::wait(due.saturating_duration_since(Instant::now()));
            loop {
                match self.commands.try_recv() {
                    Ok(Command::Stop) | Err(TryRecvError::Disconnected) => return,
                    Ok(command) => autoreleasepool(|_| self.take(command)),
                    Err(TryRecvError::Empty) => break,
                }
            }
            let now = Instant::now();
            let idle = self.idle_at.is_some_and(|at| at <= now);
            if self.changed.replace(false) || idle || self.next_tick <= now {
                if idle {
                    self.idle_at = None;
                }
                self.next_tick = now + TICK;
                autoreleasepool(|_| self.refresh());
            }
        }
    }

    fn take(&mut self, command: Command) {
        match command {
            Command::Asking(id, request) => {
                if let Some(session) = self.checking(id) {
                    session.request = Some((id, request));
                }
            }
            Command::Finished { id, units, fix } => self.finished(id, units, fix),
            Command::Skip(id) => {
                if let Some(session) = self.checking(id) {
                    session.request = None;
                    session.doc.skip();
                }
            }
            Command::Forget => self.fixes.clear(),
            Command::Learn { units, fix } => self.remember(units, fix),
            Command::Open(line) => self.open(line),
            Command::Select(units) => {
                let selected = self
                    .opened
                    .as_ref()
                    .is_some_and(|p| p.units == units && p.select());
                self.app.update(move |s, _| s.target_selected(selected));
            }
            Command::Stop => {}
        }
    }

    /// Hide the underlines until the next `refresh` finds a field.
    fn pause(&mut self) {
        self.park();
        self.show(Vec::new());
    }

    /// Keep the field being underlined among the recent ones, its check
    /// still running, dropping the oldest past [`RECENT`].
    fn park(&mut self) {
        if let Some(session) = self.session.take() {
            if self.recent.len() == RECENT {
                self.recent.remove(0);
            }
            self.recent.push(session);
        }
    }

    /// Return the field, current or recent, whose check `id` runs.
    fn checking(&mut self, id: u64) -> Option<&mut Session> {
        self.session
            .iter_mut()
            .chain(&mut self.recent)
            .find(|s| s.doc.checking() == Some(id))
    }

    /// Read the focused field, underline the mistakes known, and ask for the
    /// next paragraph's check, if it's time to. Waits while the window shows.
    fn refresh(&mut self) {
        if self.app.is_visible() {
            self.pause();
            return;
        }
        let Some(Focus { pid, field }) = self.apps.focused() else {
            self.pause();
            return;
        };
        if self
            .watch
            .as_ref()
            .is_none_or(|(watched, _)| *watched != pid)
        {
            let changed = Rc::clone(&self.changed);
            self.watch = Watch::new(pid, move || changed.set(true)).map(|w| (pid, w));
        }
        let same = self.session.as_ref().is_some_and(|s| s.field == field);
        if (!same && !field.is_text_area()) || !field.read(MAX_TEXT, &mut self.buffer) {
            self.pause();
            return;
        }
        match &mut self.session {
            Some(session) if same => {
                if let Some(start) = session.doc.edit(&self.buffer) {
                    let now = Instant::now();
                    session.edited = Some(now);
                    // Text from the edited paragraph on may have moved.
                    session.layout.words.retain(|(word, _)| word.end <= start);
                    session.layout.runs = None;
                    self.idle_at = Some(now + IDLE);
                }
            }
            _ => {
                self.park();
                let session = match self.recent.iter().position(|s| s.field == field) {
                    Some(i) => {
                        let mut session = self.recent.remove(i);
                        // It may have moved or changed while away.
                        session.layout = Layout::default();
                        if session.doc.edit(&self.buffer).is_some() {
                            let now = Instant::now();
                            session.edited = Some(now);
                            self.idle_at = Some(now + IDLE);
                        }
                        session
                    }
                    None => {
                        let fixes = &self.fixes;
                        let doc = Document::new(self.buffer.clone(), |paragraph| {
                            fixes.get(paragraph).map(|f| diff::marks(&f.changes))
                        });
                        Session {
                            field,
                            doc,
                            edited: None,
                            layout: Layout::default(),
                            request: None,
                        }
                    }
                };
                self.session = Some(session);
            }
        }
        self.request();
        self.draw();
    }

    /// Take the model's `fix` of paragraph `units`, asked as check `id`, and
    /// ask about the next paragraph if its field is the one underlined.
    fn finished(&mut self, id: u64, units: Vec<u16>, fix: Option<Fix>) {
        let found = fix.as_ref().map(|f| diff::marks(&f.changes));
        if let Some(fix) = fix {
            self.remember(units, fix);
        }
        let current = self
            .session
            .as_ref()
            .is_some_and(|s| s.doc.checking() == Some(id));
        let Some(session) = self.checking(id) else {
            return;
        };
        session.request = None;
        session.doc.finish(id, found);
        if current {
            self.request();
            self.draw();
        }
    }

    fn remember(&mut self, paragraph: Vec<u16>, fix: Fix) {
        if self.fixes.len() >= MAX_FIXES {
            self.fixes.clear();
        }
        self.fixes.insert(paragraph, fix);
    }

    /// Open the paragraph with the underlined mistake `line` in the window,
    /// with the model's fix of it if known.
    fn open(&mut self, line: Line) {
        let Some(session) = &self.session else {
            return;
        };
        let text = session.doc.text();
        let range = paragraph(text, line.at);
        let Some(units) = text.get(range.clone()) else {
            return;
        };
        let target = Target {
            text: String::from_utf16_lossy(units),
            units: units.to_vec(),
            word: line.word,
        };
        let fix = self.fixes.get(units).cloned();
        self.opened = Some(Paragraph {
            field: session.field.clone(),
            units: units.to_vec(),
            range,
        });
        self.app.update(move |s, _| s.underline_opened(target, fix));
    }

    /// Ask the window to check the next edited paragraph, once the user
    /// stopped typing and no request is running. Paragraphs fixed before
    /// take their known fix, and long ones are skipped.
    fn request(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        if session.edited.is_some_and(|at| at.elapsed() < IDLE) {
            return;
        }
        while let Some(request) = session.doc.next(self.checks + 1) {
            self.checks = request.id;
            if request.text.len() > MAX_PARAGRAPH {
                session.doc.finish(request.id, None);
            } else if let Some(fix) = self.fixes.get(&request.text) {
                session
                    .doc
                    .finish(request.id, Some(diff::marks(&fix.changes)));
            } else {
                let check = Check {
                    id: request.id,
                    text: String::from_utf16_lossy(&request.text),
                    units: request.text,
                };
                self.app.update(move |s, _| s.proofread(check));
                return;
            }
        }
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
        let mut lookups = 0;
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
                    None if lookups == MAX_LOOKUPS => break 'mistakes,
                    // Text scrolled out of the field still has bounds; leave it out.
                    None => {
                        lookups += 1;
                        field
                            .bounds(word.clone())
                            .or_else(|| {
                                let runs = layout.runs.get_or_insert_with(|| field.runs(text));
                                runs.bounds(word.clone())
                            })
                            .filter(|r| inside(*r, frame))
                    }
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
        self.show(lines);
    }

    /// Have the window underline `lines`, unless it already does.
    fn show(&mut self, lines: Vec<Line>) {
        if lines == self.drawn {
            return;
        }
        self.drawn.clone_from(&lines);
        let id = self.id;
        self.app.update_if(move |s, _| {
            s.show_underlines(id, lines);
            false
        });
    }
}

impl Paragraph {
    /// Select the paragraph in its field, so a paste replaces it. Returns
    /// false, selecting nothing, if the field no longer holds it there.
    fn select(&self) -> bool {
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
