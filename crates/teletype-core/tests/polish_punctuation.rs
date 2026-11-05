//! claim: text-integrity
//!
//! PP-POLISH-1: the polish stage can emit doubled punctuation into the
//! user's document.
//!
//! Live evidence: history row 62790e30-d904-430e-8ac7-b90be1377b02
//! (teletype.db, captured 2026-11-05, Parakeet Ultra ASR + S1-mini polish).
//! The user said:
//!
//!   So basically we need to ship this on Friday, I mean Thursday because
//!   the client meeting is on Monday.
//!
//! The app pasted:
//!
//!   So basically we need to ship this on Friday,, because the client
//!   meeting is on Monday.
//!
//! The ASR was right. The transform chain wrote the doubled comma.
//!
//! Part A mechanism, in one paragraph. The live take ran through the
//! S1-mini card-cleanup path, then the pipeline's protected-token guard
//! (pipeline.rs applies `guard::restore_or_reject` to every transformed
//! take). The transcript's capitalized words are protected tokens
//! (guard.rs `is_code_identifier`: length >= 4 starting uppercase). The
//! model dropped the spoken self-repair "I mean Thursday". The LCS diff
//! (guard.rs `lcs_word_diff`) matches the polished "Friday," to the input
//! "Friday," (identical token, no restore) and counts "Thursday" as a
//! removal (no re-insertion). The guard is therefore NOT the composing
//! stage for the live take: with a deterministic double whose answer is
//! the polished text minus the self-repair (one comma), the guard output
//! carries one comma, not two. The composing stage is the model-output
//! seam: the model's own answer carried the doubled pair, and no stage
//! before the engine exit sanitizer (Part B) sanitizes adjacent
//! punctuation, so the pair flows through the validator and the guard
//! unscathed into the user's document.
//!
//! The candidate mechanisms, ruled out one by one:
//! - splitter rejoin: the live take is 20 words, under the 500-word chunk
//!   cap, so `split_for_polish` returns one chunk and the rejoin (which
//!   joins with a single space and adds no comma) never runs.
//! - guard restore: ruled out by `guard_restore_does_not_compose_the_live_pair`.
//!   The LCS diff matches "Friday," to "Friday," (identical token) and
//!   counts "Thursday" as a removal; no in-place restore fires.
//! - model output containing the pair: CAUGHT. See
//!   `model_output_seam_doubled_comma_survives_the_chain`.

use teletype_core::llm::InferenceProvider;
use teletype_core::transforms::engine::run_transform_blocking;
use teletype_core::transforms::engine::sanitize_punctuation;
use teletype_core::transforms::prompt::PromptContext;
use teletype_core::transforms::{guard, TransformStore};

/// The live take. `raw_text` from history row
/// 62790e30-d904-430e-8ac7-b90be1377b02, verbatim.
const LIVE_RAW: &str =
    "So basically we need to ship this on Friday, I mean Thursday because the client meeting is on Monday.";

/// The deterministic S1-mini double: the polished answer of the live take,
/// minus the self-repair. It kept the retracted word "Friday" and dropped
/// "I mean Thursday", exactly what the model did on 2026-11-05. The output
/// carries exactly ONE comma.
const S1_ANSWER: &str =
    "So basically we need to ship this on Friday, because the client meeting is on Monday";

struct S1Double;

impl InferenceProvider for S1Double {
    fn model_id(&self) -> &str {
        "s1-mini"
    }
    fn model_name(&self) -> &str {
        "S1-mini"
    }
    fn generate(
        &self,
        _prompt: &str,
        _params: teletype_core::llm::GenerationParams,
    ) -> Result<String, String> {
        Ok(S1_ANSWER.to_string())
    }
    fn generate_stream(
        &self,
        _prompt: &str,
        _params: teletype_core::llm::GenerationParams,
        _on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        Ok(S1_ANSWER.to_string())
    }
    fn generate_with_system_stream(
        &self,
        _system: &str,
        _user: &str,
        _params: teletype_core::llm::GenerationParams,
        _on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        Ok(S1_ANSWER.to_string())
    }
}

/// A deterministic double whose answer is the polished text of the live
/// take WITH the doubled comma the model actually emitted on 2026-11-05.
struct S1DoubleDoubled;

impl InferenceProvider for S1DoubleDoubled {
    fn model_id(&self) -> &str {
        "s1-mini"
    }
    fn model_name(&self) -> &str {
        "S1-mini"
    }
    fn generate(
        &self,
        _prompt: &str,
        _params: teletype_core::llm::GenerationParams,
    ) -> Result<String, String> {
        Ok(
            "So basically we need to ship this on Friday,, because the client meeting is on Monday"
                .into(),
        )
    }
    fn generate_stream(
        &self,
        _prompt: &str,
        _params: teletype_core::llm::GenerationParams,
        _on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        Ok(
            "So basically we need to ship this on Friday,, because the client meeting is on Monday"
                .into(),
        )
    }
    fn generate_with_system_stream(
        &self,
        _system: &str,
        _user: &str,
        _params: teletype_core::llm::GenerationParams,
        _on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        Ok(
            "So basically we need to ship this on Friday,, because the client meeting is on Monday"
                .into(),
        )
    }
}

/// Runs the live take through the real chain: the S1-mini card-cleanup
/// engine path, then the pipeline's protected-token guard (pipeline.rs
/// applies `guard::restore_or_reject` to every transformed take).
fn run_live_take() -> String {
    let transform = TransformStore::with_built_ins()
        .get("builtin-polish")
        .unwrap()
        .clone();
    let ctx = PromptContext {
        language: "en".into(),
        polish_max_change_percent: 100, // live setting for the take
        ..Default::default()
    };
    let provider = S1DoubleDoubled;
    let result = run_transform_blocking(&provider, &transform, LIVE_RAW, &ctx, &mut None);
    assert!(
        result.transformed,
        "the deterministic double must be accepted as a transform, got: {result:?}"
    );
    let protected = guard::collect_protected(LIVE_RAW, &[], &[]);
    match guard::restore_or_reject(LIVE_RAW, &result.text, &protected) {
        guard::GuardResult::Accept(guarded) => guarded,
        guard::GuardResult::Reject { reason } => panic!("guard rejected: {reason}"),
    }
}

/// The stage that composes the pair: the model-output seam. The model's own
/// answer carries the doubled comma, and no stage before the engine exit
/// sanitizer sanitizes adjacent punctuation, so the pair flows through the
/// validator and the guard unscathed until that step removes it. Live shape:
/// history row 62790e30-d904-430e-8ac7-b90be1377b02.
#[test]
fn model_output_doubled_comma_is_removed_at_the_engine_exit() {
    let raw_model_answer =
        "So basically we need to ship this on Friday,, because the client meeting is on Monday";
    assert!(
        raw_model_answer.contains(",, because"),
        "the premise of this test is a model answer carrying the live pair: \
         {raw_model_answer:?}"
    );
    let final_text = run_live_take();
    assert!(
        !final_text.contains(",, because"),
        "the engine exit sanitizer must remove a pair the model emitted: \
         {final_text:?}"
    );
    assert!(!final_text.contains("I mean Thursday"), "{final_text:?}");
}

/// Rules out the splitter seam: the live take is 20 words, far under the
/// 500-word chunk cap, so `split_for_polish` returns a single chunk and the
/// rejoin path (which joins chunks with a single space and adds no comma)
/// never runs. The doubled comma cannot come from a chunk rejoin.
#[test]
fn splitter_is_not_the_composing_stage_for_the_live_take() {
    let chunks = teletype_core::transforms::splitter::split_for_polish(LIVE_RAW);
    assert_eq!(
        chunks.len(),
        1,
        "the 20-word live take must stay one chunk, got {chunks:?}"
    );
}

/// Rules out the guard seam: with a deterministic double whose answer is
/// the polished text of the live take minus the self-repair (one comma),
/// the guard's in-place protected-token restore does NOT compose the pair.
/// The LCS diff (guard.rs `lcs_word_diff`) matches the polished "Friday,"
/// to the input "Friday," (identical token, no restore) and counts
/// "Thursday" as a removal (no re-insertion). The guard output carries one
/// comma, not two.
#[test]
fn guard_restore_does_not_compose_the_live_pair() {
    let transform = TransformStore::with_built_ins()
        .get("builtin-polish")
        .unwrap()
        .clone();
    let ctx = PromptContext {
        language: "en".into(),
        polish_max_change_percent: 100,
        ..Default::default()
    };
    let result = run_transform_blocking(&S1Double, &transform, LIVE_RAW, &ctx, &mut None);
    assert!(
        result.transformed,
        "the single-comma answer must be accepted"
    );
    assert_eq!(
        result.text.matches(',').count(),
        1,
        "the model double must emit exactly one comma: {result:?}"
    );
    let protected = guard::collect_protected(LIVE_RAW, &[], &[]);
    let guard_out = match guard::restore_or_reject(LIVE_RAW, &result.text, &protected) {
        guard::GuardResult::Accept(guarded) => guarded,
        guard::GuardResult::Reject { reason } => panic!("guard rejected: {reason}"),
    };
    assert!(
        !guard_out.contains(",, because"),
        "the guard must not compose the pair from a single-comma model answer: \
         {guard_out:?}"
    );
    assert_eq!(
        guard_out.matches(',').count(),
        1,
        "the guard output must carry exactly one comma: {guard_out:?}"
    );
}

/// The live take, end to end through the real chain after the fix: the
/// model-output seam still emits the pair, but the sanitation step at the
/// engine exit collapses it before the text leaves the transform stage.
#[test]
fn live_take_final_text_has_no_doubled_comma_after_fix() {
    let final_text = run_live_take();
    assert!(
        !final_text.contains(",, because"),
        "the user's document must never receive a doubled comma: {final_text:?}"
    );
    assert_eq!(
        final_text,
        "So basically we need to ship this on Friday, because the client meeting is on Monday"
    );
}

/// Part B: the sanitation step applied to every polish output. Table-driven
/// over the required shapes; the last case must stay byte-identical.
#[test]
fn sanitize_punctuation_collapses_doubled_marks_and_keeps_legal_text() {
    for (input, want) in [
        // The live shape from history row 62790e30-d904-430e-8ac7-b90be1377b02.
        (
            "So basically we need to ship this on Friday,, because the client meeting is on Monday.",
            "So basically we need to ship this on Friday, because the client meeting is on Monday.",
        ),
        // Two or more identical adjacent marks collapse to one.
        ("a,, b", "a, b"),
        ("a,,, b", "a, b"),
        ("stop.. now", "stop. now"),
        ("stop... now", "stop... now"),
        ("wait... go.. stop", "wait... go. stop"),
        ("wait;; really", "wait; really"),
        // A comma followed by a period, or a period followed by a comma
        // (adjacent, no whitespace): keep the period only.
        ("end,. next", "end. next"),
        ("end., next", "end. next"),
        // Whitespace between the pair is legal and kept.
        ("end, . next", "end, . next"),
        ("end. , next", "end. , next"),
        // Legal text stays byte-identical.
        (
            "So basically we need to ship this on Friday, because the client meeting is on Monday.",
            "So basically we need to ship this on Friday, because the client meeting is on Monday.",
        ),
        ("a, b, c. d? e! f.", "a, b, c. d? e! f."),
        ("stop... wait, no.", "stop... wait, no."),
        ("a..b", "a.b"),
        ("a, b; c. d", "a, b; c. d"),
    ] {
        assert_eq!(
            sanitize_punctuation(input),
            want,
            "input: {input:?}"
        );
    }
}

/// The sanitation step is idempotent: running it twice changes nothing.
#[test]
fn sanitize_punctuation_is_idempotent() {
    for input in [
        "a,, b",
        "end,. next",
        "stop... now",
        "So basically we need to ship this on Friday,, because the client meeting is on Monday.",
    ] {
        let once = sanitize_punctuation(input);
        assert_eq!(sanitize_punctuation(&once), once, "input: {input:?}");
    }
}
