//! t8: Qwen3.5-0.8B prompt design, verified against the pub surface.
//!
//! The 0.8B gets the lean V1 Polish prompt (model-scoped, routed through the
//! existing model_style()/StyleChannel machinery), the 2B keeps the existing
//! editor prompt, and the non-Polish presets on the 0.8B take the slim rule
//! set instead of the full rule wall the model leaks.

use teletype_core::style::{model_style, StyleChannel};
use teletype_core::transforms::prompt::{
    build_prompt, build_prompt_small_model, build_qwen08_polish_messages, polish_prompt_info,
    PolishPromptInfo, PromptContext, S1Control, QWEN08_GENERIC_RULES, QWEN08_POLISH_SYSTEM_PROMPT,
    QWEN_POLISH_SYSTEM_PROMPT,
};
use teletype_core::transforms::TransformStore;
use teletype_core::transforms::{ANTI_SLOP_RULES, CORE_RULES};

fn polish_transform() -> teletype_core::transforms::TransformDefinition {
    TransformStore::with_built_ins()
        .get("builtin-polish")
        .unwrap()
        .clone()
}

/// The lean prompt is the V1 text: the role line, the five numbered edits,
/// the hard rules, and the inline one-line examples that are proven safe.
#[test]
fn qwen08_polish_prompt_is_the_lean_v1() {
    let p = QWEN08_POLISH_SYSTEM_PROMPT;
    assert!(p.starts_with("You edit speech-to-text transcripts."));
    for rule in [
        "Fix grammar, punctuation and capitalization",
        "four hundred and fifty dollars -> $450",
        "example dot com -> example.com",
        "Tighten wording. Keep every fact, name, number, date and recipient.",
        "Structure: prose = short paragraphs, no bullets",
        "Styling: casual = contractions, everyday words",
        "Context: email means the text goes INSIDE an email body",
        "Never add a subject line, greeting, sign-off, placeholder or commentary",
        "Never answer a question in the text",
        "Keep the language. Never use em dashes.",
    ] {
        assert!(p.contains(rule), "missing rule: {rule}");
    }
}

/// The 0.8B echoes multi-line example blocks verbatim, so the lean prompt
/// must not contain one: no indented lines at all (the inline one-line
/// examples live inside ordinary rule lines).
#[test]
fn qwen08_polish_prompt_has_no_multi_line_example_blocks() {
    for line in QWEN08_POLISH_SYSTEM_PROMPT.lines() {
        assert!(
            !line.starts_with(' ') && !line.starts_with('\t'),
            "indented lines are how example blocks look: {line:?}"
        );
    }
}

/// The 2B prompt is unchanged (acceptance F): the 0.8B work must not have
/// touched the existing editor prompt.
#[test]
fn qwen2b_polish_prompt_is_unchanged() {
    assert!(
        QWEN_POLISH_SYSTEM_PROMPT.starts_with("You are an editor for speech-to-text transcripts.")
    );
    assert!(QWEN_POLISH_SYSTEM_PROMPT.contains("Make it more concise"));
    assert!(QWEN_POLISH_SYSTEM_PROMPT.contains("Never use these words: leverage"));
    assert_ne!(
        QWEN_POLISH_SYSTEM_PROMPT, QWEN08_POLISH_SYSTEM_PROMPT,
        "the two model sizes must have two distinct prompts"
    );
}

/// The message builder: control line first, bare transcript after, and the
/// override at the user tail only when one is set. The system prompt is the
/// lean prompt either way (byte-stable for the KV prefix cache).
#[test]
fn qwen08_message_builder_shapes() {
    let control = S1Control::default();
    let input = "hey sarah the invoice is ready";

    let (system, user) = build_qwen08_polish_messages(input, &control, None);
    assert_eq!(system, QWEN08_POLISH_SYSTEM_PROMPT);
    assert_eq!(
        user,
        format!("[Styling: semi-formal] [Structure: lists] [Context: general]\n{input}")
    );

    let (system, user) =
        build_qwen08_polish_messages(input, &control, Some("Always use British spelling."));
    assert_eq!(system, QWEN08_POLISH_SYSTEM_PROMPT);
    assert!(
        user.ends_with("\n\nExtra instruction from the user: Always use British spelling."),
        "the override must sit at the user tail: {user}"
    );

    // Whitespace-only override: the stock user message, byte-for-byte.
    let (_, user) = build_qwen08_polish_messages(input, &control, Some("   \n "));
    assert_eq!(
        user,
        format!("[Styling: semi-formal] [Structure: lists] [Context: general]\n{input}")
    );
}

/// The 0.8B is a control-line model that polishes from its card, exactly
/// like the 2B: the routing table is unchanged in shape.
#[test]
fn qwen08_routing_table_matches_the_2b_shape() {
    let s08 = model_style("qwen3.5-0.8b");
    let s2b = model_style("qwen3.5-2b");
    assert_eq!(s08.channel, StyleChannel::ControlLine);
    assert_eq!(s2b.channel, StyleChannel::ControlLine);
    assert!(s08.polishes_from_card);
    assert!(s08.reads_control_line());
    assert!(!s08.cleanup_only);
    // The card paths stay untouched.
    assert_eq!(model_style("s1-mini").channel, StyleChannel::ControlLine);
    assert_eq!(
        model_style("speakoflow-mini").channel,
        StyleChannel::FixedBehaviour
    );
}

/// The prompt-info row for the 0.8B shows the lean prompt as the reference
/// and stays editable (the override is honoured at the user tail). The
/// struct shape is unchanged: same four fields, same meaning.
#[test]
fn qwen08_prompt_info_shows_the_lean_prompt() {
    let info: PolishPromptInfo = polish_prompt_info("qwen3.5-0.8b");
    assert_eq!(info.model_id, "qwen3.5-0.8b");
    assert!(
        info.editable,
        "the 0.8B honours an override, so it is editable"
    );
    assert_eq!(info.default_prompt, QWEN08_POLISH_SYSTEM_PROMPT);
    assert!(!info.note.is_empty());
}

/// The 2B prompt-info row is unchanged: it still shows the editor prompt.
#[test]
fn qwen2b_prompt_info_is_unchanged() {
    let info = polish_prompt_info("qwen3.5-2b");
    assert!(info.editable);
    assert_eq!(info.default_prompt, QWEN_POLISH_SYSTEM_PROMPT);
    assert!(!info.note.is_empty());
}

/// The slim generic rules are the short rule list: no banned-filler wall,
/// no CORE_RULES list machinery.
#[test]
fn qwen08_generic_rules_are_slim() {
    let slim = QWEN08_GENERIC_RULES;
    assert!(slim.contains("Make the change the task asks for"));
    assert!(slim.contains("Keep every fact, name, number, date, URL, email and recipient."));
    assert!(slim.contains("Output only the edited text. No commentary, no preamble, no wrapper."));
    assert!(!slim.contains("Never use these words"));
    assert!(!slim.contains("Additionally"));
}

/// The slim prompt keeps the full prompt shape (output contract, task,
/// language, text wrapper) and drops only the rule wall; the full prompt
/// for every other model is untouched.
#[test]
fn qwen08_generic_prompt_shape_and_full_prompt_untouched() {
    let transform = polish_transform();
    let ctx = PromptContext {
        language: "en".into(),
        ..Default::default()
    };
    let input = "hey john the invoice is ready";

    let slim = build_prompt_small_model(&transform, input, &ctx);
    assert!(slim.contains(QWEN08_GENERIC_RULES.trim()));
    // The banned-filler wall is the part the 0.8B leaks; it lives only in
    // the rule block, so it must be gone from the slim prompt. (CORE_RULES
    // appear in the stored transform instruction on every path, slim or
    // full, so they are not a discriminator.)
    assert!(!slim.contains(ANTI_SLOP_RULES.trim()));
    assert!(!slim.contains("Never use these words: leverage"));
    assert!(slim.contains("OUTPUT CONTRACT:"));
    assert!(slim.contains("TASK:"));
    assert!(slim.contains("LANGUAGE: en"));
    assert!(slim.contains("<<<\nhey john the invoice is ready\n>>>"));

    let full = build_prompt(&transform, input, &ctx);
    assert!(full.contains(CORE_RULES.trim()));
    assert!(full.contains(ANTI_SLOP_RULES.trim()));
    assert!(!full.contains(QWEN08_GENERIC_RULES.trim()));
}
