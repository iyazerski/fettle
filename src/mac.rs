//! The Mac side: read the selection in the frontmost app and paste text back,
//! both through the pasteboard and synthetic ⌘C/⌘V, which need the
//! Accessibility permission.

use std::cmp::Ordering;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::thread::sleep;
use std::time::{Duration, Instant};

use objc2::AnyThread;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    NSAttributedStringAppKitDocumentFormats, NSEvent, NSPasteboard, NSPasteboardItem,
    NSPasteboardTypeString, NSPasteboardWriting, NSScreen, NSWorkspace,
};
use objc2_application_services::{AXIsProcessTrustedWithOptions, kAXTrustedCheckOptionPrompt};
use objc2_core_foundation::{CFBoolean, CFDictionary, CGPoint, CGRect};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSData, NSDictionary, NSLocale,
    NSMutableAttributedString, NSPointInRect, NSRange, NSString, NSStringCompareOptions, NSURL,
};

use crate::settings::Appearance;

/// The virtual key codes of C and V on an ANSI keyboard; other layouts keep
/// them where ⌘C and ⌘V work.
const KEY_C: u16 = 8;
const KEY_V: u16 = 9;
/// How long the frontmost app gets to copy the selection.
const COPY_WAIT: Duration = Duration::from_millis(400);
/// How long the frontmost app gets to read the pasteboard after ⌘V.
const PASTE_WAIT: Duration = Duration::from_millis(500);
/// Marks a pasteboard write as temporary, so clipboard managers skip it
/// (see nspasteboard.org).
const TRANSIENT: &str = "org.nspasteboard.TransientType";

/// Whether Fettle may send keys to other apps.
pub fn accessibility_trusted() -> bool {
    check_accessibility(false)
}

/// Whether Fettle may send keys to other apps; if not, macOS asks the user to
/// allow it (once per launch) and lists Fettle in the settings.
pub fn request_accessibility() -> bool {
    check_accessibility(true)
}

fn check_accessibility(prompt: bool) -> bool {
    // SAFETY: an immutable constant set up before any code runs.
    let key = unsafe { kAXTrustedCheckOptionPrompt };
    let options = CFDictionary::from_slices(&[key], &[CFBoolean::new(prompt)]);
    // SAFETY: the options map a CFString key to a CFBoolean, as the function expects.
    unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) }
}

/// Draw the app in `appearance`: the window's blur and the theme Rune reports
/// follow it. Does nothing off the main thread.
pub fn set_appearance(appearance: Appearance) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // SAFETY: both names are immutable constants AppKit sets up before any
    // code runs.
    let name = unsafe {
        match appearance {
            Appearance::System => None,
            Appearance::Light => Some(NSAppearanceNameAqua),
            Appearance::Dark => Some(NSAppearanceNameDarkAqua),
        }
    };
    let appearance = name.and_then(NSAppearance::appearanceNamed);
    NSApplication::sharedApplication(mtm).setAppearance(appearance.as_deref());
}

/// The pointer, in physical pixels from the primary screen's top-left, as
/// winit counts window positions.
pub fn pointer() -> Option<(f64, f64)> {
    let mtm = MainThreadMarker::new()?;
    let screens = NSScreen::screens(mtm);
    let primary = screens.iter().next()?.frame().size.height;
    let pointer = NSEvent::mouseLocation();
    let scale = screen_at(&screens, pointer.x, pointer.y)?.backingScaleFactor();
    Some((pointer.x * scale, (primary - pointer.y) * scale))
}

fn screen_at(screens: &NSArray<NSScreen>, x: f64, y: f64) -> Option<Retained<NSScreen>> {
    screens
        .iter()
        .find(|s| NSPointInRect(CGPoint::new(x, y), s.frame()))
        .or_else(|| screens.iter().next())
}

/// Where a window `width` points wide goes to sit centred near the top of the
/// screen under the pointer, `margin` points below the menu bar: its top-left
/// in physical pixels from the primary screen's top-left, as winit counts.
pub fn top_center_under_pointer(width: f32, margin: f32) -> Option<(i32, i32)> {
    let mtm = MainThreadMarker::new()?;
    let screens = NSScreen::screens(mtm);
    let primary = screens.iter().next()?.frame().size.height;
    let pointer = NSEvent::mouseLocation();
    let screen = screen_at(&screens, pointer.x, pointer.y)?;
    let area = screen.visibleFrame();
    let scale = screen.backingScaleFactor();
    // AppKit counts up from the primary screen's bottom; winit down from its top.
    let x = area.origin.x + (area.size.width - f64::from(width)) / 2.0;
    let y = primary - (area.origin.y + area.size.height) + f64::from(margin);
    Some(((x * scale).round() as i32, (y * scale).round() as i32))
}

/// Where a window `width` by `height` points goes to sit centred over `word`,
/// `gap` points above it, or under it if there's no room above: its top-left
/// in physical pixels, as winit counts. `word` is in points from the primary
/// screen's top-left, as the Accessibility API counts.
pub fn over_word(word: CGRect, width: f32, height: f32, gap: f64) -> Option<(i32, i32)> {
    let mtm = MainThreadMarker::new()?;
    let screens = NSScreen::screens(mtm);
    let primary = screens.iter().next()?.frame().size.height;
    let (width, height) = (f64::from(width), f64::from(height));
    let middle = word.origin.x + word.size.width / 2.0;
    let screen = screen_at(&screens, middle, primary - word.origin.y)?;
    let area = screen.visibleFrame();
    let scale = screen.backingScaleFactor();
    let x = (middle - width / 2.0)
        .min(area.origin.x + area.size.width - width)
        .max(area.origin.x);
    // From here on, points down from the primary screen's top, as winit counts.
    let (area_top, area_bottom) = (
        primary - (area.origin.y + area.size.height),
        primary - area.origin.y,
    );
    let above = word.origin.y - gap - height;
    let y = if above >= area_top {
        above
    } else {
        (word.origin.y + word.size.height + gap).min(area_bottom - height)
    };
    Some(((x * scale).round() as i32, (y * scale).round() as i32))
}

/// Return the `.app` bundle `exe` runs from, if it runs from one, as in
/// `Fettle.app/Contents/MacOS/fettle`.
pub fn bundle_of(exe: &Path) -> Option<&Path> {
    exe.ancestors()
        .nth(3)
        .filter(|app| app.extension().is_some_and(|e| e == "app"))
}

/// Open the Accessibility pane of System Settings.
pub fn open_accessibility_settings() {
    open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility");
}

/// Return the user's preferred languages as BCP 47 tags, most preferred first.
pub fn preferred_languages() -> Vec<String> {
    NSLocale::preferredLanguages()
        .iter()
        .map(|tag| tag.to_string())
        .collect()
}

/// Sort `items` by their `name`s as people who speak `locale`, a BCP 47 tag,
/// order words, ignoring case.
pub fn sort_localized<T>(items: &mut [T], locale: &str, name: impl Fn(&T) -> &str) {
    let locale = NSLocale::localeWithLocaleIdentifier(&NSString::from_str(locale));
    let locale: &AnyObject = locale.as_ref();
    items.sort_by(|a, b| {
        let (a, b) = (NSString::from_str(name(a)), NSString::from_str(name(b)));
        let all = NSRange::new(0, a.length());
        let options = NSStringCompareOptions::CaseInsensitiveSearch;
        // SAFETY: `locale` is an NSLocale, as the method takes.
        let order = unsafe { a.compare_options_range_locale(&b, options, all, Some(locale)) };
        Ordering::from(order)
    });
}

pub fn open_url(url: &str) {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

/// Held while Fettle borrows the pasteboard, from saving its contents to
/// putting them back, so a copy and a paste can't save each other's text as
/// the user's.
static BORROWED: Mutex<()> = Mutex::new(());

fn borrow() -> MutexGuard<'static, ()> {
    BORROWED.lock().expect("pasteboard lock poisoned")
}

/// What an app put on the pasteboard for one item: each type, by its
/// identifier, with its data.
#[derive(Debug, Clone, Default)]
pub struct Clip(pub Vec<(String, Vec<u8>)>);

impl Clip {
    /// A clip of just `text`.
    pub fn plain(text: &str) -> Self {
        Self(vec![(PLAIN.to_owned(), text.as_bytes().to_vec())])
    }
}

/// The type of UTF-8 plain text on the pasteboard.
pub const PLAIN: &str = "public.utf8-plain-text";

/// The selection in the frontmost app, as copied with ⌘C.
#[derive(Debug)]
pub struct Copied {
    pub text: String,
    /// Everything the app copied, rich formats too.
    pub clip: Clip,
}

/// The text selected in the frontmost app, copied with ⌘C. The pasteboard
/// gets its old contents back. `None` when nothing was copied: no selection,
/// no permission, or an app that ignores ⌘C. Blocks for a moment, and while
/// a paste borrows the pasteboard: call it off the main thread.
pub fn copy_selection() -> Option<Copied> {
    let _borrowed = borrow();
    let saved = Saved::take();
    let before = saved.change_count;
    post_command_key(KEY_C);
    let start = Instant::now();
    while change_count() == before && start.elapsed() < COPY_WAIT {
        sleep(Duration::from_millis(10));
    }
    let copied = change_count();
    if copied == before {
        return None;
    }
    let (text, clip) = autoreleasepool(|_| {
        // SAFETY: NSPasteboardTypeString is an immutable constant that AppKit
        // sets up before any code runs.
        let kind = unsafe { NSPasteboardTypeString };
        let pasteboard = NSPasteboard::generalPasteboard();
        let text = pasteboard.stringForType(kind).map(|s| s.to_string());
        let clip = Saved::items(&pasteboard).into_iter().next().map(|types| {
            Clip(
                types
                    .into_iter()
                    .map(|(kind, data)| (kind.to_string(), data.to_vec()))
                    .collect(),
            )
        });
        (text, clip.unwrap_or_default())
    });
    saved.restore(copied);
    let text = text.filter(|t| !t.trim().is_empty())?;
    Some(Copied { text, clip })
}

/// Paste `clip` into the frontmost app with ⌘V, then give the pasteboard its
/// old contents back. Blocks for a moment, and while a copy borrows the
/// pasteboard: call it off the main thread.
pub fn paste(clip: &Clip) {
    let _borrowed = borrow();
    let saved = Saved::take();
    let written = autoreleasepool(|_| {
        let item = NSPasteboardItem::new();
        for (kind, data) in &clip.0 {
            item.setData_forType(&NSData::with_bytes(data), &NSString::from_str(kind));
        }
        item.setData_forType(&NSData::new(), &NSString::from_str(TRANSIENT));
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let items = [ProtocolObject::from_retained(item)];
        pasteboard.writeObjects(&NSArray::from_retained_slice(&items));
        pasteboard.changeCount()
    });
    post_command_key(KEY_V);
    sleep(PASTE_WAIT);
    saved.restore(written);
}

fn change_count() -> isize {
    autoreleasepool(|_| NSPasteboard::generalPasteboard().changeCount())
}

/// Press and release ⌘ and `key`, whatever modifiers the user still holds
/// from the shortcut.
fn post_command_key(key: u16) {
    let source = CGEventSource::new(CGEventSourceStateID::Private);
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(source.as_deref(), key, down);
        CGEvent::set_flags(event.as_deref(), CGEventFlags::MaskCommand);
        CGEvent::post(CGEventTapLocation::HIDEventTap, event.as_deref());
    }
}

/// Change the text of RTF `data` with `edit`, which gets the text of each
/// run of one style, keeping each run's style. `None` if the data isn't
/// RTF or `edit` returns false. RTF, unlike HTML, may be read off the main thread.
pub fn edit_rtf(data: &[u8], edit: impl FnOnce(&mut [String]) -> bool) -> Option<Vec<u8>> {
    autoreleasepool(|_| {
        let data = NSData::with_bytes(data);
        // SAFETY: no document attributes are asked for.
        let text = unsafe {
            NSAttributedString::initWithRTF_documentAttributes(
                NSAttributedString::alloc(),
                &data,
                None,
            )
        }?;
        let string = text.string();
        let (mut texts, mut styles) = (Vec::new(), Vec::new());
        let mut at = 0;
        while at < text.length() {
            let mut range = NSRange::new(0, 0);
            // SAFETY: `at` is inside the text, and `range` is a valid pointer.
            let attributes = unsafe { text.attributesAtIndex_effectiveRange(at, &mut range) };
            texts.push(string.substringWithRange(range).to_string());
            styles.push(attributes);
            at = range.end();
        }
        if !edit(&mut texts) {
            return None;
        }
        let out = NSMutableAttributedString::new();
        for (text, attributes) in texts.iter().zip(&styles) {
            // SAFETY: the attributes came from an attributed string.
            let run = unsafe {
                NSAttributedString::initWithString_attributes(
                    NSAttributedString::alloc(),
                    &NSString::from_str(text),
                    Some(attributes),
                )
            };
            out.appendAttributedString(&run);
        }
        let range = NSRange::new(0, out.length());
        // SAFETY: no document attributes are given.
        let rtf = unsafe { out.RTFFromRange_documentAttributes(range, &NSDictionary::new()) }?;
        Some(rtf.to_vec())
    })
}

/// Every item on the pasteboard, in every type, to put back after borrowing it.
struct Saved {
    items: Vec<Vec<(Retained<NSString>, Retained<NSData>)>>,
    change_count: isize,
}

impl Saved {
    fn take() -> Self {
        autoreleasepool(|_| {
            let pasteboard = NSPasteboard::generalPasteboard();
            Self {
                items: Self::items(&pasteboard),
                change_count: pasteboard.changeCount(),
            }
        })
    }

    fn items(pasteboard: &NSPasteboard) -> Vec<Vec<(Retained<NSString>, Retained<NSData>)>> {
        pasteboard.pasteboardItems().map_or_else(Vec::new, |items| {
            items
                .iter()
                .map(|item| {
                    item.types()
                        .iter()
                        .filter_map(|kind| item.dataForType(&kind).map(|data| (kind, data)))
                        .collect()
                })
                .collect()
        })
    }

    /// Put the saved items back, unless someone wrote to the pasteboard after
    /// Fettle's own write, which left it at change count `written`.
    fn restore(self, written: isize) {
        autoreleasepool(|_| {
            let pasteboard = NSPasteboard::generalPasteboard();
            if pasteboard.changeCount() != written {
                return;
            }
            pasteboard.clearContents();
            if self.items.is_empty() {
                return;
            }
            let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = self
                .items
                .iter()
                .map(|types| {
                    let item = NSPasteboardItem::new();
                    for (kind, data) in types {
                        item.setData_forType(data, kind);
                    }
                    ProtocolObject::from_retained(item)
                })
                .collect();
            pasteboard.writeObjects(&NSArray::from_retained_slice(&items));
        });
    }
}
