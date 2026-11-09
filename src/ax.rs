//! The focused text field in another app, read, watched and selected through
//! the Accessibility API, which needs the same permission as ⌘C and ⌘V.
//!
//! Text ranges count UTF-16 code units, as the API and `NSString` do.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ops::Range;
use std::ptr::{self, NonNull};
use std::sync::Once;
use std::time::Duration;

use objc2_app_kit::NSWorkspace;
use objc2_application_services::{AXError, AXObserver, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFIndex, CFNumber, CFRange, CFRetained, CFRunLoop, CFRunLoopSource,
    CFRunLoopSourceContext, CFString, CFType, CFURL, CGPoint, CGRect, CGSize,
    kCFRunLoopCommonModes, kCFRunLoopDefaultMode,
};

/// How long an Accessibility call waits for an app, in seconds, so a hung
/// app can't hold the checker up for long.
const TIMEOUT: f32 = 0.25;
/// At most this many elements under a field are looked at to find its
/// [`Runs`], so a huge web page can't stall the UI thread.
const MAX_ELEMENTS: usize = 500;
/// Apps Fettle leaves alone, by bundle ID: their text is code, commands or
/// queries, not prose, so checking it only adds noise. Editors built on VS
/// Code also switch to screen reader mode when asked to expose their content,
/// which changes the editor.
const SKIPPED_APPS: [&str; 23] = [
    // Editors built on VS Code.
    "com.microsoft.VSCode",
    "com.microsoft.VSCodeInsiders",
    "com.vscodium",
    "com.todesktop.230313mzl4w4u92", // Cursor
    "com.exafunction.windsurf",
    "dev.kiro.desktop",
    "com.trae.app",
    // Other code editors.
    "dev.zed.Zed",
    "com.apple.dt.Xcode",
    "com.sublimetext.4",
    "com.panic.Nova",
    "com.barebones.bbedit",
    "com.google.android.studio",
    // Database and API tools.
    "com.tinyapp.TablePlus",
    "com.postmanlabs.mac",
    // Terminals.
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
    "dev.warp.Warp-Stable",
    "net.kovidgoyal.kitty",
    "org.alacritty",
    "com.github.wez.wezterm",
    "co.zeit.hyper",
];
/// Messengers Fettle doesn't underline in, by bundle ID, and their web
/// versions, by host.
const MESSENGERS: [&str; 3] = [
    "ru.keepcoder.Telegram",
    "com.tdesktop.Telegram",
    "net.whatsapp.WhatsApp",
];
const MESSENGER_SITES: [&str; 2] = ["web.telegram.org", "web.whatsapp.com"];
/// At most this many ancestors of a field are looked through for the web page it's on.
const MAX_DEPTH: usize = 64;
/// Bundle ID prefixes of vendors whose apps are all skipped: every JetBrains
/// IDE has its own ID.
const SKIPPED_VENDORS: [&str; 1] = ["com.jetbrains."];
/// Bundle ID prefixes of Chromium browsers, which build the accessibility
/// tree of web pages, with where their text shows, only for a screen reader.
const CHROMIUM_BROWSERS: [&str; 7] = [
    "com.google.Chrome",
    "org.chromium.Chromium",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    "com.vivaldi.Vivaldi",
    "com.operasoftware.Opera",
    "company.thebrowser.",
];
/// What a [`Watch`] listens for: edits, caret moves, focus moves, and
/// windows moving under the text.
const NOTIFICATIONS: [&str; 5] = [
    "AXValueChanged",
    "AXSelectedTextChanged",
    "AXFocusedUIElementChanged",
    "AXWindowMoved",
    "AXWindowResized",
];

/// The apps Fettle reads from, remembering which it asked to expose their
/// web content, and the attribute it turned on in each, to turn it off once
/// dropped: the content costs them memory and speed, and the browsers
/// animate their windows differently.
#[derive(Debug, Default)]
pub struct Apps {
    asked: HashMap<i32, Option<&'static str>>,
}

/// The app in front and its focused element, if Fettle may read it.
#[derive(Debug)]
pub struct Focus {
    pub pid: i32,
    pub field: Field,
}

impl Apps {
    /// Return the element with the keyboard focus in the app in front,
    /// unless that's Fettle or a skipped app.
    pub fn focused(&mut self) -> Option<Focus> {
        static TIMEOUT_SET: Once = Once::new();
        TIMEOUT_SET.call_once(|| {
            // SAFETY: the system-wide element is a plain CF object; on it this
            // sets the timeout for every element.
            unsafe { AXUIElement::new_system_wide().set_messaging_timeout(TIMEOUT) };
        });
        let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let pid = front.processIdentifier();
        let id = front
            .bundleIdentifier()
            .map(|id| id.to_string())
            .unwrap_or_default();
        // SAFETY: getpid has no preconditions.
        if is_skipped(&id) || pid == unsafe { libc::getpid() } {
            return None;
        }
        // SAFETY: a plain CF object for the app with `pid`.
        let app = unsafe { AXUIElement::new_application(pid) };
        if let Entry::Vacant(entry) = self.asked.entry(pid) {
            entry.insert(expose_web_content(&app, &id));
        }
        // Not the system-wide element's: that fails now and then with
        // "cannot complete" while the app's own answers.
        let element = attribute(&app, "AXFocusedUIElement")?
            .downcast::<AXUIElement>()
            .ok()?;
        Some(Focus {
            pid,
            field: Field { element },
        })
    }
}

/// Append the UTF-16 units of `text` to `buffer`.
fn append_units(text: &CFString, buffer: &mut Vec<u16>) {
    let Ok(len) = usize::try_from(text.length()) else {
        return;
    };
    buffer.reserve(len);
    let start = buffer.len();
    // SAFETY: CFStringGetCharacters fills exactly `len` units past `start`,
    // which the buffer has room for.
    unsafe {
        text.characters(
            CFRange {
                location: 0,
                length: text.length(),
            },
            buffer.as_mut_ptr().add(start),
        );
        buffer.set_len(start + len);
    }
}

/// Whether Fettle leaves the app with bundle ID `id` alone.
fn is_skipped(id: &str) -> bool {
    SKIPPED_APPS.contains(&id)
        || MESSENGERS.contains(&id)
        || SKIPPED_VENDORS.iter().any(|v| id.starts_with(v))
}

impl Drop for Apps {
    fn drop(&mut self) {
        // VoiceOver needs the web content too, and may have asked for it first.
        if NSWorkspace::sharedWorkspace().isVoiceOverEnabled() {
            return;
        }
        for (pid, attribute) in self.asked.drain() {
            let Some(attribute) = attribute else {
                continue;
            };
            // SAFETY: a plain CF object for the app with `pid`.
            let app = unsafe { AXUIElement::new_application(pid) };
            let name = CFString::from_static_str(attribute);
            // SAFETY: the attribute takes a CFBoolean. An app that quit returns an error, which is fine.
            unsafe { app.set_attribute_value(&name, CFBoolean::new(false)) };
        }
    }
}

/// Ask an Electron app or a Chromium browser, `id`, to build its web
/// content's accessibility tree, which they only do for screen readers
/// unless asked. Returns the attribute it turned on, unless the app ignores
/// it or another client, such as a screen reader, had turned it on already.
fn expose_web_content(app: &AXUIElement, id: &str) -> Option<&'static str> {
    // Chromium browsers ignore Electron's attribute and take the one
    // VoiceOver sets, after a 2-second wait. It changes how other apps
    // animate their windows, so it's only set on the browsers.
    let name = if CHROMIUM_BROWSERS.iter().any(|b| id.starts_with(b)) {
        "AXEnhancedUserInterface"
    } else {
        "AXManualAccessibility"
    };
    let on = attribute(app, name)
        .and_then(|v| v.downcast::<CFBoolean>().ok())
        .is_some_and(|v| v.as_bool());
    if on {
        return None;
    }
    let attribute = CFString::from_static_str(name);
    // SAFETY: the attribute takes a CFBoolean. Apps without it return an error.
    let set = unsafe { app.set_attribute_value(&attribute, CFBoolean::new(true)) };
    (set == AXError::Success).then_some(name)
}

/// A text field in another app.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    element: CFRetained<AXUIElement>,
}

impl Field {
    /// Whether this is a text area, where people write prose: message
    /// composers, notes and editable web content are, while single-line
    /// fields such as address bars, search boxes and passwords aren't.
    pub fn is_text_area(&self) -> bool {
        has_role(&self.element, "AXTextArea")
    }

    /// Whether the field is on a messenger's web page, which Fettle leaves
    /// alone as it does the messenger's app.
    pub fn on_messenger_site(&self) -> bool {
        let mut element = self.element.clone();
        for _ in 0..MAX_DEPTH {
            let Some(parent) =
                attribute(&element, "AXParent").and_then(|p| p.downcast::<AXUIElement>().ok())
            else {
                return false;
            };
            element = parent;
            if has_role(&element, "AXWebArea") {
                return attribute(&element, "AXURL")
                    .and_then(|u| u.downcast::<CFURL>().ok())
                    .and_then(|u| u.host_name())
                    .is_some_and(|host| MESSENGER_SITES.contains(&host.to_string().as_str()));
            }
        }
        false
    }

    /// Read the field's text into `buffer`. Returns false, leaving `buffer`
    /// empty, if the field has no text or more than `max` units of it.
    pub fn read(&self, max: usize, buffer: &mut Vec<u16>) -> bool {
        buffer.clear();
        let count = attribute(&self.element, "AXNumberOfCharacters")
            .and_then(|n| n.downcast::<CFNumber>().ok())
            .and_then(|n| n.as_i64())
            .and_then(|n| usize::try_from(n).ok());
        if count.is_some_and(|n| n > max) {
            return false;
        }
        let Some(text) =
            attribute(&self.element, "AXValue").and_then(|v| v.downcast::<CFString>().ok())
        else {
            return false;
        };
        if usize::try_from(text.length()).is_ok_and(|len| len <= max) {
            append_units(&text, buffer);
            true
        } else {
            false
        }
    }

    /// Return the selected range, or the caret as an empty one.
    pub fn selection(&self) -> Option<Range<usize>> {
        let value = attribute(&self.element, "AXSelectedTextRange")?;
        // SAFETY: a CFRange value holds a CFRange.
        let range: CFRange = unsafe { unpack(&value, AXValueType::CFRange)? };
        to_range(range)
    }

    /// Return the range of the text that shows, if the field tells.
    pub fn visible(&self) -> Option<Range<usize>> {
        let value = attribute(&self.element, "AXVisibleCharacterRange")?;
        // SAFETY: a CFRange value holds a CFRange.
        let range: CFRange = unsafe { unpack(&value, AXValueType::CFRange)? };
        to_range(range)
    }

    /// Return the field's frame in screen points from the primary screen's top-left.
    pub fn frame(&self) -> Option<CGRect> {
        let position = attribute(&self.element, "AXPosition")?;
        let size = attribute(&self.element, "AXSize")?;
        // SAFETY: CGPoint and CGSize values hold those types.
        let (origin, size): (CGPoint, CGSize) = unsafe {
            (
                unpack(&position, AXValueType::CGPoint)?,
                unpack(&size, AXValueType::CGSize)?,
            )
        };
        Some(CGRect::new(origin, size))
    }

    /// Return where `range` of the text shows, as `frame` counts.
    pub fn bounds(&self, range: Range<usize>) -> Option<CGRect> {
        bounds(&self.element, range)
    }

    /// Find the static texts under the field and where each starts in
    /// `text`, the field's text.
    pub fn runs(&self, text: &[u16]) -> Runs {
        let mut runs = Vec::new();
        let mut cursor = 0;
        let mut units = Vec::new();
        let mut stack = children(&self.element);
        let mut seen = 0;
        while let Some(element) = stack.pop() {
            seen += 1;
            if seen > MAX_ELEMENTS {
                break;
            }
            if !has_role(&element, "AXStaticText") {
                stack.extend(children(&element));
                continue;
            }
            let Some(value) =
                attribute(&element, "AXValue").and_then(|v| v.downcast::<CFString>().ok())
            else {
                continue;
            };
            units.clear();
            append_units(&value, &mut units);
            if units.is_empty() {
                continue;
            }
            // Pieces the field's text doesn't hold, such as an emoji's name, are skipped.
            let found = text
                .get(cursor..)
                .and_then(|rest| rest.windows(units.len()).position(|w| w == units));
            if let Some(at) = found {
                let start = cursor + at;
                cursor = start + units.len();
                runs.push(Run {
                    text: start..cursor,
                    element,
                });
            }
        }
        Runs(runs)
    }

    /// Select `range` of the text, so a paste replaces it. Returns whether the field took it.
    pub fn select(&self, range: Range<usize>) -> bool {
        let Some(value) = range_value(range) else {
            return false;
        };
        let name = CFString::from_static_str("AXSelectedTextRange");
        // SAFETY: the attribute takes a CFRange value.
        unsafe { self.element.set_attribute_value(&name, &value) == AXError::Success }
    }
}

/// A field's text split into the static texts under it, for fields such as
/// Chrome's editable web content, which can't tell where their own text
/// shows but whose pieces can.
#[derive(Debug, Default)]
pub struct Runs(Vec<Run>);

#[derive(Debug)]
struct Run {
    /// The run's part of the field's text.
    text: Range<usize>,
    element: CFRetained<AXUIElement>,
}

impl Runs {
    /// Return where `range` of the field's text shows, cut at the end of the run it starts in.
    pub fn bounds(&self, range: Range<usize>) -> Option<CGRect> {
        let i = self.0.partition_point(|r| r.text.end <= range.start);
        let run = self.0.get(i).filter(|r| r.text.start <= range.start)?;
        let end = range.end.min(run.text.end);
        bounds(
            &run.element,
            range.start - run.text.start..end - run.text.start,
        )
    }
}

/// Calls a function, on the thread that made it, whenever the app it
/// watches changes text, caret, focus or window, until dropped. The calls
/// come while that thread [`wait`]s. Apps that don't send these are still
/// read on a timer; this only makes the rest quick.
pub struct Watch {
    observer: CFRetained<AXObserver>,
    run_loop: CFRetained<CFRunLoop>,
    on_change: *mut Box<dyn Fn()>,
}

impl Watch {
    /// Watch the app with `pid`.
    pub fn new(pid: i32, on_change: impl Fn() + 'static) -> Option<Self> {
        let mut out: *mut AXObserver = ptr::null_mut();
        // SAFETY: `out` receives a +1 reference on success.
        let error = unsafe { AXObserver::create(pid, Some(changed), NonNull::from(&mut out)) };
        if error != AXError::Success {
            return None;
        }
        // SAFETY: a successful create hands over a +1 reference.
        let observer = unsafe { CFRetained::from_raw(NonNull::new(out)?) };
        let on_change: Box<Box<dyn Fn()>> = Box::new(Box::new(on_change));
        let watch = Self {
            observer,
            run_loop: CFRunLoop::current()?,
            on_change: Box::into_raw(on_change),
        };
        // SAFETY: a plain CF object for the app with `pid`.
        let app = unsafe { AXUIElement::new_application(pid) };
        for name in NOTIFICATIONS {
            let name = CFString::from_static_str(name);
            // SAFETY: the refcon lives until `drop` removes the observer from the run loop.
            // Apps lacking a notification return an error, which is fine.
            unsafe {
                watch
                    .observer
                    .add_notification(&app, &name, watch.on_change.cast())
            };
        }
        // SAFETY: an immutable constant set up before any code runs, and the
        // observer's own source.
        let (modes, source) = unsafe { (kCFRunLoopCommonModes, watch.observer.run_loop_source()) };
        watch.run_loop.add_source(Some(&source), modes);
        Some(watch)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: as in `new`.
        let (modes, source) = unsafe { (kCFRunLoopCommonModes, self.observer.run_loop_source()) };
        self.run_loop.remove_source(Some(&source), modes);
        // SAFETY: made by `Box::into_raw` in `new`, and the run loop no longer
        // calls `changed` with it.
        drop(unsafe { Box::from_raw(self.on_change) });
    }
}

/// Wakes the thread that made it from its [`wait`], from any thread. A wake
/// that comes while the thread is busy ends its next wait at once.
pub struct Waker {
    source: CFRetained<CFRunLoopSource>,
    run_loop: CFRetained<CFRunLoop>,
}

// SAFETY: a waker only signals its source and wakes its run loop, which
// Core Foundation allows from any thread, and CF reference counts are atomic.
unsafe impl Send for Waker {}
// SAFETY: as for `Send`: both calls are thread-safe.
unsafe impl Sync for Waker {}

impl Waker {
    /// Make a waker for the current thread.
    pub fn new() -> Option<Self> {
        let mut context = CFRunLoopSourceContext {
            version: 0,
            info: ptr::null_mut(),
            retain: None,
            release: None,
            copyDescription: None,
            equal: None,
            hash: None,
            schedule: None,
            cancel: None,
            perform: Some(woken),
        };
        // SAFETY: the context is valid for the call, which copies it, and
        // its callback needs no `info`.
        let source = unsafe { CFRunLoopSource::new(None, 0, &mut context) }?;
        let run_loop = CFRunLoop::current()?;
        // SAFETY: an immutable constant set up before any code runs.
        run_loop.add_source(Some(&source), unsafe { kCFRunLoopDefaultMode });
        Some(Self { source, run_loop })
    }

    pub fn wake(&self) {
        self.source.signal();
        self.run_loop.wake_up();
    }
}

/// A [`Waker`]'s callback: being woken is all it's for.
unsafe extern "C-unwind" fn woken(_: *mut c_void) {}

/// Wait on the current thread for up to `timeout`, or until a [`Watch`]
/// it made reports a change or a [`Waker`] it made is woken.
pub fn wait(timeout: Duration) {
    // SAFETY: an immutable constant set up before any code runs.
    let mode = unsafe { kCFRunLoopDefaultMode };
    CFRunLoop::run_in_mode(mode, timeout.as_secs_f64(), true);
}

unsafe extern "C-unwind" fn changed(
    _: NonNull<AXObserver>,
    _: NonNull<AXUIElement>,
    _: NonNull<CFString>,
    on_change: *mut c_void,
) {
    // SAFETY: the refcon is the `Watch`'s callback, alive while the observer is on the run loop.
    let on_change = unsafe { &*on_change.cast::<Box<dyn Fn()>>() };
    on_change();
}

/// Return where `range` of `element`'s text shows, in screen points from the
/// primary screen's top-left.
fn bounds(element: &AXUIElement, range: Range<usize>) -> Option<CGRect> {
    let param = range_value(range)?;
    let name = CFString::from_static_str("AXBoundsForRange");
    let mut out: *const CFType = ptr::null();
    // SAFETY: the attribute takes a CFRange value; `out` receives a +1 reference.
    let error = unsafe {
        element.copy_parameterized_attribute_value(&name, &param, NonNull::from(&mut out))
    };
    let value = retained(error, out)?;
    // SAFETY: a CGRect value holds a CGRect.
    let rect: CGRect = unsafe { unpack(&value, AXValueType::CGRect)? };
    // Chrome answers for editable web content with an empty rect.
    (rect.size.width > 0.0 && rect.size.height > 0.0).then_some(rect)
}

/// Return `element`'s children, last first, so popping them goes in order.
fn children(element: &AXUIElement) -> Vec<CFRetained<AXUIElement>> {
    let Some(array) = attribute(element, "AXChildren").and_then(|v| v.downcast::<CFArray>().ok())
    else {
        return Vec::new();
    };
    // SAFETY: a CFArray holds CF objects.
    let array = unsafe { array.cast_unchecked::<CFType>() };
    let mut out: Vec<_> = array
        .iter()
        .filter_map(|c| c.downcast::<AXUIElement>().ok())
        .collect();
    out.reverse();
    out
}

/// Return `element`'s attribute `name`, if it has one.
fn attribute(element: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let name = CFString::from_static_str(name);
    let mut out: *const CFType = ptr::null();
    // SAFETY: `out` receives a +1 reference on success.
    let error = unsafe { element.copy_attribute_value(&name, NonNull::from(&mut out)) };
    retained(error, out)
}

/// Whether `element`'s role is `role`.
fn has_role(element: &AXUIElement, role: &'static str) -> bool {
    attribute(element, "AXRole")
        .and_then(|v| v.downcast::<CFString>().ok())
        .is_some_and(|r| *r == *CFString::from_static_str(role))
}

/// Take ownership of the +1 reference a successful copy left in `out`.
fn retained(error: AXError, out: *const CFType) -> Option<CFRetained<CFType>> {
    if error != AXError::Success {
        return None;
    }
    // SAFETY: a successful copy hands over a +1 reference, or null.
    NonNull::new(out.cast_mut()).map(|p| unsafe { CFRetained::from_raw(p) })
}

fn to_range(range: CFRange) -> Option<Range<usize>> {
    let start = usize::try_from(range.location).ok()?;
    Some(start..start + usize::try_from(range.length).ok()?)
}

/// Wrap `range` in an AXValue.
fn range_value(range: Range<usize>) -> Option<CFRetained<AXValue>> {
    let range = CFRange {
        location: CFIndex::try_from(range.start).ok()?,
        length: CFIndex::try_from(range.len()).ok()?,
    };
    // SAFETY: a CFRange value is made from a CFRange.
    unsafe { AXValue::new(AXValueType::CFRange, NonNull::from(&range).cast()) }
}

/// Read the `T` that `value` holds.
///
/// # Safety
///
/// `kind` must name `T`.
unsafe fn unpack<T>(value: &CFType, kind: AXValueType) -> Option<T> {
    let value = value.downcast_ref::<AXValue>()?;
    let mut out = MaybeUninit::<T>::uninit();
    // SAFETY: `kind` names `T`, so a successful read fills `out`.
    unsafe {
        value
            .value(kind, NonNull::from(&mut out).cast())
            .then(|| out.assume_init())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Instant;

    use super::*;

    #[test]
    fn a_waker_ends_its_threads_wait_even_when_woken_early() {
        let (tx, rx) = mpsc::channel();
        let (go, ready) = mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            tx.send(Waker::new().expect("a run loop source")).unwrap();
            // Woken before it waits: the first wait ends at once.
            ready.recv().unwrap();
            let start = Instant::now();
            wait(Duration::from_secs(5));
            let early = start.elapsed();
            // Woken while it waits.
            let start = Instant::now();
            wait(Duration::from_secs(5));
            (early, start.elapsed())
        });
        let waker = rx.recv().unwrap();
        waker.wake();
        go.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        waker.wake();
        let (early, late) = thread.join().unwrap();
        assert!(early < Duration::from_secs(1), "first wait took {early:?}");
        assert!(late < Duration::from_secs(1), "second wait took {late:?}");
    }
}
