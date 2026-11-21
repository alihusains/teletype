# Wow model defaults and 2B CoreML check

Status: implemented in the main working tree, uncommitted.

## Defaults changed
- Accelerated macOS (Apple Silicon 14+, FluidAudio bridge): Parakeet Ultra CoreML ASR + Qwen3.5 2B IQ4_XS polish.
- `qwen3.5-2b` now leads the accelerated polish shortlist and carries the Recommended badge; S1-mini remains the CPU cleanup default.
- Windows/Other: added official `sherpa-parakeet-ultra-int8` (465 MB, SHA-256 `d216b71d...c09`) and made it the Windows/Other default; v3 remains fallback. `sherpa.rs` accepts the new id.
- CPU-only Windows/macOS stays on S1-mini/SpeakoFlow/Qwen3.5 0.8B because Qwen3.5 2B is marked too slow without GPU.

## Qwen3.5 2B CoreML finding
CoreML/Core AI conversions exist: `mlboydaisuke/qwen3.5-2B-CoreML` (~2.8 GB, ~27 tok/s iPhone, chunked ANE, ~15 min first compile) and `mlboydaisuke/qwen3.5-2B-CoreAI` (`.aimodel`, requires macOS/iOS 27 APIs, M4 Max ~160 tok/s, iPhone ~28–30 tok/s). On macOS 26.3 CoreML `computeUnits=ALL` routes to GPU, not ANE. These are slower or require a new runtime/OS API; the existing GGUF/llama.cpp Metal path is faster and portable, so CoreML was **not adopted**.

## Windows answer
Yes with caveats: Parakeet Ultra works on Windows via official sherpa-onnx int8 and is now the default. Qwen3.5 2B GGUF works on Windows with llama.cpp/CUDA; on CPU-only Windows it remains filtered out in favor of the cleanup models. CoreML is Apple-only.

## Verification
Core 1376, speech 81, inference 131, desktop 624, eval self-test PASS, tsc clean, vitest 58 files / 599 tests, fmt clean, brain freshness PASS. Brain updated with D046.
