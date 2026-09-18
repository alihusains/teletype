//! Building the compact preference packet sent to the model.
//!
//! Only relevant preferences are included, in priority order. The packet is
//! small by construction — the model never sees the whole profile.

use super::{context_baseline, UserProfile};
use crate::context::ApplicationContext;

/// The resolved set of style cues for one transform.
#[derive(Debug, Clone, Default)]
pub struct PreferencePacket {
    /// Effective style phrases, highest priority first.
    pub style: Vec<String>,
    /// Preferred terminology phrases.
    pub terms: Vec<String>,
}

/// Resolves the effective packet for a context.
///
/// Priority: explicit preferences > learned preferences > context baseline.
/// (An explicit instruction *in the current input* is handled by the caller —
/// e.g. the Rewriter's user instruction — and always wins.)
pub fn resolve(profile: &UserProfile, app: &ApplicationContext) -> PreferencePacket {
    let mut style: Vec<String> = Vec::new();
    let mut terms: Vec<String> = Vec::new();

    if profile.enabled() {
        for p in profile.relevant(app) {
            if p.description.to_lowercase().contains("term") || p.phrase.contains("instead of") {
                terms.push(p.phrase.clone());
            } else {
                style.push(p.phrase.clone());
            }
        }
    }

    // Context baseline fills gaps only.
    if app.is_known() {
        for base in context_baseline(app) {
            if !style.iter().any(|s| s.to_lowercase().contains(base)) {
                style.push(base.to_string());
            }
        }
    }

    PreferencePacket { style, terms }
}

/// The packet when personalization is entirely disabled.
pub fn disabled() -> PreferencePacket {
    PreferencePacket::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::personalization::Preference;

    #[test]
    fn explicit_and_baseline_combine() {
        let mut profile = UserProfile::default();
        profile.add(Preference::new_explicit("Keep it concise", "be concise"));
        let gmail = crate::context::normalize("com.google.gmail", "Gmail");
        let packet = resolve(&profile, &gmail);
        assert_eq!(packet.style[0], "be concise");
        // Baseline adds professional/friendly but not a duplicate "concise".
        assert!(packet.style.contains(&"professional".to_string()));
        assert!(packet.style.contains(&"friendly".to_string()));
        assert_eq!(
            packet
                .style
                .iter()
                .filter(|s| s.contains("concise"))
                .count(),
            1
        );
    }

    #[test]
    fn term_preferences_go_to_terms() {
        let mut profile = UserProfile::default();
        profile.add(Preference::new_explicit(
            "Use customer instead of client",
            "use 'customer' instead of 'client'",
        ));
        let packet = resolve(&profile, &crate::context::ApplicationContext::unknown());
        assert_eq!(packet.terms, vec!["use 'customer' instead of 'client'"]);
        assert!(packet.style.is_empty());
    }

    #[test]
    fn unknown_app_gets_no_baseline() {
        let profile = UserProfile::default();
        let packet = resolve(&profile, &ApplicationContext::unknown());
        assert!(packet.style.is_empty());
    }
}
