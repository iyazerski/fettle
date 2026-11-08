// A C interface to Apple's on-device model (Foundation Models), which has only
// a Swift API. build.rs compiles this into a static library that Rust links.

import Foundation
import FoundationModels

/// Called with each new piece of the answer, as UTF-8.
public typealias OnChunk = @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>) -> Void
/// Called once at the end with an error code (0 for success) and a message.
public typealias OnDone = @convention(c) (UnsafeMutableRawPointer?, Int32, UnsafePointer<CChar>) -> Void

// The codes `fettle_on_device_status` returns.
private let ready: Int32 = 0
private let oldMacOS: Int32 = 1
private let notEligible: Int32 = 2
private let turnedOff: Int32 = 3
private let notReady: Int32 = 4

// The codes `OnDone` gets.
private let ok: Int32 = 0
private let failed: Int32 = 1
private let tooLong: Int32 = 2
private let language: Int32 = 3
private let blocked: Int32 = 4

/// Whether the model can run now, as one of the status codes above.
@_cdecl("fettle_on_device_status")
public func status() -> Int32 {
    guard #available(macOS 26, *) else { return oldMacOS }
    switch SystemLanguageModel.default.availability {
    case .available: return ready
    case .unavailable(.deviceNotEligible): return notEligible
    case .unavailable(.appleIntelligenceNotEnabled): return turnedOff
    default: return notReady
    }
}

/// A running answer, so Rust can cancel it.
final class Job {
    var task: Task<Void, Never>?
}

/// The pointers Rust passed in. Rust keeps `context` alive until `onDone`.
struct Callbacks: @unchecked Sendable {
    let context: UnsafeMutableRawPointer?
    let onChunk: OnChunk
    let onDone: OnDone

    func done(_ code: Int32, _ message: String) {
        message.withCString { onDone(context, code, $0) }
    }
}

/// Stream the answer to `prompt` under `instructions`. Returns a job that
/// `fettle_on_device_end` must release; `onDone` runs exactly once.
@_cdecl("fettle_on_device_stream")
public func stream(
    _ instructions: UnsafePointer<CChar>,
    _ prompt: UnsafePointer<CChar>,
    _ context: UnsafeMutableRawPointer?,
    _ onChunk: OnChunk,
    _ onDone: OnDone
) -> UnsafeMutableRawPointer {
    let instructions = String(cString: instructions)
    let prompt = String(cString: prompt)
    let callbacks = Callbacks(context: context, onChunk: onChunk, onDone: onDone)
    let job = Job()
    if #available(macOS 26, *) {
        job.task = Task { await run(instructions, prompt, callbacks) }
    } else {
        callbacks.done(failed, "The on-device model needs macOS 26 or later.")
    }
    return Unmanaged.passRetained(job).toOpaque()
}

/// Cancel the job if it still runs, and release it.
@_cdecl("fettle_on_device_end")
public func end(_ job: UnsafeMutableRawPointer) {
    Unmanaged<Job>.fromOpaque(job).takeRetainedValue().task?.cancel()
}

@available(macOS 26, *)
private func run(_ instructions: String, _ prompt: String, _ callbacks: Callbacks) async {
    // Fettle edits the user's own text, which the default guardrails can refuse.
    let model = SystemLanguageModel(guardrails: .permissiveContentTransformations)
    let session = LanguageModelSession(model: model, instructions: instructions)
    var sent = ""
    do {
        for try await snapshot in session.streamResponse(to: prompt) {
            // Each snapshot holds the whole answer so far; pass on the new part.
            let text = snapshot.content
            guard text.hasPrefix(sent), text.count > sent.count else { continue }
            let piece = String(text.dropFirst(sent.count))
            sent = text
            piece.withCString { callbacks.onChunk(callbacks.context, $0) }
        }
        callbacks.done(ok, "")
    } catch let error as LanguageModelSession.GenerationError {
        switch error {
        case .exceededContextWindowSize: callbacks.done(tooLong, "")
        case .unsupportedLanguageOrLocale: callbacks.done(language, "")
        case .guardrailViolation, .refusal: callbacks.done(blocked, "")
        default: callbacks.done(failed, error.localizedDescription)
        }
    } catch {
        callbacks.done(failed, error.localizedDescription)
    }
}
