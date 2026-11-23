//! Accessibility calls can block on other apps, so they run on this worker, not the UI thread.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use objc2::rc::autoreleasepool;
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_foundation::NSPointInRect;
use rune::AppHandle;

use super::{Check, Fix, Target};
use crate::ax::{self, Apps, Field, Focus, Runs, Waker, Watch};
use crate::diff;
use crate::document::{Document, paragraph};
use crate::mac::{self, Clip};
use crate::rich;
use crate::ui::pending::{Pending, Ticket};
use crate::ui::state::{PASTE_DELAY, State};
use crate::underlines::Line;

/// Poll for scrolling and apps that don't report changes.
const TICK: Duration = Duration::from_millis(300);
/// Wait for a typing pause rather than sending every keystroke.
const IDLE: Duration = Duration::from_millis(1200);
/// Longer paragraphs, in UTF-16 units, aren't sent: each edit would resend them.
const MAX_PARAGRAPH: usize = 4000;
const MAX_FIXES: usize = 256;
/// Longer texts, in UTF-16 units, aren't checked: reading them on every
/// change would cost too much.
const MAX_TEXT: usize = 100_000;
/// At most this many words are underlined, as each costs a call to the app.
const MAX_LINES: usize = 40;
/// Bound failed lookups too, so an uncooperative field can't monopolize the worker.
const MAX_LOOKUPS: usize = 60;
/// Keep pending edits and checks across quick field switches.
const RECENT: usize = 8;

pub(super) enum Command {
    Asking(u64, Pending),
    Finished {
        id: u64,
        units: Vec<u16>,
        fix: Option<Fix>,
    },
    /// Leave the edits of the field that asked for check `id` unchecked:
    /// there's no model to ask.
    Skip(u64),
    Forget,
    Learn {
        units: Vec<u16>,
        fix: Fix,
    },
    LearnOpened {
        opened: Ticket,
        fix: Fix,
    },
    Open(Line),
    Locate(Ticket),
    Replace(String),
    Stop,
}

pub(super) struct Handle {
    commands: Sender<Command>,
    waker: Waker,
    /// Closes once the thread has ended and undone what it changed in other apps.
    ended: Receiver<()>,
}

impl Handle {
    pub(super) fn send(&self, command: Command) {
        // A closed channel means the thread is gone; there's nothing to tell.
        let _ = self.commands.send(command);
        self.waker.wake();
    }

    /// Bound the wait because an Accessibility call may be stuck in another app.
    pub(super) fn stop_and_wait(self, timeout: Duration) {
        self.send(Command::Stop);
        let _ = self.ended.recv_timeout(timeout);
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.send(Command::Stop);
    }
}

pub(super) fn spawn(app: AppHandle<State>, id: u64) -> Option<Handle> {
    let (commands, received) = mpsc::channel();
    let (ready, waker) = mpsc::sync_channel(1);
    let (end, ended) = mpsc::channel::<()>();
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
            drop(end);
        });
    if let Err(e) = spawned {
        eprintln!("starting the checker: {e}");
        return None;
    }
    let waker = waker.recv().ok().flatten()?;
    Some(Handle {
        commands,
        waker,
        ended,
    })
}

struct Worker {
    app: AppHandle<State>,
    /// Which checker this is, so the window ignores one it stopped.
    id: u64,
    commands: Receiver<Command>,
    apps: Apps,
    buffer: Vec<u16>,
    /// Unique across fields so late replies can't land in a different document.
    checks: u64,
    fixes: HashMap<Vec<u16>, Fix>,
    watch: Option<(i32, Watch)>,
    session: Option<Session>,
    /// Fields underlined before, most recent last, to pick up again.
    recent: Vec<Session>,
    /// The last field found not to underline in, so it isn't looked at again each tick.
    left_alone: Option<Field>,
    drawn: Vec<Line>,
    opened: Option<Paragraph>,
    next_tick: Instant,
    /// Send after a typing pause without waiting for the next poll.
    idle_at: Option<Instant>,
}

struct Session {
    field: Field,
    doc: Document,
    edited: Option<Instant>,
    layout: Layout,
    /// The model request for check `.0`; dropping the session stops it.
    request: Option<(u64, Pending)>,
}

/// Reuse word bounds until the field's position, scroll, or preceding text changes.
#[derive(Default)]
struct Layout {
    frame: Option<CGRect>,
    visible: Option<Range<usize>>,
    /// Each word asked about, and its bounds unless it doesn't show.
    words: Vec<(Range<usize>, Option<CGRect>)>,
    /// The field's runs, found the first time the field can't place a word.
    runs: Option<Runs>,
}

struct Paragraph {
    opened: Ticket,
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
            session: None,
            recent: Vec::new(),
            left_alone: None,
            drawn: Vec::new(),
            opened: None,
            next_tick: Instant::now(),
            idle_at: None,
        }
    }

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
            let changed = self.watch.as_ref().is_some_and(|(_, w)| w.take_changed());
            if changed || idle || self.next_tick <= now {
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
            // Fields' documents hold the old model's marks, and their checks ask it.
            Command::Forget => {
                self.fixes.clear();
                self.session = None;
                self.recent.clear();
            }
            Command::Learn { units, fix } => self.remember(units, fix),
            Command::LearnOpened { opened, fix } => {
                if let Some(p) = self.opened.as_ref().filter(|p| p.opened == opened) {
                    self.remember(p.units.clone(), fix);
                }
            }
            Command::Open(line) => self.open(line),
            Command::Locate(opened) => {
                let found = self
                    .opened
                    .as_ref()
                    .is_some_and(|p| p.opened == opened && p.holds());
                self.app.update(move |s, _| s.target_located(opened, found));
            }
            Command::Replace(text) => match &self.opened {
                Some(opened) if opened.select() => {
                    // Meanwhile the window hides, and the app gets its keys back for ⌘C.
                    std::thread::sleep(PASTE_DELAY);
                    let old = String::from_utf16_lossy(&opened.units);
                    let clip = match mac::copy_selection() {
                        Some(copied) => rich::replacement(&copied.clip, &old, &text),
                        None => Clip::plain(&text),
                    };
                    mac::paste(&clip);
                }
                _ => eprintln!("pasting a fix: the paragraph moved"),
            },
            Command::Stop => {}
        }
    }

    fn pause(&mut self) {
        self.park();
        self.show(Vec::new());
    }

    fn park(&mut self) {
        if let Some(session) = self.session.take() {
            if self.recent.len() == RECENT {
                self.recent.remove(0);
            }
            self.recent.push(session);
        }
    }

    fn checking(&mut self, id: u64) -> Option<&mut Session> {
        self.session
            .iter_mut()
            .chain(&mut self.recent)
            .find(|s| s.doc.checking() == Some(id))
    }

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
            self.watch = Watch::new(pid).map(|w| (pid, w));
        }
        let same = self.session.as_ref().is_some_and(|s| s.field == field);
        if (!same && !self.underlines_in(&field)) || !field.read(MAX_TEXT, &mut self.buffer) {
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
                // Taken out before parking the current field, which may drop the oldest.
                let back = self
                    .recent
                    .iter()
                    .position(|s| s.field == field)
                    .map(|i| self.recent.remove(i));
                self.park();
                let session = match back {
                    Some(mut session) => {
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

    fn underlines_in(&mut self, field: &Field) -> bool {
        if self.left_alone.as_ref() == Some(field) {
            return false;
        }
        let prose = field.is_text_area() && !field.on_messenger_site();
        if !prose {
            self.left_alone = Some(field.clone());
        }
        prose
    }

    fn finished(&mut self, id: u64, units: Vec<u16>, fix: Option<Fix>) {
        let current = self
            .session
            .as_ref()
            .is_some_and(|s| s.doc.checking() == Some(id));
        let Some(session) = self.checking(id) else {
            return;
        };
        session.request = None;
        session
            .doc
            .finish(id, fix.as_ref().map(|f| diff::marks(&f.changes)));
        if let Some(fix) = fix {
            self.remember(units, fix);
        }
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

    fn open(&mut self, line: Line) {
        let Some(session) = &self.session else {
            return;
        };
        let text = session.doc.text();
        let range = paragraph(text, line.at);
        let units = &text[range.clone()];
        // A double-click must keep the first opening's ticket, which the window already holds.
        let opened = self
            .opened
            .as_ref()
            .filter(|p| p.field == session.field && p.range == range && p.units == units)
            .map_or_else(Ticket::new, |p| p.opened);
        let target = Target {
            opened,
            text: String::from_utf16_lossy(units),
            word: line.word,
        };
        let fix = self.fixes.get(units).cloned();
        self.opened = Some(Paragraph {
            opened,
            field: session.field.clone(),
            units: units.to_vec(),
            range,
        });
        self.app.update(move |s, _| s.underline_opened(target, fix));
    }

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
    fn holds(&self) -> bool {
        let mut text = Vec::new();
        self.field.read(MAX_TEXT, &mut text)
            && text.get(self.range.clone()) == Some(&self.units[..])
    }

    /// Leave the selection alone if external edits moved the paragraph.
    fn select(&self) -> bool {
        self.holds() && self.field.select(self.range.clone())
    }
}

fn inside(rect: CGRect, frame: Option<CGRect>) -> bool {
    let middle = CGPoint::new(
        rect.origin.x + rect.size.width / 2.0,
        rect.origin.y + rect.size.height / 2.0,
    );
    frame.is_none_or(|f| NSPointInRect(middle, f))
}

/// Split phrases into words so wrapped text gets separate underlines, not one large box.
fn words(text: &[u16], range: Range<usize>) -> impl Iterator<Item = Range<usize>> + '_ {
    let space = |u: u16| char::from_u32(u32::from(u)).is_some_and(char::is_whitespace);
    let end = range.end.min(text.len());
    let mut i = range.start;
    std::iter::from_fn(move || {
        while i < end && space(text[i]) {
            i += 1;
        }
        let start = i;
        while i < end && !space(text[i]) {
            i += 1;
        }
        (start < i).then_some(start..i)
    })
}
