//! Spelling and grammar mistakes, found by macOS's own checker, in the
//! languages the user set up for it. Nothing leaves the Mac.
//!
//! Ranges count UTF-16 code units, as `NSString` does.

use std::ops::Range;
use std::ptr::NonNull;
use std::sync::Mutex;

use block2::RcBlock;
use objc2::AnyThread;
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSSpellChecker;
use objc2_foundation::{
    NSArray, NSGrammarRange, NSInteger, NSOrthography, NSRange, NSString, NSTextCheckingResult,
    NSTextCheckingType, NSValue,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Spelling,
    Grammar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mistake {
    pub range: Range<usize>,
    pub kind: Kind,
}

/// Check `text` in the background and call `done` with its mistakes, in
/// order, on some other thread.
///
/// The checker's own background API keeps the UI thread free; the synchronous
/// one is no faster.
pub fn request(text: &[u16], done: impl FnOnce(Vec<Mistake>) + Send + 'static) {
    if text.is_empty() {
        done(Vec::new());
        return;
    }
    // The block may be called only once, but its type allows more.
    let done = Mutex::new(Some(done));
    let block = RcBlock::new(
        move |_: NSInteger,
              results: NonNull<NSArray<NSTextCheckingResult>>,
              _: NonNull<NSOrthography>,
              _: NSInteger| {
            // SAFETY: the checker passes a valid array for the call's duration.
            let found = mistakes(unsafe { results.as_ref() });
            let done = done.lock().expect("checker callback lock poisoned").take();
            if let Some(done) = done {
                done(found);
            }
        },
    );
    autoreleasepool(|_| {
        // SAFETY: `text` is valid for `text.len()` units; NSString copies them.
        let string = unsafe {
            NSString::initWithCharacters_length(
                NSString::alloc(),
                NonNull::from(text).cast(),
                text.len(),
            )
        };
        // SAFETY: no options; the block outlives the request, as the checker retains it.
        unsafe {
            NSSpellChecker::sharedSpellChecker()
                .requestCheckingOfString_range_types_options_inSpellDocumentWithTag_completionHandler(
                    &string,
                    NSRange::new(0, text.len()),
                    (NSTextCheckingType::Spelling | NSTextCheckingType::Grammar).bits(),
                    None,
                    0,
                    Some(&block),
                );
        }
    });
}

fn mistakes(results: &NSArray<NSTextCheckingResult>) -> Vec<Mistake> {
    let mut found = Vec::new();
    for result in results.iter() {
        let range = result.range();
        if result.resultType() == NSTextCheckingType::Spelling {
            found.push(mistake(range, 0, Kind::Spelling));
            continue;
        }
        // A grammar result covers the sentence; its details point at the
        // words, counting from the sentence's start.
        let details = result.grammarDetails().unwrap_or_default();
        let before = found.len();
        for detail in details.iter() {
            // SAFETY: NSGrammarRange is an immutable constant set up before any code runs.
            let key = unsafe { NSGrammarRange };
            if let Some(value) = detail.objectForKey(key)
                && let Ok(value) = value.downcast::<NSValue>()
            {
                // SAFETY: the grammar range is an NSRange.
                let detail = unsafe { value.rangeValue() };
                found.push(mistake(detail, range.location, Kind::Grammar));
            }
        }
        if found.len() == before {
            found.push(mistake(range, 0, Kind::Grammar));
        }
    }
    found.sort_by_key(|m| m.range.start);
    found
}

fn mistake(range: NSRange, offset: usize, kind: Kind) -> Mistake {
    let start = offset + range.location;
    Mistake {
        range: start..start + range.length,
        kind,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    fn found(text: &str) -> Vec<(String, Kind)> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let (tx, rx) = mpsc::channel();
        request(&units, move |found| tx.send(found).unwrap());
        let found = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        found
            .into_iter()
            .map(|m| (String::from_utf16_lossy(&units[m.range]), m.kind))
            .collect()
    }

    #[test]
    fn finds_misspelled_words_and_bad_grammar() {
        assert_eq!(found("Café is fine."), []);
        assert_eq!(found(""), []);
        let mistakes = found("Café is nice. They was at the libary.");
        assert!(
            mistakes.contains(&("libary".into(), Kind::Spelling)),
            "{mistakes:?}"
        );
        assert!(
            mistakes
                .iter()
                .any(|(text, kind)| *kind == Kind::Grammar && text == "was"),
            "{mistakes:?}"
        );
    }
}
