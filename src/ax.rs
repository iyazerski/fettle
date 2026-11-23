//! Accessibility text ranges count UTF-16 code units.

use std::cell::Cell;
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

/// Bound each call so an unresponsive app can't stall the checker indefinitely.
const TIMEOUT: f32 = 0.25;
/// Bound traversal of huge web pages when finding static text runs.
const MAX_ELEMENTS: usize = 500;
/// Code isn't prose; exposing VS Code's Accessibility tree also changes its editor behavior.
const SKIPPED_APPS: [&str; 23] = [
    "com.microsoft.VSCode",
    "com.microsoft.VSCodeInsiders",
    "com.vscodium",
    "com.todesktop.230313mzl4w4u92", // Cursor
    "com.exafunction.windsurf",
    "dev.kiro.desktop",
    "com.trae.app",
    "dev.zed.Zed",
    "com.apple.dt.Xcode",
    "com.sublimetext.4",
    "com.panic.Nova",
    "com.barebones.bbedit",
    "com.google.android.studio",
    "com.tinyapp.TablePlus",
    "com.postmanlabs.mac",
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
    "dev.warp.Warp-Stable",
    "net.kovidgoyal.kitty",
    "org.alacritty",
    "com.github.wez.wezterm",
    "co.zeit.hyper",
];
const MESSENGERS: [&str; 3] = [
    "ru.keepcoder.Telegram",
    "com.tdesktop.Telegram",
    "net.whatsapp.WhatsApp",
];
const MESSENGER_SITES: [&str; 2] = ["web.telegram.org", "web.whatsapp.com"];
const MAX_DEPTH: usize = 64;
const CHROMIUM_BROWSERS: [&str; 7] = [
    "com.google.Chrome",
    "org.chromium.Chromium",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    "com.vivaldi.Vivaldi",
    "com.operasoftware.Opera",
    "company.thebrowser.",
];
const NOTIFICATIONS: [&str; 5] = [
    "AXValueChanged",
    "AXSelectedTextChanged",
    "AXFocusedUIElementChanged",
    "AXWindowMoved",
    "AXWindowResized",
];

/// Track attributes enabled in other apps so dropping this restores their Accessibility state.
#[derive(Debug, Default)]
pub struct Apps {
    asked: HashMap<i32, Option<&'static str>>,
}

#[derive(Debug)]
pub struct Focus {
    pub pid: i32,
    pub field: Field,
}

impl Apps {
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
        if is_skipped(&id) || u32::try_from(pid) == Ok(std::process::id()) {
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

fn is_skipped(id: &str) -> bool {
    // Every JetBrains IDE has its own bundle ID under this prefix.
    SKIPPED_APPS.contains(&id) || MESSENGERS.contains(&id) || id.starts_with("com.jetbrains.")
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

/// Return only attributes enabled by Fettle, not those another Accessibility client already enabled.
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

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    element: CFRetained<AXUIElement>,
}

impl Field {
    /// Exclude single-line fields, including address bars, searches, and passwords.
    pub fn is_text_area(&self) -> bool {
        has_role(&self.element, "AXTextArea")
    }

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

    /// Failure clears the buffer, including when text exceeds `max` UTF-16 units.
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

    pub fn bounds(&self, range: Range<usize>) -> Option<CGRect> {
        bounds(&self.element, range)
    }

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

    pub fn select(&self, range: Range<usize>) -> bool {
        let Some(value) = range_value(range) else {
            return false;
        };
        let name = CFString::from_static_str("AXSelectedTextRange");
        // SAFETY: the attribute takes a CFRange value.
        unsafe { self.element.set_attribute_value(&name, &value) == AXError::Success }
    }
}

/// Chrome editable fields can't locate their own text, but their static-text children can.
#[derive(Debug, Default)]
pub struct Runs(Vec<Run>);

#[derive(Debug)]
struct Run {
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

/// Callbacks run on the creating thread while it processes its run loop in [`wait`].
pub struct Watch {
    observer: CFRetained<AXObserver>,
    run_loop: CFRetained<CFRunLoop>,
    changed: *const Cell<bool>,
}

impl Watch {
    pub fn new(pid: i32) -> Option<Self> {
        let mut out: *mut AXObserver = ptr::null_mut();
        // SAFETY: `out` receives a +1 reference on success.
        let error = unsafe { AXObserver::create(pid, Some(changed), NonNull::from(&mut out)) };
        if error != AXError::Success {
            return None;
        }
        // SAFETY: a successful create hands over a +1 reference.
        let observer = unsafe { CFRetained::from_raw(NonNull::new(out)?) };
        let watch = Self {
            observer,
            run_loop: CFRunLoop::current()?,
            changed: Box::into_raw(Box::new(Cell::new(false))),
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
                    .add_notification(&app, &name, watch.changed.cast_mut().cast())
            };
        }
        // SAFETY: an immutable constant set up before any code runs, and the
        // observer's own source.
        let (modes, source) = unsafe { (kCFRunLoopCommonModes, watch.observer.run_loop_source()) };
        watch.run_loop.add_source(Some(&source), modes);
        Some(watch)
    }

    pub fn take_changed(&self) -> bool {
        // SAFETY: `self.changed` lives until `drop` and is only touched on the creating thread.
        unsafe { &*self.changed }.replace(false)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: as in `new`.
        let (modes, source) = unsafe { (kCFRunLoopCommonModes, self.observer.run_loop_source()) };
        self.run_loop.remove_source(Some(&source), modes);
        // SAFETY: made by `Box::into_raw` in `new`, and the run loop no longer
        // calls `changed` with it.
        drop(unsafe { Box::from_raw(self.changed.cast_mut()) });
    }
}

/// Signal from any thread; an early wake ends the next wait immediately.
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

pub fn wait(timeout: Duration) {
    // SAFETY: an immutable constant set up before any code runs.
    let mode = unsafe { kCFRunLoopDefaultMode };
    CFRunLoop::run_in_mode(mode, timeout.as_secs_f64(), true);
}

unsafe extern "C-unwind" fn changed(
    _: NonNull<AXObserver>,
    _: NonNull<AXUIElement>,
    _: NonNull<CFString>,
    flag: *mut c_void,
) {
    // SAFETY: the refcon is the `Watch`'s flag, alive while the observer is on the run loop.
    unsafe { &*flag.cast::<Cell<bool>>() }.set(true);
}

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

fn attribute(element: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let name = CFString::from_static_str(name);
    let mut out: *const CFType = ptr::null();
    // SAFETY: `out` receives a +1 reference on success.
    let error = unsafe { element.copy_attribute_value(&name, NonNull::from(&mut out)) };
    retained(error, out)
}

fn has_role(element: &AXUIElement, role: &'static str) -> bool {
    attribute(element, "AXRole")
        .and_then(|v| v.downcast::<CFString>().ok())
        .is_some_and(|r| *r == *CFString::from_static_str(role))
}

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

fn range_value(range: Range<usize>) -> Option<CFRetained<AXValue>> {
    let range = CFRange {
        location: CFIndex::try_from(range.start).ok()?,
        length: CFIndex::try_from(range.len()).ok()?,
    };
    // SAFETY: a CFRange value is made from a CFRange.
    unsafe { AXValue::new(AXValueType::CFRange, NonNull::from(&range).cast()) }
}

/// # Safety
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
