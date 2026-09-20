//! The unified pipeline: voice and typed input converge here.
//!
//! ```text
//! UnifiedInput
//!   ↓ ContextProvider        → ApplicationContext
//!   ↓ AutoText protect()     → {{AUTOTEXT_n}} placeholders
//!   ↓ Personalization        → PreferencePacket
//!   ↓ TransformEngine        → prompt → inference → validation
//!   ↓ AutoText restore()     → exact values back
//!   ↓ TextInjector           → clipboard + paste
//! ```
//!
//! Data safety: the raw input is preserved at every stage. If any AI stage
//! fails, the pipeline returns the original input (with AutoText expanded) —
//! never an error that loses the user's words.

use crate::{
    autotext::{self, protect, AutoTextStore},
    context::ApplicationContext,
    dictionary::Dictionary,
    llm::InferenceProvider,
    personalization::{self, UserProfile},
    platform::Platform,
    style::StyleProfileStore,
    transforms::{
        engine::{self, TransformResult},
        prompt::PromptContext,
        TransformDefinition, TransformStore,
    },
};

/// Where the input came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputSource {
    /// From the microphone (transcript).
    #[default]
    Voice,
    /// Typed directly (AutoText expansion only, no AI by default).
    Typed,
}

/// One unit of input entering the pipeline.
#[derive(Debug, Clone)]
pub struct UnifiedInput {
    pub source: InputSource,
    /// Raw text, before any processing.
    pub text: String,
}

/// The outcome of one pipeline run.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// The final text that was (or would be) injected.
    pub final_text: String,
    /// The raw input, preserved for history/debugging.
    pub raw_input: String,
    /// True when an AI transform changed the text.
    pub transformed: bool,
    /// True when AutoText expanded at least one trigger.
    pub autotext_expanded: bool,
    /// The context the pipeline saw.
    pub context: ApplicationContext,
    /// Transform metrics, if a transform ran.
    pub transform: Option<TransformResult>,
}

/// Everything the pipeline needs, assembled by the desktop app.
pub struct Pipeline<'a> {
    pub platform: &'a dyn Platform,
    pub autotext: &'a AutoTextStore,
    pub transforms: &'a TransformStore,
    pub profile: &'a UserProfile,
    /// The inference provider for the selected model, if ready.
    pub inference: Option<&'a dyn InferenceProvider>,
    /// The user dictionary (custom words protected from "corrections").
    pub dictionary: &'a Dictionary,
    /// Style profiles; `active_style` is the id of the active profile.
    pub styles: &'a StyleProfileStore,
    pub active_style: &'a str,
    /// When true, run the auto-apply transform (voice input).
    pub auto_apply: bool,
    /// Restore the user's clipboard after injection.
    pub restore_clipboard: bool,
    /// When true, strip filler words from voice transcripts.
    pub remove_filler_words: bool,
    /// The list of filler words to remove (lowercased).
    pub filler_words: Vec<String>,
}

impl<'a> Pipeline<'a> {
    /// Runs the pipeline and returns the final text. Does NOT inject —
    /// the caller decides (tests assert on the returned text).
    pub fn run(
        &self,
        input: UnifiedInput,
        explicit_transform: Option<&TransformDefinition>,
    ) -> PipelineResult {
        let raw_input = input.text.clone();
        let context = self.platform.active_application().unwrap_or_default();
        let is_voice = input.source == InputSource::Voice;

        // 1. Protect AutoText values (typed `/trigger`s) and, for voice,
        //    spoken snippet phrases, so the exact values survive a transform.
        //    The two namespaces are disjoint, so protect each on the original
        //    text and keep the pass that matched anything.
        let protected = if is_voice {
            let typed = protect::protect(&input.text, self.autotext, &context);
            let spoken = protect::protect_snippets(&input.text, self.autotext, &context);
            if spoken.has_placeholders() {
                spoken
            } else {
                typed
            }
        } else {
            protect::protect(&input.text, self.autotext, &context)
        };
        let autotext_expanded = protected.has_placeholders();

        // 2. Choose the transform (owned, so the match arms have a uniform type).
        let transform: Option<TransformDefinition> = match explicit_transform {
            Some(t) => Some(t.clone()),
            None if input.source == InputSource::Voice && self.auto_apply => {
                self.transforms.auto_apply().cloned()
            }
            None => None,
        };

        // 3. Run the transform (if any and if a provider is ready).
        let (text_after_transform, transform_result) = match transform.as_ref() {
            Some(t) => match self.inference {
                Some(provider) => {
                    let packet = personalization::packet::resolve(self.profile, &context);
                    let mut style = packet.style;
                    // Active style profile phrases (deduped).
                    for p in self.styles.active_phrases(self.active_style) {
                        if !style.contains(&p) {
                            style.push(p);
                        }
                    }
                    let mut known = self
                        .dictionary
                        .known_words()
                        .into_iter()
                        .collect::<Vec<_>>();
                    known.sort();
                    let ctx = PromptContext {
                        app: Some(context.clone()),
                        preferences: style,
                        preferred_terms: known
                            .into_iter()
                            .map(|w| format!("keep '{w}' as written"))
                            .collect(),
                        user_instruction: None,
                        language: self.profile.language.clone(),
                    };
                    let result = engine::run_transform_blocking(provider, t, &protected.text, &ctx);
                    (result.text.clone(), Some(result))
                }
                None => {
                    // No model: expand AutoText (and snippets for voice).
                    let mut expanded = autotext::expand::expand(&input.text, self.autotext, &context);
                    if is_voice {
                        expanded = autotext::expand::expand_snippets(&expanded, self.autotext, &context);
                    }
                    (expanded, None)
                }
            },
            None => (protected.text.clone(), None),
        };

        // 4. Remove filler words (voice input only, when enabled).
        let cleaned = if self.remove_filler_words
            && is_voice
            && !self.filler_words.is_empty()
        {
            remove_filler_words(&text_after_transform, &self.filler_words)
        } else {
            text_after_transform
        };

        // 5. Restore AutoText values, or expand directly when no transform ran.
        let final_text = if autotext_expanded {
            protect::restore(&cleaned, &protected)
        } else if transform.is_none() {
            // No transform: expand triggers directly (typed input path), plus
            // spoken snippets for voice.
            let mut expanded = autotext::expand::expand(&input.text, self.autotext, &context);
            if is_voice {
                expanded = autotext::expand::expand_snippets(&expanded, self.autotext, &context);
            }
            expanded
        } else {
            cleaned
        };

        PipelineResult {
            final_text,
            raw_input,
            transformed: transform_result.as_ref().is_some_and(|r| r.transformed),
            autotext_expanded,
            context,
            transform: transform_result,
        }
    }
}

/// Remove filler words from a transcript. Only removes whole-word, case-
/// insensitive matches that are NOT part of a longer word (e.g. "er" is
/// removed but "error" is not). Cleans up double spaces left behind.
fn remove_filler_words(text: &str, words: &[String]) -> String {
    if words.is_empty() {
        return text.to_string();
    }
    let filler: std::collections::HashSet<String> =
        words.iter().map(|w| w.to_lowercase()).collect();

    let result: String = text
        .split_whitespace()
        .filter(|tok| {
            // Strip surrounding punctuation for comparison.
            let bare = tok
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            !filler.contains(&bare)
        })
        .collect::<Vec<_>>()
        .join(" ");

    // Collapse any multiple spaces that may have resulted.
    // Use a regex for all runs of 2+ spaces (handles triple spaces too).
    let re = regex::Regex::new(r" {2,}").unwrap();
    re.replace_all(&result, " ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{context::normalize, platform::MockPlatform};

    static DICT: std::sync::LazyLock<crate::dictionary::Dictionary> =
        std::sync::LazyLock::new(crate::dictionary::Dictionary::default);
    static STYLES: std::sync::LazyLock<crate::style::StyleProfileStore> =
        std::sync::LazyLock::new(crate::style::StyleProfileStore::with_built_ins);

    /// A mock inference provider that returns a fixed string.
    struct MockLlm {
        output: String,
    }
    impl InferenceProvider for MockLlm {
        fn model_id(&self) -> &str {
            "mock"
        }
        fn model_name(&self) -> &str {
            "mock"
        }
        fn generate(
            &self,
            _prompt: &str,
            _params: crate::llm::GenerationParams,
        ) -> Result<String, String> {
            Ok(self.output.clone())
        }
    }

    #[test]
    fn voice_with_transform_and_autotext() {
        let platform = MockPlatform::with_app(normalize("com.google.gmail", "Gmail"));
        let mut autotext = AutoTextStore::default();
        autotext
            .insert(crate::autotext::AutoTextEntry::new(
                "/email",
                "user@example.com",
            ))
            .unwrap();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();
        let llm = MockLlm {
            output: "Please send the updated proposal to {{AUTOTEXT_0}} and let me know when you receive it.".into(),
        };

        let pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: Some(&llm),
            auto_apply: true,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
        };

        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "hey john can you send the updated proposal to /email and let me know when you get a chance".into(),
        };
        let result = pipeline.run(input, None);

        // The email must survive intact.
        assert!(
            result.final_text.contains("user@example.com"),
            "email lost: {}",
            result.final_text
        );
        // The transform ran.
        assert!(result.transformed);
        // Context was detected.
        assert_eq!(
            result.context.application_type,
            crate::context::AppType::Email
        );
    }

    #[test]
    fn typed_input_expands_autotext_without_ai() {
        let platform = MockPlatform::with_app(ApplicationContext::unknown());
        let mut autotext = AutoTextStore::default();
        autotext
            .insert(crate::autotext::AutoTextEntry::new("/name", "Ali Husain"))
            .unwrap();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();

        let pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: None,
            auto_apply: false,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
        };

        let input = UnifiedInput {
            source: InputSource::Typed,
            text: "regards /name".into(),
        };
        let result = pipeline.run(input, None);
        assert_eq!(result.final_text, "regards Ali Husain");
        assert!(!result.transformed);
        assert!(result.autotext_expanded);
    }

    #[test]
    fn no_model_falls_back_to_expanded_input() {
        let platform = MockPlatform::with_app(normalize("com.google.gmail", "Gmail"));
        let mut autotext = AutoTextStore::default();
        autotext
            .insert(crate::autotext::AutoTextEntry::new(
                "/email",
                "user@example.com",
            ))
            .unwrap();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();

        let pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: None, // no model
            auto_apply: true,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
        };

        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "send to /email".into(),
        };
        let result = pipeline.run(input, None);
        // No model: AutoText still expands, no transform.
        assert_eq!(result.final_text, "send to user@example.com");
        assert!(!result.transformed);
    }

    #[test]
    fn voice_expands_spoken_snippets() {
        let platform = MockPlatform::with_app(ApplicationContext::unknown());
        let mut autotext = AutoTextStore::default();
        let mut e = crate::autotext::AutoTextEntry::new("/email", "abcd@gmail.com");
        e.snippet = "my email".into();
        autotext.insert(e).unwrap();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();

        let pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: None,
            auto_apply: false,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
        };

        // Voice: the spoken phrase expands even though there's no `/trigger`.
        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "my email please".into(),
        };
        let result = pipeline.run(input, None);
        assert_eq!(result.final_text, "abcd@gmail.com please");
        assert!(!result.transformed);

        // Typed input does NOT expand snippets (only spoken).
        let input = UnifiedInput {
            source: InputSource::Typed,
            text: "my email please".into(),
        };
        let result = pipeline.run(input, None);
        assert_eq!(result.final_text, "my email please");
    }

    #[test]
    fn inference_failure_preserves_input() {
        struct FailLlm;
        impl InferenceProvider for FailLlm {
            fn model_id(&self) -> &str {
                "fail"
            }
            fn model_name(&self) -> &str {
                "fail"
            }
            fn generate(&self, _: &str, _: crate::llm::GenerationParams) -> Result<String, String> {
                Err("model crashed".into())
            }
        }
        let platform = MockPlatform::with_app(ApplicationContext::unknown());
        let autotext = AutoTextStore::default();
        let transforms = TransformStore::with_built_ins();
        let profile = UserProfile::default();
        let llm = FailLlm;

        let pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: Some(&llm),
            auto_apply: true,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
        };

        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "my important words".into(),
        };
        let result = pipeline.run(input, None);
        assert_eq!(result.final_text, "my important words");
        assert!(!result.transformed);
    }

    #[test]
    fn explicit_instruction_overrides_context() {
        // The Rewriter's user_instruction is the explicit instruction.
        // Here we just verify the pipeline passes it through.
        let platform = MockPlatform::with_app(normalize("com.google.gmail", "Gmail"));
        let autotext = AutoTextStore::default();
        let transforms = TransformStore::with_built_ins();
        let mut rewriter = transforms.get("builtin-rewriter").unwrap().clone();
        rewriter.auto_apply = false;
        let profile = UserProfile::default();

        let llm = MockLlm {
            output: "Make it super casual, bro!".into(),
        };

        let pipeline = Pipeline {
            platform: &platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: Some(&llm),
            auto_apply: false,
            restore_clipboard: true,
            remove_filler_words: false,
            filler_words: vec![],
            dictionary: &DICT,
            styles: &STYLES,
            active_style: "",
        };

        let input = UnifiedInput {
            source: InputSource::Voice,
            text: "formal text here".into(),
        };
        let result = pipeline.run(input, Some(&rewriter));
        assert!(result.transformed);
        assert_eq!(result.final_text, "Make it super casual, bro!");
    }
}
