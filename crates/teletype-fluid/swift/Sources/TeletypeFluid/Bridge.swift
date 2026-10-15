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
// Return contract: 0 = success; `ttf_transcribe` returns the needed output
// buffer size as a positive number when the caller's buffer is too small
// (retry with that capacity); -1 = failure (detail via `ttf_last_error`, plus
// the `err` buffer for `ttf_init`). All entry points block and must not run on
// the main thread (plan §3.1 rule).
//
// Model storage: Parakeet models live under `<modelsDir>/<repo folder>/`,
// where `<modelsDir>` is the app's `models/fluid` directory. Vocabulary
// boosting's CTC models are the exception — FluidAudio's
// `VocabularyBoostingSession` hardcodes `CtcModels.defaultCacheDirectory` for
// its tokenizer and rescorer, so those stay in FluidAudio's own Application
// Support folder (recorded in PROGRESS.md, P3-A).

import Foundation
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
private func loadASR(version: AsrModelVersion, base: URL) async throws -> AsrManager {
    let folder = base.appendingPathComponent(repoFolder(version), isDirectory: true)
    try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)

    let models: AsrModels
    if AsrModels.modelsExist(at: folder, version: version) {
        log.info("fluid: loading \(repoFolder(version), privacy: .public) from \(folder.path, privacy: .public)")
        models = try await AsrModels.load(from: folder, version: version)
    } else {
        log.info("fluid: downloading \(repoFolder(version), privacy: .public) to \(base.path, privacy: .public)")
        models = try await AsrModels.downloadAndLoad(to: folder, version: version)
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
    applied: [String]
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
        applied_terms: applied
    )
    let data = try JSONEncoder().encode(payload)
    guard let json = String(data: data, encoding: .utf8) else {
        throw BridgeFailure.message("transcript JSON is not valid UTF-8")
    }
    return json
}

// MARK: - C ABI

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
    do {
        let manager = try runBlocking {
            try await loadASR(version: requested, base: base)
        }
        withBridge {
            bridge.manager = manager
            bridge.boostSession = nil
            bridge.lastError = ""
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
    let snapshot = withBridge { (bridge.manager, bridge.boostSession) }
    guard let manager = snapshot.0 else {
        return fail(nil, 0, "ttf_transcribe: ASR models are not loaded")
    }
    let boost = snapshot.1

    do {
        let json = try runBlocking { () async throws -> String in
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
