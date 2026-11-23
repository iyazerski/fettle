use std::ptr::NonNull;

use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSBackingStoreType, NSBox, NSBoxType, NSColor, NSEvent, NSEventMask, NSFloatingWindowLevel,
    NSScreen, NSTitlePosition, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSUnionRect};
const THICKNESS: f64 = 2.0;
const GAP: f64 = 1.0;
const SLOP: f64 = 2.0;

/// `word` uses Accessibility screen points; `at` is a UTF-16 offset in the field's text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub word: CGRect,
    pub at: usize,
}

#[derive(Default)]
pub struct Underlines {
    window: Option<Retained<NSWindow>>,
    lines: Vec<Line>,
    /// The words of `lines`, in the same order, in AppKit's coordinates,
    /// which count up from the primary screen's bottom.
    words: Vec<CGRect>,
    bars: Vec<Retained<NSBox>>,
}

impl Underlines {
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
        let frame = rects
            .iter()
            .copied()
            .reduce(|a, b| NSUnionRect(a, b))
            .unwrap_or_default();
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
            let Some(rect) = rects.get(i) else {
                bar.setHidden(true);
                continue;
            };
            let origin = CGPoint::new(
                rect.origin.x - frame.origin.x,
                rect.origin.y - frame.origin.y,
            );
            bar.setFrame(CGRect::new(origin, rect.size));
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

    pub fn hit(&self, (x, y): (f64, f64)) -> Option<Line> {
        let i = self.words.iter().position(|w| {
            (w.origin.x - SLOP..=w.origin.x + w.size.width + SLOP).contains(&x)
                && (w.origin.y - GAP - THICKNESS - SLOP..=w.origin.y + w.size.height + SLOP)
                    .contains(&y)
        })?;
        self.lines.get(i).copied()
    }
}

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
    window.setLevel(NSFloatingWindowLevel);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    window
}

fn underline(mtm: MainThreadMarker) -> Retained<NSBox> {
    let bar = NSBox::initWithFrame(NSBox::alloc(mtm), CGRect::default());
    bar.setBoxType(NSBoxType::Custom);
    bar.setTitlePosition(NSTitlePosition::NoTitle);
    bar.setBorderWidth(0.0);
    bar.setCornerRadius(THICKNESS / 2.0);
    bar.setFillColor(&NSColor::systemRedColor());
    bar
}

/// Report other apps' clicks in AppKit coordinates until dropped.
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
