//! The Mac side: read the selection in the frontmost app and paste text back,
//! both through the pasteboard and synthetic ⌘C/⌘V, which need the
//! Accessibility permission.

use std::thread::sleep;
use std::time::{Duration, Instant};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSEvent,
    NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting, NSScreen,
    NSWorkspace,
};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSData, NSDictionary, NSNumber, NSString, NSURL,
};

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

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const NSDictionary<NSString, NSNumber>) -> bool;
}

/// Whether Quill may send keys to other apps. With `prompt`, macOS asks the
/// user to allow it (once per launch) and lists Quill in the settings.
pub fn accessibility_trusted(prompt: bool) -> bool {
    let key = NSString::from_str("AXTrustedCheckOptionPrompt");
    let value = NSNumber::new_bool(prompt);
    let options = NSDictionary::from_slices(&[&*key], &[&*value]);
    // SAFETY: the dictionary is a valid CFDictionary (toll-free bridged) that
    // outlives the call.
    unsafe { AXIsProcessTrustedWithOptions(&*options) }
}

/// Draw the app light or dark (`None` follows macOS): the window's blur and
/// the theme Rune reports follow it. Does nothing off the main thread.
pub fn set_dark(dark: Option<bool>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let appearance = dark.and_then(|dark| {
        // SAFETY: both names are immutable constants AppKit sets up before any
        // code runs.
        let name = unsafe {
            if dark {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            }
        };
        NSAppearance::appearanceNamed(name)
    });
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
        .find(|s| {
            let f = s.frame();
            (f.origin.x..f.origin.x + f.size.width).contains(&x)
                && (f.origin.y..f.origin.y + f.size.height).contains(&y)
        })
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

/// Open the Accessibility pane of System Settings.
pub fn open_accessibility_settings() {
    open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility");
}

pub fn open_url(url: &str) {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

/// The text selected in the frontmost app, copied with ⌘C. The pasteboard
/// gets its old contents back. `None` when nothing was copied: no selection,
/// no permission, or an app that ignores ⌘C.
pub fn copy_selection() -> Option<String> {
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
    let text = autoreleasepool(|_| {
        // SAFETY: NSPasteboardTypeString is an immutable constant that AppKit
        // sets up before any code runs.
        let kind = unsafe { NSPasteboardTypeString };
        NSPasteboard::generalPasteboard()
            .stringForType(kind)
            .map(|s| s.to_string())
    });
    saved.restore(copied);
    text.filter(|t| !t.trim().is_empty())
}

/// Paste `text` into the frontmost app with ⌘V, then give the pasteboard its
/// old contents back. Blocks for a moment: call it off the main thread.
pub fn paste(text: &str) {
    let saved = Saved::take();
    let written = autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        // SAFETY: as in `copy_selection`.
        let kind = unsafe { NSPasteboardTypeString };
        pasteboard.setString_forType(&NSString::from_str(text), kind);
        pasteboard.setString_forType(&NSString::from_str(""), &NSString::from_str(TRANSIENT));
        pasteboard.changeCount()
    });
    post_command_key(KEY_V);
    sleep(PASTE_WAIT);
    saved.restore(written);
}

/// Put `text` on the pasteboard for good.
pub fn copy(text: &str) {
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        // SAFETY: as in `copy_selection`.
        let kind = unsafe { NSPasteboardTypeString };
        pasteboard.setString_forType(&NSString::from_str(text), kind);
    });
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

/// Every item on the pasteboard, in every type, to put back after borrowing it.
struct Saved {
    items: Vec<Vec<(Retained<NSString>, Retained<NSData>)>>,
    change_count: isize,
}

impl Saved {
    fn take() -> Self {
        autoreleasepool(|_| {
            let pasteboard = NSPasteboard::generalPasteboard();
            let items = pasteboard.pasteboardItems().map_or_else(Vec::new, |items| {
                items
                    .iter()
                    .map(|item| {
                        item.types()
                            .iter()
                            .filter_map(|kind| item.dataForType(&kind).map(|data| (kind, data)))
                            .collect()
                    })
                    .collect()
            });
            Self {
                items,
                change_count: pasteboard.changeCount(),
            }
        })
    }

    /// Put the saved items back, unless someone wrote to the pasteboard after
    /// Quill's own write, which left it at change count `written`.
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
