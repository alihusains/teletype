// TeletypeApple, C ABI bridge to Apple's on-device speech (SpeechAnalyzer:
// SpeechTranscriber / DictationTranscriber), macOS 26.0+ only.
//
// Rust side: `crates/teletype-apple/src/bridge.rs`. The signatures and the
// return-code contract below are mirrored there; change both together.
//
//   tta_available(err, errCap) -> Int32
//   tta_installed_locales(out, outCap) -> Int32
//   tta_supported_locales(out, outCap) -> Int32
//   tta_install_assets(locale, cb, userData, err, errCap) -> Int32
//   tta_transcribe(pcm, n, locale, out, outCap, err, errCap) -> Int32
//   tta_last_error(out, outCap) -> Int32
//
// Return contract: 0 = success; a positive number means the caller's output
// buffer was too small and the value is the needed capacity (retry with that
// capacity); -1 = failure (detail in the `err` buffer when provided and via
// `tta_last_error`). `tta_available` returns 1 when `SpeechTranscriber.
// isAvailable` is true on this machine, 0 otherwise, and writes the reason
// into `err`.
//
// Every entry point blocks its thread and must not run on the main thread
// (same contract as the TeletypeFluid bridge). The bridge owns one
// process-wide session state (locale reservation + last error); one live
// instance is expected, exactly like TeletypeFluid.
//
// Models are OS-managed: `AssetInventory` downloads and stores them in
// system storage, so there is no app download and no file on disk to check.
//
// SDK notes (verified against the macOS 27.0 swiftinterface):
// - `AssetInventory`'s members are all static (`reserve(locale:)`,
//   `release(reservedLocale:)`, `maximumReservedLocales`,
//   `assetInstallationRequest(supporting:)`); there is no instance
//   initializer. Locales are `Foundation.Locale` values.
// - `supportedLocales` / `installedLocales` are async static properties, so
//   every read of them happens inside an async context.
// - `AssetInstallationRequest.downloadAndInstall()` takes no delegate;
//   progress is read from its `Progress` object.

import Foundation
import Speech
import os.log

private let log = Logger(subsystem: "com.teletype.app", category: "apple-bridge")

/// The whole call is bounded by this; a stuck asset download must not hang
/// the dictation worker forever.
private let installTimeout: TimeInterval = 10 * 60
/// Transcription of a take must finish well before the user's patience runs
/// out; the research smoke test measured a median of 88 ms per clip.
private let transcribeTimeout: TimeInterval = 60

// MARK: - Process-wide session state

private final class BridgeState: @unchecked Sendable {
    let lock = NSLock()
    /// Locale reserved through `AssetInventory.reserve` during an install.
    var reservedLocale: String?
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
///   the capacity the caller must provide (always >= 1).
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
/// caller provided an error buffer, the message is also written there.
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

/// Accumulates text fragments from a detached results-draining task.
private final class TextBox: @unchecked Sendable {
    private let lock = NSLock()
    private var parts: [String] = []
    func append(_ s: String) {
        lock.lock()
        parts.append(s)
        lock.unlock()
    }
    var text: String {
        lock.lock()
        defer { lock.unlock() }
        return parts.joined()
    }
}

/// Run `body` on a detached task, blocking the calling thread until it
/// finishes or `timeout` elapses. The caller must not be the main thread
/// (documented bridge contract).
private func runBlocking<T>(
    _ timeout: TimeInterval,
    _ body: @escaping @Sendable () async throws -> T
) throws -> T {
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
    guard done.wait(timeout: .now() + timeout) == .success else {
        throw BridgeFailure.message("timed out after \(Int(timeout)) s")
    }
    guard let value = box.value else {
        throw BridgeFailure.message("internal error: async bridge produced no result")
    }
    return try value.get()
}

/// The transcriber module for a locale: `SpeechTranscriber` when it supports
/// the locale, `DictationTranscriber` when only it does (the "older devices"
/// module covers 9 extra locales, mostly European). The check is async
/// because `supportedLocales` is an async property.
@available(macOS 26.0, *)
private func module(forLocale locale: String) async -> any SpeechModule {
    let localeValue = Locale(identifier: locale)
    let transcriberLocales = Set(await SpeechTranscriber.supportedLocales.map(\.identifier))
    if transcriberLocales.contains(locale) {
        return SpeechTranscriber(locale: localeValue, preset: .transcription)
    }
    let dictationLocales = Set(await DictationTranscriber.supportedLocales.map(\.identifier))
    if dictationLocales.contains(locale) {
        return DictationTranscriber(locale: localeValue, preset: .shortDictation)
    }
    return SpeechTranscriber(locale: localeValue, preset: .transcription)
}

/// Run the analyzer over the Int16 samples and return the final transcript text.
/// `prepareToAnalyze` first: on first use it installs the locale's assets
/// when the user has pre-approved the download, so a `load` that only
/// checked `installedLocales` still ends with a working module.
@available(macOS 26.0, *)
private func transcriberText(of result: some SpeechModuleResult) async -> String {
    if let transcriber = result as? SpeechTranscriber.Result {
        return String(transcriber.text.characters)
    }
    if let dictation = result as? DictationTranscriber.Result {
        return String(dictation.text.characters)
    }
    return ""
}

@available(macOS 26.0, *)
private func analyze(
    int16Data: [Int16],
    module: some SpeechModule
) async throws -> String {
    let analyzer = SpeechAnalyzer(modules: [module])
    try await analyzer.prepareToAnalyze(in: nil)
    let context = AnalysisContext()
    context.contextualStrings[.general] = ["Transcribe dictated text."]
    try await analyzer.setContext(context)
    // The analyzer's `analyzeSequence(from:)` takes an `AVAudioFile`, not a
    // raw buffer. `AVAudioFile.write(from:)` writes the PCM data but leaves
    // the file's `length` at zero (verified on macOS 27), so the file is
    // unusable for analysis. Instead, a WAV file is written manually with
    // the JUNK+FLLR header the analyzer requires. The file is deleted
    // afterwards and holds at most a few seconds of 16 kHz mono audio.
    let tmpDir = FileManager.default.temporaryDirectory
    let wavURL = tmpDir.appendingPathComponent("tta-analysis-\(UUID().uuidString).wav")
    do {
        try writeWav(int16Data: int16Data, to: wavURL)
        let audioFile = try AVAudioFile(forReading: wavURL)
        guard audioFile.length > 0 else {
            try? FileManager.default.removeItem(at: wavURL)
            throw BridgeFailure.message("audio file is empty after writing")
        }
        // Drain the results stream in a task while feeding the audio file
        // to the analyzer; the two must run concurrently or the results
        // stream never ends. A non-detached task is required: a detached
        // task runs in a separate actor context and the results stream
        // silently produces no values (verified on macOS 27).
        let box = TextBox()
        let resultsTask = Task {
            do {
                for try await result in module.results {
                    let t = await transcriberText(of: result)
                    if !t.isEmpty {
                        box.append(t)
                    }
                }
            } catch {
                // The results stream signals the end of analysis by
                // throwing; an empty catch is the normal completion path.
            }
        }
        _ = try await analyzer.analyzeSequence(from: audioFile)
        try await analyzer.finalizeAndFinishThroughEndOfInput()
        try await resultsTask.value
        let text = box.text
        try? FileManager.default.removeItem(at: wavURL)
        return text
    } catch {
        try? FileManager.default.removeItem(at: wavURL)
        throw error
    }
}

/// Write Int16 PCM samples as a 16 kHz mono WAV file in the format that
/// `SpeechAnalyzer.analyzeSequence(from:)` requires on macOS 27. A standard
/// 44-byte RIFF header produces a file that `AVAudioFile` reads but the
/// speech analyzer silently ignores (verified: the analyzer returns no
/// results). The required format includes a 28-byte JUNK chunk and a
/// 4008-byte FLLR chunk between the `fmt ` and `data` chunks. The total
/// header is 4096 bytes. The PCM data is written as raw little-endian
/// Int16 bytes.
private func writeWav(int16Data: [Int16], to url: URL) throws {
    let sampleRate = 16_000
    let channelCount = 1
    let bytesPerSample = 2
    let blockAlign = bytesPerSample * channelCount
    let byteRate = sampleRate * blockAlign
    let dataSize = int16Data.count * bytesPerSample
    // Write PCM as raw little-endian bytes.
    let pcmBytes: [UInt8] = int16Data.flatMap { sample in
        [UInt8(truncatingIfNeeded: sample), UInt8(truncatingIfNeeded: sample >> 8)]
    }
    // Header layout: RIFF(12) + JUNK(36) + fmt(24) + FLLR(4016) + data(8) = 4096
    var out = Data(capacity: 4096 + dataSize)
    func appendStr(_ s: String) { out.append(Data(s.utf8)) }
    func appendLE16(_ v: Int) {
        out.append(UInt8(v & 0xFF))
        out.append(UInt8((v >> 8) & 0xFF))
    }
    func appendLE32(_ v: Int) {
        out.append(UInt8(v & 0xFF))
        out.append(UInt8((v >> 8) & 0xFF))
        out.append(UInt8((v >> 16) & 0xFF))
        out.append(UInt8((v >> 24) & 0xFF))
    }
    // RIFF header
    appendStr("RIFF")
    appendLE32(4096 - 8 + dataSize)
    appendStr("WAVE")
    // JUNK chunk (36 bytes: 8 header + 28 zero-filled)
    appendStr("JUNK")
    appendLE32(28)
    for _ in 0..<28 { out.append(0) }
    // fmt chunk
    appendStr("fmt ")
    appendLE32(16)
    appendLE16(1) // PCM
    appendLE16(channelCount)
    appendLE32(sampleRate)
    appendLE32(byteRate)
    appendLE16(blockAlign)
    appendLE16(16)
    // FLLR chunk (4016 bytes: 8 header + 4008 zero-filled)
    appendStr("FLLR")
    appendLE32(4008)
    for _ in 0..<4008 { out.append(0) }
    // data chunk header
    appendStr("data")
    appendLE32(dataSize)
    // PCM data
    out.append(Data(pcmBytes))
    try out.write(to: url)
}

// MARK: - C ABI

@_cdecl("tta_available")
public func tta_available(_ err: UnsafeMutablePointer<CChar>, _ errCap: Int) -> Int32 {
    guard #available(macOS 26.0, *) else {
        return fail(err, errCap, "Apple on-device speech needs macOS 26 or newer")
    }
    guard SpeechTranscriber.isAvailable else {
        return fail(err, errCap, "SpeechTranscriber is not available on this hardware")
    }
    return 1
}

@_cdecl("tta_installed_locales")
public func tta_installed_locales(_ out: UnsafeMutablePointer<CChar>, _ outCap: Int) -> Int32 {
    guard #available(macOS 26.0, *) else {
        return fail(nil, 0, "Apple on-device speech needs macOS 26 or newer")
    }
    do {
        let locales = try runBlocking(5) {
            await Array(SpeechTranscriber.installedLocales)
                .sorted { $0.identifier < $1.identifier }
                .map(\.identifier)
        }
        let data = try JSONEncoder().encode(locales)
        guard let json = String(data: data, encoding: .utf8) else {
            return fail(nil, 0, "locale list is not valid UTF-8")
        }
        return Int32(copyOut(json, to: out, cap: outCap))
    } catch {
        return fail(nil, 0, "reading installed locales failed: \(error)")
    }
}

@_cdecl("tta_supported_locales")
public func tta_supported_locales(_ out: UnsafeMutablePointer<CChar>, _ outCap: Int) -> Int32 {
    guard #available(macOS 26.0, *) else {
        return fail(nil, 0, "Apple on-device speech needs macOS 26 or newer")
    }
    do {
        // The union of both modules: DictationTranscriber covers locales the
        // transcriber does not, and the bridge picks the module per locale.
        let supported = try runBlocking(5) {
            await Set(SpeechTranscriber.supportedLocales.map(\.identifier))
                .union(DictationTranscriber.supportedLocales.map(\.identifier))
                .sorted()
        }
        let data = try JSONEncoder().encode(supported)
        guard let json = String(data: data, encoding: .utf8) else {
            return fail(nil, 0, "locale list is not valid UTF-8")
        }
        return Int32(copyOut(json, to: out, cap: outCap))
    } catch {
        return fail(nil, 0, "reading supported locales failed: \(error)")
    }
}

/// The C callback and its user data, boxed so the detached install task can
/// carry them without capturing a non-Sendable raw pointer directly.
private final class ProgressReporter: @unchecked Sendable {
    let cb: (@convention(c) (UnsafeMutableRawPointer?, Double, UnsafePointer<CChar>?) -> Void)?
    let userData: UnsafeMutableRawPointer?

    init(
        cb: (@convention(c) (UnsafeMutableRawPointer?, Double, UnsafePointer<CChar>?) -> Void)?,
        userData: UnsafeMutableRawPointer?
    ) {
        self.cb = cb
        self.userData = userData
    }

    func report(_ fraction: Double) {
        cb?(userData, Swift.min(Swift.max(fraction, 0.0), 0.99), nil)
    }

    func finish() {
        cb?(userData, 1.0, nil)
    }
}

/// Report install progress from the request's `Progress` object through the
/// reporter, polled on a detached task.
private func pumpProgress(_ progress: Progress, reporter: ProgressReporter) async {
    while !progress.isFinished {
        reporter.report(Double(progress.fractionCompleted))
        try? await Task.sleep(for: .milliseconds(250))
    }
    reporter.finish()
}

@_cdecl("tta_install_assets")
public func tta_install_assets(
    _ locale: UnsafePointer<CChar>,
    _ cb: @convention(c) (UnsafeMutableRawPointer?, Double, UnsafePointer<CChar>?) -> Void,
    _ userData: UnsafeMutableRawPointer?,
    _ err: UnsafeMutablePointer<CChar>,
    _ errCap: Int
) -> Int32 {
    guard #available(macOS 26.0, *) else {
        return fail(err, errCap, "Apple on-device speech needs macOS 26 or newer")
    }
    let localeId = String(cString: locale)
    // Built outside the task: the C callback and its user data cross into the
    // detached install task through this Sendable box, not as raw captures.
    let reporter = ProgressReporter(cb: cb, userData: userData)
    do {
        try runBlocking(installTimeout) {
            let localeValue = Locale(identifier: localeId)
            let reserved = try await AssetInventory.reserve(locale: localeValue)
            withBridge { bridge.reservedLocale = reserved ? localeId : nil }
            let module = await module(forLocale: localeId)
            guard let request = try await AssetInventory.assetInstallationRequest(
                supporting: [module]
            ) else {
                throw BridgeFailure.message(
                    "no install request for '\(localeId)': the OS already has it or does not offer it"
                )
            }
            async let install: Void = request.downloadAndInstall()
            await pumpProgress(request.progress, reporter: reporter)
            try await install
            // The reservation is consumed once the assets are in; release it
            // so the 5-locale cap stays free for other languages.
            if withBridge({ let l = bridge.reservedLocale; bridge.reservedLocale = nil; return l })
                == localeId
            {
                _ = await AssetInventory.release(reservedLocale: localeValue)
            }
        }
        return 0
    } catch {
        // Leave the reservation in place on failure: the OS owns it and the
        // next attempt re-reserves the same locale, which is idempotent.
        return fail(err, errCap, "installing assets for '\(localeId)' failed: \(error)")
    }
}

@_cdecl("tta_transcribe")
public func tta_transcribe(
    _ pcm: UnsafePointer<Float>,
    _ sampleCount: Int,
    _ locale: UnsafePointer<CChar>,
    _ out: UnsafeMutablePointer<CChar>,
    _ outCap: Int,
    _ err: UnsafeMutablePointer<CChar>,
    _ errCap: Int
) -> Int32 {
    guard #available(macOS 26.0, *) else {
        return fail(err, errCap, "Apple on-device speech needs macOS 26 or newer")
    }
    guard sampleCount > 0 else {
        return fail(err, errCap, "tta_transcribe: empty audio")
    }
    let localeId = String(cString: locale)
    do {
        let samples = Array(UnsafeBufferPointer(start: pcm, count: sampleCount))
        let text = try runBlocking(transcribeTimeout) {
            // Convert the f32 samples to Int16 directly: the analyzer's
            // WAV writer reads the buffer's native format and the
            // float-to-int16 conversion inside `pcmBuffer` produces a
            // buffer whose `floatChannelData` returns all zeros on macOS
            // 27, which writes a silent WAV. Converting the samples
            // before creating the buffer avoids the issue entirely.
            let int16Data = samples.map {
                Int16((max(-1.0, min(1.0, Double($0))) * 32_767.0).rounded())
            }
            let module = await module(forLocale: localeId)
            return try await analyze(int16Data: int16Data, module: module)
        }
        _ = copyOut(text, to: out, cap: outCap)
        return 1
    } catch {
        return fail(err, errCap, "tta_transcribe: \(error)")
    }
}

@_cdecl("tta_last_error")
public func tta_last_error(_ out: UnsafeMutablePointer<CChar>, _ outCap: Int) -> Int32 {
    let message = withBridge { bridge.lastError }
    if message.isEmpty { return 0 }
    return Int32(copyOut(message, to: out, cap: outCap))
}
