//! The focused text field in another app, read, watched and selected through
//! the Accessibility API, which needs the same permission as ⌘C and ⌘V.
//!
//! Text ranges count UTF-16 code units, as the API and `NSString` do.

use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ops::Range;
use std::ptr::{self, NonNull};
use std::sync::Once;

use objc2_app_kit::NSWorkspace;
use objc2_application_services::{AXError, AXObserver, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{
    CFBoolean, CFIndex, CFNumber, CFRange, CFRetained, CFRunLoop, CFString, CFType, CGPoint,
    CGRect, CGSize, kCFRunLoopCommonModes,
};

/// How long an Accessibility call waits for an app, in seconds: checks run
/// on the UI thread, which a hung app mustn't stall.
const TIMEOUT: f32 = 0.25;
/// Code editors built on VS Code, by bundle ID. Fettle leaves them alone:
/// asking them to expose their content switches them to screen reader mode,
/// which changes the editor, and spell checking code only adds noise.
const CODE_EDITORS: [&str; 5] = [
    "com.microsoft.VSCode",
    "com.microsoft.VSCodeInsiders",
    "com.vscodium",
    "com.todesktop.230313mzl4w4u92", // Cursor
    "com.exafunction.windsurf",
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
/// web content.
#[derive(Debug, Default)]
pub struct Apps {
    asked: HashSet<i32>,
}

/// The app in front and its focused element, if Fettle may read it.
#[derive(Debug)]
pub struct Focus {
    pub pid: i32,
    pub field: Field,
}

impl Apps {
    /// Return the element with the keyboard focus in the app in front,
    /// unless that's Fettle or a code editor.
    pub fn focused(&mut self) -> Option<Focus> {
        static TIMEOUT_SET: Once = Once::new();
        TIMEOUT_SET.call_once(|| {
            // SAFETY: the system-wide element is a plain CF object; on it this
            // sets the timeout for every element.
            unsafe { AXUIElement::new_system_wide().set_messaging_timeout(TIMEOUT) };
        });
        let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let pid = front.processIdentifier();
        let editor = front
            .bundleIdentifier()
            .is_some_and(|id| CODE_EDITORS.contains(&id.to_string().as_str()));
        // SAFETY: getpid has no preconditions.
        if editor || pid == unsafe { libc::getpid() } {
            return None;
        }
        // SAFETY: a plain CF object for the app with `pid`.
        let app = unsafe { AXUIElement::new_application(pid) };
        if self.asked.insert(pid) {
            expose_web_content(&app);
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

/// Ask an Electron or Chromium app to build its accessibility tree, which
/// such apps only do for screen readers unless asked. Other apps ignore it.
fn expose_web_content(app: &AXUIElement) {
    let name = CFString::from_static_str("AXManualAccessibility");
    // SAFETY: the attribute takes a CFBoolean. Apps without it return an error, which is fine.
    unsafe { app.set_attribute_value(&name, CFBoolean::new(true)) };
}

/// A text field in another app.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    element: CFRetained<AXUIElement>,
}

impl Field {
    /// Whether this is a password field.
    pub fn is_secure(&self) -> bool {
        attribute(&self.element, "AXSubrole")
            .and_then(|v| v.downcast::<CFString>().ok())
            .is_some_and(|role| role.to_string() == "AXSecureTextField")
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
        let Ok(len) = usize::try_from(text.length()) else {
            return false;
        };
        if len > max {
            return false;
        }
        buffer.reserve(len);
        // SAFETY: CFStringGetCharacters fills exactly `len` units, which the
        // buffer has room for.
        unsafe {
            text.characters(
                CFRange {
                    location: 0,
                    length: text.length(),
                },
                buffer.as_mut_ptr(),
            );
            buffer.set_len(len);
        }
        true
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
        let param = range_value(range)?;
        let name = CFString::from_static_str("AXBoundsForRange");
        let mut out: *const CFType = ptr::null();
        // SAFETY: the attribute takes a CFRange value; `out` receives a +1 reference.
        let error = unsafe {
            self.element
                .copy_parameterized_attribute_value(&name, &param, NonNull::from(&mut out))
        };
        let value = retained(error, out)?;
        // SAFETY: a CGRect value holds a CGRect.
        let rect: CGRect = unsafe { unpack(&value, AXValueType::CGRect)? };
        (rect.size.width > 0.0 && rect.size.height > 0.0).then_some(rect)
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

/// Calls a function on the UI thread whenever the app it watches changes
/// text, caret, focus or window, until dropped. Apps that don't send these
/// are still read on a timer; this only makes the rest quick.
pub struct Watch {
    observer: CFRetained<AXObserver>,
    on_change: *mut Box<dyn Fn()>,
}

impl Watch {
    /// Watch the app with `pid`. Must run on the UI thread, whose run loop delivers the calls.
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
        CFRunLoop::main()?.add_source(Some(&source), modes);
        Some(watch)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: as in `new`.
        let (modes, source) = unsafe { (kCFRunLoopCommonModes, self.observer.run_loop_source()) };
        if let Some(main) = CFRunLoop::main() {
            main.remove_source(Some(&source), modes);
        }
        // SAFETY: made by `Box::into_raw` in `new`, and the run loop no longer
        // calls `changed` with it.
        drop(unsafe { Box::from_raw(self.on_change) });
    }
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

/// Return `element`'s attribute `name`, if it has one.
fn attribute(element: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let name = CFString::from_static_str(name);
    let mut out: *const CFType = ptr::null();
    // SAFETY: `out` receives a +1 reference on success.
    let error = unsafe { element.copy_attribute_value(&name, NonNull::from(&mut out)) };
    retained(error, out)
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
