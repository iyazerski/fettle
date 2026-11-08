//! Underlines under mistakes in another app's text: a borderless window over
//! them that clicks pass through, with a thin bar under each word, and a
//! watch on clicks so a click on an underlined word can open Fettle.

use std::ptr::NonNull;

use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSBackingStoreType, NSBox, NSBoxType, NSColor, NSEvent, NSEventMask, NSScreen, NSTitlePosition,
    NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::MainThreadMarker;

use crate::spelling::Kind;

/// NSFloatingWindowLevel: over the app's windows, as Fettle's own window is.
const FLOATING: isize = 3;
/// The bar's thickness, and how far under the word it sits, in points.
const THICKNESS: f64 = 2.0;
const GAP: f64 = 1.0;
/// How far around a word a click still counts as on it, in points.
const SLOP: f64 = 2.0;

/// An underlined word: where it is, in screen points from the primary
/// screen's top-left, as the Accessibility API counts, what's wrong, and
/// where its mistake starts in the text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub word: CGRect,
    pub kind: Kind,
    pub at: usize,
}

/// The window that draws the lines, made when first needed.
#[derive(Default)]
pub struct Underlines {
    window: Option<Retained<NSWindow>>,
    lines: Vec<Line>,
    /// The words of `lines`, in the same order, in AppKit's coordinates,
    /// which count up from the primary screen's bottom.
    words: Vec<CGRect>,
    /// Bars made so far, reused as lines come and go; the extra ones hide.
    bars: Vec<Retained<NSBox>>,
}

impl Underlines {
    /// Underline `lines`, and nothing else. Does nothing off the main thread.
    pub fn show(&mut self, lines: Vec<Line>) {
        if lines == self.lines {
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if lines.is_empty() {
            self.hide();
            return;
        }
        let Some(primary) = NSScreen::screens(mtm).iter().next() else {
            return;
        };
        let height = primary.frame().size.height;
        self.words = lines
            .iter()
            .map(|l| {
                let w = l.word;
                CGRect::new(
                    CGPoint::new(w.origin.x, height - w.origin.y - w.size.height),
                    w.size,
                )
            })
            .collect();
        let rects: Vec<CGRect> = self
            .words
            .iter()
            .map(|w| {
                CGRect::new(
                    CGPoint::new(w.origin.x, w.origin.y - GAP - THICKNESS),
                    CGSize::new(w.size.width, THICKNESS),
                )
            })
            .collect();
        let frame = rects.iter().copied().reduce(union).unwrap_or_default();
        let window = self.window.get_or_insert_with(|| overlay(mtm));
        let Some(content) = window.contentView() else {
            return;
        };
        window.setFrame_display(frame, false);
        while self.bars.len() < rects.len() {
            let bar = underline(mtm);
            content.addSubview(&bar);
            self.bars.push(bar);
        }
        for (i, bar) in self.bars.iter().enumerate() {
            let Some((rect, line)) = rects.get(i).zip(lines.get(i)) else {
                bar.setHidden(true);
                continue;
            };
            let origin = CGPoint::new(
                rect.origin.x - frame.origin.x,
                rect.origin.y - frame.origin.y,
            );
            bar.setFrame(CGRect::new(origin, rect.size));
            bar.setFillColor(&color(line.kind));
            bar.setHidden(false);
        }
        window.orderFrontRegardless();
        self.lines = lines;
    }

    pub fn hide(&mut self) {
        if let Some(window) = &self.window {
            window.orderOut(None);
        }
        self.lines.clear();
        self.words.clear();
    }

    /// Return the line whose word is at `point`, in AppKit's coordinates.
    pub fn hit(&self, (x, y): (f64, f64)) -> Option<Line> {
        let i = self.words.iter().position(|w| {
            (w.origin.x - SLOP..=w.origin.x + w.size.width + SLOP).contains(&x)
                && (w.origin.y - GAP - THICKNESS - SLOP..=w.origin.y + w.size.height + SLOP)
                    .contains(&y)
        })?;
        self.lines.get(i).copied()
    }
}

/// A borderless, transparent window that never takes clicks or focus, on every Space.
fn overlay(mtm: MainThreadMarker) -> Retained<NSWindow> {
    // SAFETY: a plain borderless window, kept alive by `Underlines`.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            CGRect::default(),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: `Underlines` owns the window and never closes it.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setHasShadow(false);
    window.setIgnoresMouseEvents(true);
    window.setLevel(FLOATING);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    window
}

/// A rounded bar.
fn underline(mtm: MainThreadMarker) -> Retained<NSBox> {
    let bar = NSBox::initWithFrame(NSBox::alloc(mtm), CGRect::default());
    bar.setBoxType(NSBoxType::Custom);
    bar.setTitlePosition(NSTitlePosition::NoTitle);
    bar.setBorderWidth(0.0);
    bar.setCornerRadius(THICKNESS / 2.0);
    bar
}

/// Red for spelling and green for grammar, as macOS marks them.
fn color(kind: Kind) -> Retained<NSColor> {
    match kind {
        Kind::Spelling => NSColor::systemRedColor(),
        Kind::Grammar => NSColor::systemGreenColor(),
    }
}

fn union(a: CGRect, b: CGRect) -> CGRect {
    let x = a.origin.x.min(b.origin.x);
    let y = a.origin.y.min(b.origin.y);
    let right = (a.origin.x + a.size.width).max(b.origin.x + b.size.width);
    let top = (a.origin.y + a.size.height).max(b.origin.y + b.size.height);
    CGRect::new(CGPoint::new(x, y), CGSize::new(right - x, top - y))
}

/// Calls a function with the pointer, in AppKit's coordinates, on each left
/// click in another app, until dropped.
pub struct Clicks(Retained<AnyObject>);

impl Clicks {
    pub fn watch(on_click: impl Fn((f64, f64)) + 'static) -> Option<Self> {
        let block = block2::RcBlock::new(move |_: NonNull<NSEvent>| {
            let at = NSEvent::mouseLocation();
            on_click((at.x, at.y));
        });
        NSEvent::addGlobalMonitorForEventsMatchingMask_handler(NSEventMask::LeftMouseDown, &block)
            .map(Self)
    }
}

impl Drop for Clicks {
    fn drop(&mut self) {
        // SAFETY: the monitor came from `addGlobalMonitorForEventsMatchingMask_handler`.
        unsafe { NSEvent::removeMonitor(&self.0) };
    }
}
