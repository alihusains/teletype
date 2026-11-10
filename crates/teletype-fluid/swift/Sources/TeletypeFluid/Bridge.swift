// TeletypeFluid — C ABI bridge to FluidAudio's Parakeet Core ML stack.
//
// Rust side: `crates/teletype-fluid/src/bridge.rs`. The signatures and the
// return-code contract below are mirrored there; change both together.
//
//   ttf_init(version, modelsDir, err, errCap) -> Int32
//   ttf_is_ready() -> Int32
//   ttf_set_vocab(json) -> Int32
//   ttf_transcribe(pcm, n, lang, out, outCap) -> Int32
//   ttf_unload()
//   ttf_last_error(out, outCap) -> Int32      // additive, not in plan §3.1
//
// Nemotron 3.5 streaming (independent session, see the MARK section below):
//
//   ttf_nemotron_init(modelsDir, chunkMs, err, errCap) -> Int32
//   ttf_nemotron_is_ready() -> Int32
//   ttf_nemotron_transcribe(pcm, n, lang, out, outCap) -> Int32
//   ttf_nemotron_unload()
//   ttf_nemotron_last_error(out, outCap) -> Int32
//
// Return contract: 0 = success; `ttf_transcribe` and
// `ttf_nemotron_transcribe` return the needed output buffer size as a
// positive number when the caller's buffer is too small (retry with that
// capacity); -1 = failure (detail via the matching `ttf_*_last_error`, plus
// the `err` buffer for the init entry points). All entry points block and
// must not run on the main thread (plan §3.1 rule).
//
// Model storage: Parakeet models live under `<modelsDir>/<repo folder>/`,
// where `<modelsDir>` is the app's `models/fluid` directory. Vocabulary
// boosting's CTC models are the exception — FluidAudio's
// `VocabularyBoostingSession` hardcodes `CtcModels.defaultCacheDirectory` for
// its tokenizer and rescorer, so those stay in FluidAudio's own Application
// Support folder (recorded in PROGRESS.md, P3-A).

import Foundation
import CoreML
import FluidAudio
import os.log

private let log = Logger(subsystem: "com.teletype.app", category: "fluid-bridge")

// MARK: - Process-wide session state

/// All mutable bridge state, guarded by `lock`. FluidAudio serializes
/// inference inside its `AsrManager` actor; this lock only protects our own
/// references and the last-error string.
private final class BridgeState: @unchecked Sendable {
    let lock = NSLock()
    var manager: AsrManager?
    var boostSession: VocabularyBoostingSession?
    var lastError = ""
    /// Everything needed to reload the ASR models on a different encoder
    /// compute unit after a Core ML specialization failure (G038).
    var asrVersion: AsrModelVersion?
    var asrBase: URL?
    var encoderUnits = MLComputeUnits.cpuAndNeuralEngine
    var faultFired = false
}

private let bridge = BridgeState()

private func withBridge<T>(_ body: () -> T) -> T {
    bridge.lock.lock()
    defer { bridge.lock.unlock() }
    return body()
}

// MARK: - Helpers

private enum BridgeFailure: Error, CustomStringConvertible {
    case message(String)

    var description: String {
        switch self {
        case .message(let text): return text
        }
    }
}

/// Copy `text` into `buf` as UTF-8 with a trailing NUL.
///
/// - Returns: bytes written (including the NUL) when the text fits, otherwise
///   the capacity the caller must provide (always ≥ 1).
private func copyOut(_ text: String, to buf: UnsafeMutablePointer<CChar>, cap: Int) -> Int {
    var bytes = Array(text.utf8)
    let needed = bytes.count + 1
    guard cap >= needed else { return needed }
    bytes.append(0)
    bytes.withUnsafeBufferPointer { source in
        if let base = source.baseAddress {
            _ = memcpy(buf, base, needed)
        }
    }
    return needed
}

/// Record `message` as the last bridge error, log it, and return -1. When the
/// caller provided an error buffer (`ttf_init`), the message is also written
/// there.
private func fail(
    _ err: UnsafeMutablePointer<CChar>?,
    _ errCap: Int,
    _ message: String
) -> Int32 {
    withBridge { bridge.lastError = message }
    log.error("\(message, privacy: .public)")
    if let err {
        _ = copyOut(message, to: err, cap: errCap)
    }
    return -1
}

/// Box carrying a detached task's result back to the blocked caller.
private final class ResultBox<T>: @unchecked Sendable {
    var value: Result<T, Error>?
}

/// Run `body` to completion on a detached task, blocking the calling thread.
/// The caller must not be the main thread (documented bridge contract).
private func runBlocking<T>(_ body: @escaping @Sendable () async throws -> T) throws -> T {
    let done = DispatchSemaphore(value: 0)
    let box = ResultBox<T>()
    Task.detached(priority: .userInitiated) {
        do {
            box.value = .success(try await body())
        } catch {
            box.value = .failure(error)
        }
        done.signal()
    }
    done.wait()
    guard let value = box.value else {
        throw BridgeFailure.message("internal error: async bridge produced no result")
    }
    return try value.get()
}

private func asrVersion(_ raw: String) -> AsrModelVersion? {
    switch raw {
    case "ultra": return .ultra
    case "v3": return .v3
    case "v2": return .v2
    default: return nil
    }
}

/// Local repository folder FluidAudio caches `version` in under a models base.
private func repoFolder(_ version: AsrModelVersion) -> String {
    switch version {
    case .ultra: return Repo.parakeetUltra.folderName
    case .v3: return Repo.parakeetV3.folderName
    case .v2: return Repo.parakeetV2.folderName
    default: return Repo.parakeetV3.folderName
    }
}

/// Language hint for script-aware token filtering (v3 family). `nil` disables
/// the hint — FluidAudio does not report a detected language back.
private func languageHint(_ code: String) -> Language? {
    let trimmed = code.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    if trimmed.isEmpty || trimmed == "auto" { return nil }
    let primary = trimmed.split(separator: "-").first.map(String.init) ?? trimmed
    return Language(rawValue: primary)
}

/// Load (downloading when needed) the Parakeet models into an `AsrManager`.
///
/// `encoderUnits` selects the compute device for the conformer encoder.
/// `.cpuAndNeuralEngine` (FluidAudio's default) is the fastest arm measured on
/// macOS 27 / M4 Pro: 37 ms p50 over 100 live AMI clips, versus 86 ms on
/// `.cpuOnly` and 393 ms on `.cpuAndGPU`. It is also the arm that
/// intermittently fails ANE specialization (G038), so callers must be able to
/// reload the same models on `.cpuOnly` and retry, which is why this is a
/// parameter and not a constant.
private func loadASR(
    version: AsrModelVersion,
    base: URL,
    encoderUnits: MLComputeUnits
) async throws -> AsrManager {
    let folder = base.appendingPathComponent(repoFolder(version), isDirectory: true)
    try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)

    let models: AsrModels
    if AsrModels.modelsExist(at: folder, version: version) {
        log.info("fluid: loading \(repoFolder(version), privacy: .public) from \(folder.path, privacy: .public)")
        models = try await AsrModels.load(from: folder, version: version, encoderComputeUnits: encoderUnits)
    } else {
        log.info("fluid: downloading \(repoFolder(version), privacy: .public) to \(base.path, privacy: .public)")
        models = try await AsrModels.downloadAndLoad(to: folder, version: version, encoderComputeUnits: encoderUnits)
    }

    let manager = AsrManager()
    try await manager.loadModels(models)
    return manager
}

// MARK: - Transcript JSON

private struct OutToken: Codable {
    let token: String
    let start_ms: Int
    let end_ms: Int
    let conf: Float
}

private struct OutWord: Codable {
    let text: String
    let start_ms: Int
    let end_ms: Int
    let conf: Float
}

private struct OutTranscript: Codable {
    let text: String
    let confidence: Float
    let duration_ms: Int
    let words: [OutWord]
    let tokens: [OutToken]
    let detected_terms: [String]
    let applied_terms: [String]
    /// Detected language (BCP-47), when the engine reports one. The Parakeet
    /// path never reports a detection and leaves this null; the Nemotron
    /// path fills it from the model's own detector.
    let language: String?
}

private func milliseconds(_ seconds: TimeInterval) -> Int {
    max(0, Int((seconds * 1000).rounded()))
}

/// Group sentence-piece token timings into words. A token that starts with
/// the sentence-piece word boundary (`▁`) or a space opens a new word.
private func words(from timings: [TokenTiming]) -> [OutWord] {
    var result: [OutWord] = []
    var current = ""
    var start: TimeInterval = 0
    var end: TimeInterval = 0
    var confidence: Float = 0
    var members = 0

    func flush() {
        guard !current.isEmpty else { return }
        result.append(OutWord(
            text: current,
            start_ms: milliseconds(start),
            end_ms: milliseconds(end),
            conf: members > 0 ? confidence / Float(members) : 0
        ))
        current = ""
        confidence = 0
        members = 0
    }

    for timing in timings {
        let boundary = timing.token.hasPrefix("\u{2581}") || timing.token.hasPrefix(" ")
        let piece = timing.token
            .replacingOccurrences(of: "\u{2581}", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if boundary, !current.isEmpty { flush() }
        if current.isEmpty { start = timing.startTime }
        current += piece
        end = timing.endTime
        confidence += timing.confidence
        members += 1
    }
    flush()
    return result
}

private func encodeJSON(
    text: String,
    confidence: Float,
    duration: TimeInterval,
    timings: [TokenTiming],
    detected: [String],
    applied: [String],
    language: String? = nil
) throws -> String {
    let payload = OutTranscript(
        text: text,
        confidence: confidence,
        duration_ms: milliseconds(duration),
        words: words(from: timings),
        tokens: timings.map {
            OutToken(
                token: $0.token,
                start_ms: milliseconds($0.startTime),
                end_ms: milliseconds($0.endTime),
                conf: $0.confidence
            )
        },
        detected_terms: detected,
        applied_terms: applied,
        language: language
    )
    let data = try JSONEncoder().encode(payload)
    guard let json = String(data: data, encoding: .utf8) else {
        throw BridgeFailure.message("transcript JSON is not valid UTF-8")
    }
    return json
}

// MARK: - C ABI

/// Decode one PCM buffer with the loaded `AsrManager`, returning the
/// transcript JSON. Split out of `ttf_transcribe` so the same take can be
/// retried against a reloaded engine after an encoder specialization failure.
private func runTranscribe(
    manager: AsrManager,
    boost: VocabularyBoostingSession?,
    samples: [Float],
    languageCode: String
) throws -> String {
    try runBlocking { () async throws -> String in
        var state = TdtDecoderState.make(decoderLayers: await manager.decoderLayerCount)
        let result = try await manager.transcribe(
            samples,
            decoderState: &state,
            language: languageHint(languageCode)
        )

        var text = result.text
        var detected: [String] = []
        var applied: [String] = []
        if let boost, let timings = result.tokenTimings, !timings.isEmpty {
            if let rescored = await boost.rescore(
                text: text, tokenTimings: timings, audioSamples: samples
            ) {
                text = rescored.text
                detected = rescored.detectedTerms
                applied = rescored.replacements
                    .filter(\.shouldReplace)
                    .compactMap(\.replacementWord)
            }
        }

        return try encodeJSON(
            text: text,
            confidence: result.confidence,
            duration: result.duration,
            timings: result.tokenTimings ?? [],
            detected: detected,
            applied: applied
        )
    }
}

/// True for the Core ML error raised when the encoder cannot be specialized
/// for its compute devices. FluidAudio raises it as an `NSError` from the
/// `MLModel` compile/specialize step (`com.apple.CoreML` Code 3, with an
/// `ANECCompile() FAILED` underlayer) and it surfaces at transcribe time, not
/// at load time, so it is matched on the rendered message.
private func isEncoderSpecializationFailure(_ error: Error) -> Bool {
    let message = String(describing: error)
    return message.contains("Failed to specialize the model")
        || message.contains("ANECCompile() FAILED")
}

/// Fault injection for the recovery path above. `TELETYPE_FLUID_FAULT=specialize`
/// makes the first decode of the session fail exactly the way an ANE
/// specialization failure does, so the reload-and-retry can be exercised on
/// hardware where the real failure is intermittent. Fires once, so the retry
/// runs against a healthy engine; inert when the variable is unset.
private func injectedSpecializationFault() -> Error? {
    guard ProcessInfo.processInfo.environment["TELETYPE_FLUID_FAULT"] == "specialize" else {
        return nil
    }
    let shouldFire = withBridge { () -> Bool in
        if bridge.faultFired { return false }
        bridge.faultFired = true
        return true
    }
    guard shouldFire else { return nil }
    return NSError(
        domain: "com.apple.CoreML",
        code: 3,
        userInfo: [NSLocalizedDescriptionKey:
            "Failed to specialize the model Encoder for the compute device(s): CPU, NeuralEngine. It can be an unsupported model."]
    )
}

@_cdecl("ttf_init")
public func ttf_init(
    _ version: UnsafePointer<CChar>,
    _ modelsDir: UnsafePointer<CChar>,
    _ err: UnsafeMutablePointer<CChar>,
    _ errCap: Int
) -> Int32 {
    let rawVersion = String(cString: version)
    guard let requested = asrVersion(rawVersion) else {
        return fail(err, errCap,
            "ttf_init: unknown FluidAudio model version '\(rawVersion)' (expected ultra|v3|v2)")
    }
    let base = URL(fileURLWithPath: String(cString: modelsDir), isDirectory: true)
    let units = withBridge { bridge.encoderUnits }
    do {
        let manager = try runBlocking {
            try await loadASR(version: requested, base: base, encoderUnits: units)
        }
        withBridge {
            bridge.manager = manager
            bridge.boostSession = nil
            bridge.lastError = ""
            bridge.asrVersion = requested
            bridge.asrBase = base
            bridge.encoderUnits = units
        }
        return 0
    } catch {
        return fail(err, errCap, "ttf_init: \(error)")
    }
}

@_cdecl("ttf_is_ready")
public func ttf_is_ready() -> Int32 {
    withBridge { bridge.manager != nil ? 1 : 0 }
}

@_cdecl("ttf_set_vocab")
public func ttf_set_vocab(_ json: UnsafePointer<CChar>) -> Int32 {
    struct Term: Codable {
        let text: String
        let aliases: [String]?
        let weight: Float?
    }

    let raw = String(cString: json)
    let terms: [Term]
    do {
        terms = try JSONDecoder().decode([Term].self, from: Data(raw.utf8))
    } catch {
        return fail(nil, 0, "ttf_set_vocab: bad JSON: \(error)")
    }

    if terms.isEmpty {
        // Clearing the list needs no CTC models at all.
        withBridge { bridge.boostSession = nil }
        return 0
    }

    let ready = withBridge { bridge.manager != nil }
    guard ready else {
        return fail(nil, 0, "ttf_set_vocab: ASR models are not loaded")
    }

    do {
        let session = try runBlocking { () async throws -> VocabularyBoostingSession in
            let vocabulary = CustomVocabularyContext(
                terms: terms.map {
                    CustomVocabularyTerm(text: $0.text, weight: $0.weight, aliases: $0.aliases)
                }
            )
            // CTC models stay in FluidAudio's default cache directory; the
            // session API hardcodes it (see the file header).
            let ctc = try await CtcModels.downloadAndLoad()
            return try await VocabularyBoostingSession(vocabulary: vocabulary, ctcModels: ctc)
        }
        withBridge { bridge.boostSession = session }
        return 0
    } catch {
        return fail(nil, 0, "ttf_set_vocab: \(error)")
    }
}

@_cdecl("ttf_transcribe")
public func ttf_transcribe(
    _ pcm: UnsafePointer<Float>,
    _ sampleCount: Int,
    _ lang: UnsafePointer<CChar>,
    _ out: UnsafeMutablePointer<CChar>,
    _ outCap: Int
) -> Int32 {
    guard sampleCount > 0 else {
        return fail(nil, 0, "ttf_transcribe: empty audio")
    }
    let samples = Array(UnsafeBufferPointer(start: pcm, count: sampleCount))
    let languageCode = String(cString: lang)
    let snapshot = withBridge {
        (bridge.manager, bridge.boostSession, bridge.asrVersion, bridge.asrBase)
    }
    guard let manager = snapshot.0 else {
        return fail(nil, 0, "ttf_transcribe: ASR models are not loaded")
    }
    let boost = snapshot.1

    do {
        let json: String
        do {
            if let fault = injectedSpecializationFault() { throw fault }
            json = try runTranscribe(
                manager: manager, boost: boost, samples: samples, languageCode: languageCode
            )
        } catch let specializationError where isEncoderSpecializationFailure(specializationError) {
            // The take must survive an encoder that cannot be specialized for
            // its compute devices (G038). Reload the same models on CPU and
            // decode the audio we already hold; the sample buffer is still
            // valid here, so nothing captured is lost.
            guard let version = snapshot.2, let base = snapshot.3,
                  withBridge({ bridge.encoderUnits }) != .cpuOnly
            else { throw specializationError }
            log.warning("fluid: encoder specialization failed, retrying this take on CPU")
            let reloaded = try runBlocking {
                try await loadASR(version: version, base: base, encoderUnits: .cpuOnly)
            }
            withBridge {
                bridge.manager = reloaded
                bridge.encoderUnits = .cpuOnly
            }
            json = try runTranscribe(
                manager: reloaded, boost: boost, samples: samples, languageCode: languageCode
            )
        }
        let written = copyOut(json, to: out, cap: outCap)
        return written <= outCap ? 0 : Int32(written)
    } catch {
        return fail(nil, 0, "ttf_transcribe: \(error)")
    }
}

@_cdecl("ttf_unload")
public func ttf_unload() {
    withBridge {
        bridge.manager = nil
        bridge.boostSession = nil
    }
}

@_cdecl("ttf_last_error")
public func ttf_last_error(_ out: UnsafeMutablePointer<CChar>, _ outCap: Int) -> Int32 {
    let message = withBridge { bridge.lastError }
    if message.isEmpty { return 0 }
    return Int32(copyOut(message, to: out, cap: outCap))
}

// MARK: - Nemotron 3.5 streaming (second, independent session)

/// All mutable Nemotron bridge state, guarded by `lock`.
///
/// Deliberately separate from the Parakeet `bridge` state: the two models
/// have independent load/unload lifecycles and can be resident at the same
/// time, and a failure in one must not clobber the other's session or
/// last-error string. The manager serializes its own inference; this lock
/// only protects our reference and the last-error string, exactly as
/// `BridgeState` does for Parakeet.
private final class NemotronState: @unchecked Sendable {
    let lock = NSLock()
    var manager: StreamingNemotronMultilingualAsrManager?
    var lastError = ""
}

private let nemotron = NemotronState()

private func withNemotron<T>(_ body: () -> T) -> T {
    nemotron.lock.lock()
    defer { nemotron.lock.unlock() }
    return body()
}

/// Record `message` as the last Nemotron bridge error, log it, and return -1.
/// When the caller provided an error buffer (`ttf_nemotron_init`), the
/// message is also written there.
private func failNemotron(
    _ err: UnsafeMutablePointer<CChar>?,
    _ errCap: Int,
    _ message: String
) -> Int32 {
    withNemotron { nemotron.lastError = message }
    log.error("nemotron: \(message, privacy: .public)")
    if let err {
        _ = copyOut(message, to: err, cap: errCap)
    }
    return -1
}

/// Language hint for `setLanguage`. `nil` means "the model decides" (auto
/// detection), which is what "auto"/empty request.
private func nemotronLanguage(_ code: String) -> String? {
    let trimmed = code.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    if trimmed.isEmpty || trimmed == "auto" { return nil }
    return trimmed
}

@_cdecl("ttf_nemotron_init")
public func ttf_nemotron_init(
    _ modelsDir: UnsafePointer<CChar>,
    _ chunkMs: Int,
    _ err: UnsafeMutablePointer<CChar>,
    _ errCap: Int
) -> Int32 {
    let base = URL(fileURLWithPath: String(cString: modelsDir), isDirectory: true)
    do {
        // "auto" downloads the full-vocab `multilingual` variant (the
        // Latin-script languages get a pruned `latin` variant only when a
        // specific Latin hint is passed, which the dictation path does not).
        // The variant lands in `<base>/<repo folder>/<langDir>/<chunkMs>ms/`
        // and cached downloads are reused.
        let variantDir: URL = try runBlocking {
            try await StreamingNemotronMultilingualAsrManager.downloadVariant(
                languageCode: "auto",
                chunkMs: chunkMs,
                to: base
            )
        }
        let manager = StreamingNemotronMultilingualAsrManager()
        try runBlocking {
            try await manager.loadModels(from: variantDir)
        }
        withNemotron {
            nemotron.manager = manager
            nemotron.lastError = ""
        }
        return 0
    } catch {
        return failNemotron(err, errCap, "ttf_nemotron_init: \(error)")
    }
}

@_cdecl("ttf_nemotron_is_ready")
public func ttf_nemotron_is_ready() -> Int32 {
    withNemotron { nemotron.manager != nil ? 1 : 0 }
}

@_cdecl("ttf_nemotron_transcribe")
public func ttf_nemotron_transcribe(
    _ pcm: UnsafePointer<Float>,
    _ sampleCount: Int,
    _ lang: UnsafePointer<CChar>,
    _ out: UnsafeMutablePointer<CChar>,
    _ outCap: Int
) -> Int32 {
    guard sampleCount > 0 else {
        return failNemotron(nil, 0, "ttf_nemotron_transcribe: empty audio")
    }
    let samples = Array(UnsafeBufferPointer(start: pcm, count: sampleCount))
    let languageCode = String(cString: lang)
    let manager = withNemotron { nemotron.manager }
    guard let manager else {
        return failNemotron(nil, 0, "ttf_nemotron_transcribe: Nemotron model is not loaded")
    }

    do {
        // The manager chunks the stream internally and accumulates the
        // utterance; `finishWithTokenTimings` returns the full-utterance text
        // plus per-token timings and then clears the working buffers.
        // `reset` first so a fresh take never inherits encoder state from a
        // previous one (it is a no-op on a fresh session).
        let result: (text: String, timings: [TokenTiming], language: String?) = try runBlocking {
            () async throws -> (String, [TokenTiming], String?) in
            await manager.reset()
            await manager.setLanguage(nemotronLanguage(languageCode))
            _ = try await manager.process(samples: samples)
            let (text, timings) = try await manager.finishWithTokenTimings()
            return (text, timings, await manager.detectedLanguage())
        }

        let duration = Double(samples.count) / 16_000.0
        // The manager does not expose an utterance-level confidence; the mean
        // per-token confidence is the honest aggregate. A take with no token
        // timings (some families) reports the neutral 1.0, as the other
        // engines do.
        let confidence: Float = result.timings.isEmpty
            ? 1.0
            : result.timings.map(\.confidence).reduce(0, +) / Float(result.timings.count)

        let json = try encodeJSON(
            text: result.text,
            confidence: confidence,
            duration: duration,
            timings: result.timings,
            detected: [],
            applied: [],
            language: result.language
        )
        let written = copyOut(json, to: out, cap: outCap)
        return written <= outCap ? 0 : Int32(written)
    } catch {
        return failNemotron(nil, 0, "ttf_nemotron_transcribe: \(error)")
    }
}

@_cdecl("ttf_nemotron_unload")
public func ttf_nemotron_unload() {
    withNemotron {
        nemotron.manager = nil
    }
}

@_cdecl("ttf_nemotron_last_error")
public func ttf_nemotron_last_error(_ out: UnsafeMutablePointer<CChar>, _ outCap: Int) -> Int32 {
    let message = withNemotron { nemotron.lastError }
    if message.isEmpty { return 0 }
    return Int32(copyOut(message, to: out, cap: outCap))
}
