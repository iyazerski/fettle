//! Winit drops most macOS system-dictation input, so Fettle listens through Speech directly.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};
use objc2_foundation::NSError;
use objc2_speech::{
    SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult, SFSpeechRecognitionTask,
    SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus,
};
use unicode_segmentation::UnicodeSegmentation;

/// The recognizer's error for a session that ended before any speech.
const NO_SPEECH: isize = 1110;

#[derive(Debug)]
pub enum Problem {
    SpeechOff,
    MicrophoneOff,
    Unavailable,
    Failed(String),
}

#[derive(Debug)]
pub enum Heard {
    /// Everything heard since the session started, as text.
    Words(String),
    /// How loud the microphone is, from 0 to 1, many times a second.
    Level(f32),
    Done,
    Failed(String),
}

/// A running session; dropping it stops listening and drops what's unheard.
pub struct Listening {
    engine: Retained<AVAudioEngine>,
    request: Retained<SFSpeechAudioBufferRecognitionRequest>,
    task: Retained<SFSpeechRecognitionTask>,
}

/// The permission callback may run on any thread.
pub fn authorize(done: impl Fn(Allowed) + Send + Sync + 'static) {
    let done = Arc::new(done);
    // SAFETY: the status is a plain class property.
    match unsafe { SFSpeechRecognizer::authorizationStatus() } {
        SFSpeechRecognizerAuthorizationStatus::Authorized => microphone(done),
        SFSpeechRecognizerAuthorizationStatus::NotDetermined => {
            let asked = RcBlock::new(move |status: SFSpeechRecognizerAuthorizationStatus| {
                if status == SFSpeechRecognizerAuthorizationStatus::Authorized {
                    microphone(Arc::clone(&done));
                } else {
                    done(Err(Problem::SpeechOff));
                }
            });
            // SAFETY: the binary's Info.plist explains the request, which
            // macOS requires; the block is copied and may run on any queue.
            unsafe { SFSpeechRecognizer::requestAuthorization(&asked) };
        }
        _ => done(Err(Problem::SpeechOff)),
    }
}

fn microphone(done: Arc<impl Fn(Allowed) + Send + Sync + 'static>) {
    // SAFETY: AVFoundation defines the audio media type for the app's life.
    let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
        return done(Err(Problem::Unavailable));
    };
    let denied = || Err(Problem::MicrophoneOff);
    // SAFETY: a class property, read with a valid media type.
    match unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) } {
        AVAuthorizationStatus::Authorized => done(Ok(())),
        AVAuthorizationStatus::NotDetermined => {
            let asked = RcBlock::new(move |granted: Bool| {
                done(if granted.as_bool() { Ok(()) } else { denied() });
            });
            // SAFETY: as for speech recognition: the Info.plist explains it,
            // and the block may run on any queue.
            unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &asked) };
        }
        _ => done(denied()),
    }
}

impl Listening {
    /// Callbacks report levels on the audio thread and words on the main thread; authorize first.
    pub fn start(heard: impl Fn(Heard) + Send + Sync + 'static) -> Result<Self, Problem> {
        let heard = Arc::new(heard);
        // SAFETY: plain Speech and AVFAudio calls on the main thread, with
        // valid objects. The recognizer reports on the main queue, its default.
        unsafe {
            let recognizer = SFSpeechRecognizer::new();
            if !recognizer.isAvailable() {
                return Err(Problem::Unavailable);
            }
            let request = SFSpeechAudioBufferRecognitionRequest::new();
            request.setShouldReportPartialResults(true);
            request.setAddsPunctuation(true);
            // On the Mac, when it can: private, and with no time limit.
            if recognizer.supportsOnDeviceRecognition() {
                request.setRequiresOnDeviceRecognition(true);
            }
            let engine = AVAudioEngine::new();
            let input = engine.inputNode();
            let format = input.outputFormatForBus(0);
            let feed = request.clone();
            let loud = heard.clone();
            let tap = RcBlock::new(
                move |buffer: NonNull<AVAudioPCMBuffer>, _: NonNull<AVAudioTime>| {
                    let buffer = buffer.as_ref();
                    feed.appendAudioPCMBuffer(buffer);
                    if let Some(level) = level(buffer) {
                        loud(Heard::Level(level));
                    }
                },
            );
            input.installTapOnBus_bufferSize_format_block(
                0,
                1024,
                Some(&format),
                RcBlock::as_ptr(&tap),
            );
            let transcript = RefCell::new(Transcript::default());
            let handler = RcBlock::new(
                move |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
                    if let Some(result) = result.as_ref() {
                        let current = result.bestTranscription().formattedString().to_string();
                        // Metadata comes with the last result before a pause.
                        let pause = result.speechRecognitionMetadata().is_some();
                        let words = transcript.borrow_mut().take(&current, pause);
                        heard(Heard::Words(words));
                        if result.isFinal() {
                            heard(Heard::Done);
                        }
                    } else if let Some(error) = error.as_ref() {
                        heard(if error.code() == NO_SPEECH {
                            Heard::Done
                        } else {
                            Heard::Failed(error.localizedDescription().to_string())
                        });
                    }
                },
            );
            let task = recognizer.recognitionTaskWithRequest_resultHandler(&request, &handler);
            engine.prepare();
            if let Err(e) = engine.startAndReturnError() {
                input.removeTapOnBus(0);
                task.cancel();
                return Err(Problem::Failed(e.localizedDescription().to_string()));
            }
            Ok(Self {
                engine,
                request,
                task,
            })
        }
    }

    /// Stop listening; the last words and `Heard::Done` follow.
    pub fn stop(&self) {
        // SAFETY: valid objects; stopping twice is harmless.
        unsafe {
            self.engine.stop();
            self.engine.inputNode().removeTapOnBus(0);
            self.request.endAudio();
        }
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.stop();
        // SAFETY: a valid task; cancelling a finished one does nothing.
        unsafe { self.task.cancel() };
    }
}

/// How loud `buffer` is, from 0 for silence to 1 for loud speech.
///
/// # Safety
/// `buffer` must be a valid buffer from the tap, alive for the call.
unsafe fn level(buffer: &AVAudioPCMBuffer) -> Option<f32> {
    // SAFETY: the tap's buffers hold float samples, `frameLength` of them in
    // each channel; the first channel is enough.
    let samples = unsafe {
        let channels = NonNull::new(buffer.floatChannelData())?;
        std::slice::from_raw_parts(channels.read().as_ptr(), buffer.frameLength() as usize)
    };
    if samples.is_empty() {
        return None;
    }
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    // Speech sits between about -50 dBFS (quiet) and -10 dBFS (loud).
    let db = 20.0 * rms.max(1e-6).log10();
    Some(((db + 50.0) / 40.0).clamp(0.0, 1.0))
}

/// Recognition restarts after pauses and may repeat the last segment or the full session on stop.
#[derive(Default)]
struct Transcript {
    said: String,
    /// The transcription kept at the last pause, until another starts.
    kept: Option<String>,
}

impl Transcript {
    fn take(&mut self, current: &str, pause: bool) -> String {
        if self
            .kept
            .as_deref()
            .is_some_and(|kept| same_words(kept, current))
        {
            return self.said.clone();
        }
        if !self.said.is_empty() && starts_with_words(current, &self.said) {
            return current.to_owned();
        }
        let words = join(&self.said, current);
        self.kept = pause.then(|| current.to_owned());
        if pause {
            self.said = words.clone();
        }
        words
    }
}

fn words(s: &str) -> impl Iterator<Item = &str> {
    s.unicode_words()
}

fn same_word(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

/// Stop at the first different word, as a new transcription after a pause usually does.
fn starts_with_words(s: &str, prefix: &str) -> bool {
    let mut words_of_s = words(s);
    words(prefix).all(|p| words_of_s.next().is_some_and(|w| same_word(w, p)))
}

fn same_words(a: &str, b: &str) -> bool {
    let (mut a, mut b) = (words(a), words(b));
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if same_word(x, y) => {}
            _ => return false,
        }
    }
}

pub fn join(before: &str, after: &str) -> String {
    match (before.trim_end(), after.trim_start()) {
        ("", after) => after.to_owned(),
        (before, "") => before.to_owned(),
        (before, after) => format!("{before} {after}"),
    }
}

type Allowed = Result<(), Problem>;

#[cfg(test)]
mod tests {
    use super::Transcript;

    #[test]
    fn keeps_each_pause_once() {
        let mut t = Transcript::default();
        let steps = [
            ("Nice", false, "Nice"),
            ("Nice it works", true, "Nice it works"),
            ("Really", false, "Nice it works Really"),
            ("Really well", true, "Nice it works Really well"),
            // Stopping repeats the last transcription as final.
            ("Really well.", false, "Nice it works Really well"),
        ];
        for (current, pause, heard) in steps {
            assert_eq!(t.take(current, pause), heard, "after {current:?}");
        }
        let mut t = Transcript::default();
        t.take("Nice it works", true);
        // Or it repeats everything.
        assert_eq!(
            t.take("Nice, it works really well.", false),
            "Nice, it works really well."
        );
    }
}
