//! The menu bar item: Fettle's sparkles in the menu bar, with a menu to open
//! the window, open the settings, or quit.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSObject, NSString};
use rune::AppHandle;

use super::state::State;
use crate::i18n::Strings;

/// The SF Symbol it shows: the sparkles of Fettle's icon.
/// Fettle Dev's differs, to tell the two apart.
const SYMBOL: &str = if crate::DEV { "hammer" } else { "sparkles" };

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Target` doesn't implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FettleMenuBarTarget"]
    #[ivars = AppHandle<State>]
    struct Target;

    impl Target {
        #[unsafe(method(openFettle:))]
        fn open_fettle(&self, _sender: Option<&AnyObject>) {
            self.ivars().update(|s, _| s.open_from_menu_bar());
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            self.ivars().update(|s, _| s.open_settings());
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            self.ivars().quit();
        }
    }
);

/// The item in the menu bar, there until dropped.
pub struct MenuBar {
    mtm: MainThreadMarker,
    item: Retained<NSStatusItem>,
    /// The menu items' target, which they don't retain.
    target: Retained<Target>,
}

impl MenuBar {
    /// Put the item in the menu bar, its menu labelled in `t`'s language.
    /// `None` off the main thread.
    pub fn new(app: AppHandle<State>, t: &Strings) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let target: Retained<Target> = {
            let this = Target::alloc(mtm).set_ivars(app);
            // SAFETY: NSObject's init on an allocated object with its ivars set.
            unsafe { msg_send![super(this), init] }
        };
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = item.button(mtm) {
            let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(SYMBOL),
                Some(&NSString::from_str(crate::NAME)),
            );
            button.setImage(image.as_deref());
        }
        let bar = Self { mtm, item, target };
        bar.set_labels(t);
        Some(bar)
    }

    /// Label the menu in `t`'s language.
    pub fn set_labels(&self, t: &Strings) {
        let menu = NSMenu::new(self.mtm);
        menu.addItem(&self.menu_item(t.open_fettle, sel!(openFettle:)));
        menu.addItem(&self.menu_item(t.settings, sel!(openSettings:)));
        menu.addItem(&NSMenuItem::separatorItem(self.mtm));
        menu.addItem(&self.menu_item(t.quit, sel!(quit:)));
        self.item.setMenu(Some(&menu));
    }

    /// Make a menu item titled `title` that sends `action` to the target.
    fn menu_item(&self, title: &str, action: Sel) -> Retained<NSMenuItem> {
        // SAFETY: `action` is one of `Target`'s methods, and `self.target`
        // outlives the menu, which goes with `self.item`.
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(self.mtm),
                &NSString::from_str(title),
                Some(action),
                &NSString::from_str(""),
            );
            item.setTarget(Some(&self.target));
            item
        }
    }
}

impl Drop for MenuBar {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}
