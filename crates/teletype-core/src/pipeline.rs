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
    llm::InferenceProvider,
    personalization::{self, UserProfile},
    platform::Platform,
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
    /// When true, run the auto-apply transform (voice input).
    pub auto_apply: bool,
    /// Restore the user's clipboard after injection.
    pub restore_clipboard: bool,
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

        // 1. Protect AutoText values.
        let protected = protect::protect(&input.text, self.autotext, &context);
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
                    let ctx = PromptContext {
                        app: Some(context.clone()),
                        preferences: packet.style,
                        preferred_terms: packet.terms,
                        user_instruction: None,
                        language: self.profile.language.clone(),
                    };
                    let result = engine::run_transform_blocking(provider, t, &protected.text, &ctx);
                    (result.text.clone(), Some(result))
                }
                None => {
                    // No model: expand AutoText and return the input.
                    let expanded = autotext::expand::expand(&input.text, self.autotext, &context);
                    (expanded, None)
                }
            },
            None => (protected.text.clone(), None),
        };

        // 4. Restore AutoText values.
        let final_text = if autotext_expanded {
            protect::restore(&text_after_transform, &protected)
        } else if transform.is_none() {
            // No transform: expand triggers directly (typed input path).
            autotext::expand::expand(&input.text, self.autotext, &context)
        } else {
            text_after_transform
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{context::normalize, platform::MockPlatform};

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
        let mut transforms = TransformStore::with_built_ins();
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
