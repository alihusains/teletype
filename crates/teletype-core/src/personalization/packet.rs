//! Building the compact preference packet sent to the model.
//!
//! Only relevant preferences are included, in priority order. The packet is
//! small by construction — the model never sees the whole profile.

use super::{context_baseline, UserProfile};
use crate::context::ApplicationContext;

/// The resolved set of style cues for one transform.
#[derive(Debug, Clone, Default, PartialEq)]
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
    resolve_with_style_rules(profile, app, &[])
}

/// Like `resolve`, but also merges the promoted style rules for the take's
/// app category (e.g. `("chat", "salutation", "hey")`), in store order.
///
/// Each rule is mapped to a short preference phrase and merged at the
/// *learned* priority: after explicit/learned profile preferences, before
/// the context baseline. A rule whose axis an explicit setting already covers
/// is dropped; a rule whose value is just the baseline default adds nothing
/// and is dropped too. An empty slice behaves exactly like `resolve`.
pub fn resolve_with_style_rules(
    profile: &UserProfile,
    app: &ApplicationContext,
    style_rules: &[crate::learning::store::StyleRuleRow],
) -> PreferencePacket {
    let mut style: Vec<String> = Vec::new();
    let mut terms: Vec<String> = Vec::new();

    if profile.enabled() {
        for p in profile
            .relevant(app)
            .into_iter()
            .filter(|p| p.explicit || p.count >= 2)
        {
            if p.description.to_lowercase().contains("term") || p.phrase.contains("instead of") {
                terms.push(p.phrase.clone());
            } else {
                style.push(p.phrase.clone());
            }
        }
    }

    // Learned style rules (T09): promoted rows for this app category, merged
    // at learned priority — below profile preferences (explicit or learned),
    // above the context baseline. A rule whose axis an explicit setting
    // already covers is dropped; a rule whose value is just the baseline
    // default adds nothing and is dropped too.
    for rule in style_rules {
        let Some(phrase) = phrase_for_rule(&rule.rule, &rule.value) else {
            continue;
        };
        let covered = style.iter().any(|s| {
            let s = s.to_lowercase();
            // Same axis ("greet" covers "greet with 'hey'") or the same
            // concrete value ("sign off with 'Cheers'" covers the learned
            // "cheers").
            rule_axis(&rule.rule).is_some_and(|axis| s.contains(axis))
                || rule
                    .value
                    .to_lowercase()
                    .split_whitespace()
                    .any(|w| w.len() >= 3 && s.contains(&format!("'{w}'")))
        });
        if covered {
            continue;
        }
        if app.is_known()
            && context_baseline(app)
                .iter()
                .any(|base| rule.value.to_lowercase().contains(base))
        {
            continue;
        }
        style.push(phrase);
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

/// The prompt axis a rule phrase is written for (its leading verb), so an
/// explicit setting on the same axis can be recognized as covering it.
fn rule_axis(rule: &str) -> Option<&'static str> {
    match rule {
        "greeting" | "salutation" => Some("greet"),
        "signoff" | "sign-off" => Some("sign off"),
        "contraction" => Some("contractions"),
        "list" => Some("lists"),
        _ => None,
    }
}

/// Maps one promoted style rule to its short prompt phrase, or `None` when
/// the rule axis is unknown (future rule kinds are ignored until mapped).
fn phrase_for_rule(rule: &str, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    match rule {
        // A learned greeting: the value is the salutation the user settles on.
        "greeting" | "salutation" => Some(format!("greet with '{value}'")),
        // A learned sign-off: the value is the closing, or "none" when the
        // user consistently deletes it.
        "signoff" | "sign-off" => {
            if value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("drop") {
                Some("drop the sign-off".to_string())
            } else {
                Some(format!("sign off with '{value}'"))
            }
        }
        "contraction" => Some(format!("{value} contractions")),
        "list" => Some(format!("use {value} lists")),
        _ => None,
    }
}

/// The packet when personalization is entirely disabled.
pub fn disabled() -> PreferencePacket {
    PreferencePacket::default()
}

#[cfg(test)]
mod tests {
    use super::super::PreferenceScope;
    use super::*;
    use crate::learning::store::StyleRuleRow;
    use crate::personalization::Preference;

    fn rule(rule: &str, value: &str) -> StyleRuleRow {
        StyleRuleRow {
            app_category: "chat".into(),
            rule: rule.into(),
            value: value.into(),
            evidence: 3,
            updated: 1_000,
        }
    }

    fn chat() -> ApplicationContext {
        crate::context::normalize("com.tinyspeck.slackmacgap", "Slack")
    }

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
        let packet = resolve(&profile, &ApplicationContext::unknown());
        assert_eq!(packet.terms, vec!["use 'customer' instead of 'client'"]);
        assert!(packet.style.is_empty());
    }

    #[test]
    fn unknown_app_gets_no_baseline() {
        let profile = UserProfile::default();
        let packet = resolve(&profile, &ApplicationContext::unknown());
        assert!(packet.style.is_empty());
    }

    #[test]
    fn learned_style_rule_applies_only_in_its_category() {
        // "In chat, drop the sign-off" is learned and promoted: the phrase
        // must reach the chat packet and must not leak into the email one.
        let profile = UserProfile::default();
        let rules = vec![rule("signoff", "none")];

        let chat_packet = resolve_with_style_rules(&profile, &chat(), &rules);
        assert!(
            chat_packet.style.contains(&"drop the sign-off".to_string()),
            "chat packet lost the learned rule: {:?}",
            chat_packet.style
        );

        let gmail = crate::context::normalize("com.google.gmail", "Gmail");
        let email_packet = resolve(&profile, &gmail);
        assert!(!email_packet.style.iter().any(|s| s.contains("sign-off")));
    }

    #[test]
    fn explicit_setting_beats_learned_rule_on_same_axis() {
        // The user has explicitly stated a greeting preference; a learned
        // greeting rule for the same axis must not also reach the packet.
        let mut profile = UserProfile::default();
        profile.add(Preference::new_explicit(
            "Greet with 'Hi'",
            "greet with 'Hi'",
        ));
        let rules = vec![rule("salutation", "hey")];

        let packet = resolve_with_style_rules(&profile, &chat(), &rules);
        // The explicit phrase is first (top priority) and the learned rule
        // on the same axis is gone; only the baseline gap-fills remain.
        assert_eq!(packet.style[0], "greet with 'Hi'");
        assert!(
            packet.style.iter().all(|s| !s.contains("'hey'")),
            "learned rule must not duplicate the explicit axis: {:?}",
            packet.style
        );
    }

    #[test]
    fn learned_rule_beats_context_baseline() {
        // A learned salutation outranks the chat baseline in the packet
        // order (learned priority sits above the baseline).
        let profile = UserProfile::default();
        let rules = vec![rule("salutation", "hey")];
        let packet = resolve_with_style_rules(&profile, &chat(), &rules);
        let learned = packet.style.iter().position(|s| s.contains("'hey'"));
        let baseline = packet.style.iter().position(|s| s == "conversational");
        assert!(
            learned.is_some() && baseline.is_some() && learned < baseline,
            "learned rule must precede the baseline: {:?}",
            packet.style
        );
    }

    #[test]
    fn learned_rule_matching_baseline_is_dropped() {
        // "In chat, be concise" is learned, but concise is already the chat
        // baseline: the rule adds nothing and must not duplicate it.
        let profile = UserProfile::default();
        let rules = vec![rule("signoff", "concise")];
        let packet = resolve_with_style_rules(&profile, &chat(), &rules);
        assert_eq!(
            packet
                .style
                .iter()
                .filter(|s| s.contains("concise"))
                .count(),
            1,
            "{:?}",
            packet.style
        );
    }

    #[test]
    fn unknown_rule_axis_is_ignored() {
        // A future rule kind is skipped, not guessed at.
        let profile = UserProfile::default();
        let rules = vec![rule("tone", "casual")];
        let packet = resolve_with_style_rules(&profile, &chat(), &rules);
        assert!(packet.style.iter().all(|s| !s.contains("casual")));
    }

    #[test]
    fn empty_style_rules_match_today() {
        // Snapshot of today's resolved packets: an empty rule slice must
        // reproduce them exactly.
        let mut profile = UserProfile::default();
        profile.add(Preference::new_explicit("Keep it concise", "be concise"));
        let learned = Preference {
            id: "l1".into(),
            description: "Use 'customer' instead of 'client'".into(),
            phrase: "use 'customer' instead of 'client'".into(),
            explicit: false,
            scope: PreferenceScope::Global,
            count: 4,
            created_at: 0,
            updated_at: 0,
            learned_from: None,
            learned_to: None,
        };
        profile.add(learned);

        for app in [
            crate::context::normalize("com.google.gmail", "Gmail"),
            chat(),
            ApplicationContext::unknown(),
        ] {
            let baseline = resolve(&profile, &app);
            assert_eq!(
                baseline,
                resolve_with_style_rules(&profile, &app, &[]),
                "empty-slice packet must equal the rule-free packet for {:?}",
                app.application_type
            );
            if app.is_known() {
                // The empty-slice packet is the pre-T09 shape: profile
                // phrases first, then the baseline gap-fills, nothing else.
                assert!(
                    baseline.style.iter().all(|s| {
                        s == "be concise"
                            || matches!(
                                s.as_str(),
                                "professional"
                                    | "friendly"
                                    | "concise"
                                    | "conversational"
                                    | "natural"
                            )
                    }),
                    "{:?}",
                    baseline.style
                );
                assert_eq!(baseline.terms, vec!["use 'customer' instead of 'client'"]);
            }
        }
    }
}
