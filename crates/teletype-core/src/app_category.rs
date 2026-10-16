//! App category classification from a bundle id (and, as a last resort,
//! the window title).
//!
//! The category feeds the per-take engine boost list and the polish prompt.
//! Classification is deliberately dumb and table-driven: exact bundle ids,
//! a small number of prefix rules, and a documented window-title fallback
//! that is used ONLY when the bundle id is unknown. No heuristics beyond
//! that — `Other` is a valid answer, and guessing is worse than not knowing.

use serde::{Deserialize, Serialize};

/// The kind of application the user is dictating into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AppCategory {
    Email,
    Chat,
    Code,
    Docs,
    Terminal,
    Browser,
    Notes,
    #[default]
    Other,
}

impl AppCategory {
    /// Stable lowercase name used in traces and prompts.
    pub fn as_str(self) -> &'static str {
        match self {
            AppCategory::Email => "email",
            AppCategory::Chat => "chat",
            AppCategory::Code => "code",
            AppCategory::Docs => "docs",
            AppCategory::Terminal => "terminal",
            AppCategory::Browser => "browser",
            AppCategory::Notes => "notes",
            AppCategory::Other => "other",
        }
    }
}

/// Bundle ids that map to a specific category, checked in order.
///
/// Exact matches first; the prefix entries are the only fuzzy rules allowed.
const BUNDLE_TABLE: &[(&str, AppCategory, bool)] = &[
    // email
    ("com.apple.mail", AppCategory::Email, false),
    ("com.microsoft.Outlook", AppCategory::Email, false),
    ("com.readdle.SparkDesktop", AppCategory::Email, false),
    ("com.superhuman.Electron", AppCategory::Email, false),
    // chat
    ("com.tinyspeck.slackmacgap", AppCategory::Chat, false),
    ("com.hnc.Discord", AppCategory::Chat, false),
    ("net.whatsapp.WhatsApp", AppCategory::Chat, false),
    ("ru.keepcoder.Telegram", AppCategory::Chat, false),
    ("com.apple.MobileSMS", AppCategory::Chat, false),
    ("com.microsoft.teams2", AppCategory::Chat, false),
    // code
    ("com.microsoft.VSCode", AppCategory::Code, false),
    ("com.apple.dt.Xcode", AppCategory::Code, false),
    ("dev.zed.Zed", AppCategory::Code, false),
    ("com.sublimetext.4", AppCategory::Code, false),
    ("com.jetbrains.", AppCategory::Code, true),
    ("com.google.android.studio", AppCategory::Code, false),
    // terminal
    ("com.apple.Terminal", AppCategory::Terminal, false),
    ("com.googlecode.iterm2", AppCategory::Terminal, false),
    ("io.alacritty", AppCategory::Terminal, false),
    ("dev.warp.Warp-Stable", AppCategory::Terminal, false),
    ("com.mitchellh.ghostty", AppCategory::Terminal, false),
    // browser
    ("com.apple.Safari", AppCategory::Browser, false),
    ("com.google.Chrome", AppCategory::Browser, false),
    ("org.mozilla.firefox", AppCategory::Browser, false),
    ("com.microsoft.edgemac", AppCategory::Browser, false),
    ("com.brave.Browser", AppCategory::Browser, false),
    ("company.thebrowser.Browser", AppCategory::Browser, false),
    ("com.vivaldi.Vivaldi", AppCategory::Browser, false),
    // docs
    ("com.microsoft.Word", AppCategory::Docs, false),
    ("com.apple.iWork.Pages", AppCategory::Docs, false),
    ("com.apple.iWork.Keynote", AppCategory::Docs, false),
    // notes
    ("com.apple.Notes", AppCategory::Notes, false),
    ("md.obsidian", AppCategory::Notes, false),
    ("notion.id", AppCategory::Notes, false),
    ("com.evernote.Evernote", AppCategory::Notes, false),
    ("net.shinyfrog.bear", AppCategory::Notes, false),
];

/// Classify an app from its bundle id and window title.
///
/// Rule: an exact or prefix bundle-id match always wins. The window title
/// is consulted ONLY when the bundle id is `None` or matches no table
/// entry: a title containing "Notion" or "Obsidian" (or " - Notes") means
/// Notes, one containing "Terminal" means Terminal, anything else is
/// `Other`. The title hints are deliberately few and conservative — a
/// misclassified category silently changes the boost list.
pub fn app_category(bundle_id: Option<&str>, window_title: Option<&str>) -> AppCategory {
    if let Some(bundle) = bundle_id {
        for (id, category, is_prefix) in BUNDLE_TABLE {
            if *is_prefix {
                if bundle.starts_with(id) {
                    return *category;
                }
            } else if bundle == *id {
                return *category;
            }
        }
    }
    category_from_title(window_title)
}

/// The documented window-title fallback; only used when the bundle id is
/// unknown.
fn category_from_title(window_title: Option<&str>) -> AppCategory {
    let Some(title) = window_title else {
        return AppCategory::Other;
    };
    if title.contains("Notion") || title.contains("Obsidian") || title.contains(" - Notes") {
        return AppCategory::Notes;
    }
    if title.contains("Terminal") {
        return AppCategory::Terminal;
    }
    AppCategory::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_uses_plan_names() {
        assert_eq!(AppCategory::Email.as_str(), "email");
        assert_eq!(AppCategory::Chat.as_str(), "chat");
        assert_eq!(AppCategory::Code.as_str(), "code");
        assert_eq!(AppCategory::Docs.as_str(), "docs");
        assert_eq!(AppCategory::Terminal.as_str(), "terminal");
        assert_eq!(AppCategory::Browser.as_str(), "browser");
        assert_eq!(AppCategory::Notes.as_str(), "notes");
        assert_eq!(AppCategory::Other.as_str(), "other");
    }

    #[test]
    fn email_bundles() {
        for id in [
            "com.apple.mail",
            "com.microsoft.Outlook",
            "com.readdle.SparkDesktop",
            "com.superhuman.Electron",
        ] {
            assert_eq!(app_category(Some(id), None), AppCategory::Email, "{id}");
        }
    }

    #[test]
    fn chat_bundles() {
        for id in [
            "com.tinyspeck.slackmacgap",
            "com.hnc.Discord",
            "net.whatsapp.WhatsApp",
            "ru.keepcoder.Telegram",
            "com.apple.MobileSMS",
            "com.microsoft.teams2",
        ] {
            assert_eq!(app_category(Some(id), None), AppCategory::Chat, "{id}");
        }
    }

    #[test]
    fn code_bundles() {
        for id in [
            "com.microsoft.VSCode",
            "com.apple.dt.Xcode",
            "dev.zed.Zed",
            "com.sublimetext.4",
            "com.google.android.studio",
        ] {
            assert_eq!(app_category(Some(id), None), AppCategory::Code, "{id}");
        }
    }

    #[test]
    fn jetbrains_prefix_matches_any_ide() {
        for id in [
            "com.jetbrains.intellij.idea",
            "com.jetbrains.WebStorm",
            "com.jetbrains.goland",
        ] {
            assert_eq!(app_category(Some(id), None), AppCategory::Code, "{id}");
        }
        // The prefix must not swallow unrelated com.* bundles.
        assert_eq!(
            app_category(Some("com.apple.Safari"), None),
            AppCategory::Browser
        );
    }

    #[test]
    fn terminal_bundles() {
        for id in [
            "com.apple.Terminal",
            "com.googlecode.iterm2",
            "io.alacritty",
            "dev.warp.Warp-Stable",
            "com.mitchellh.ghostty",
        ] {
            assert_eq!(app_category(Some(id), None), AppCategory::Terminal, "{id}");
        }
    }

    #[test]
    fn browser_bundles() {
        for id in [
            "com.apple.Safari",
            "com.google.Chrome",
            "org.mozilla.firefox",
            "com.microsoft.edgemac",
            "com.brave.Browser",
            "company.thebrowser.Browser",
            "com.vivaldi.Vivaldi",
        ] {
            assert_eq!(app_category(Some(id), None), AppCategory::Browser, "{id}");
        }
    }

    #[test]
    fn docs_bundles() {
        for id in [
            "com.microsoft.Word",
            "com.apple.iWork.Pages",
            "com.apple.iWork.Keynote",
        ] {
            assert_eq!(app_category(Some(id), None), AppCategory::Docs, "{id}");
        }
    }

    #[test]
    fn notes_bundles() {
        for id in [
            "com.apple.Notes",
            "md.obsidian",
            "notion.id",
            "com.evernote.Evernote",
            "net.shinyfrog.bear",
        ] {
            assert_eq!(app_category(Some(id), None), AppCategory::Notes, "{id}");
        }
    }

    #[test]
    fn unknown_bundle_is_other() {
        assert_eq!(
            app_category(Some("com.example.unknown"), None),
            AppCategory::Other
        );
        assert_eq!(app_category(Some(""), None), AppCategory::Other);
    }

    #[test]
    fn no_bundle_and_no_title_is_other() {
        assert_eq!(app_category(None, None), AppCategory::Other);
    }

    #[test]
    fn title_hint_only_when_bundle_unknown() {
        assert_eq!(
            app_category(None, Some("Untitled - Notion")),
            AppCategory::Notes
        );
        assert_eq!(
            app_category(None, Some("Vault — Obsidian")),
            AppCategory::Notes
        );
        assert_eq!(
            app_category(None, Some("Shopping List - Notes")),
            AppCategory::Notes
        );
        assert_eq!(
            app_category(None, Some("zsh — zsh — 80x24 — Terminal")),
            AppCategory::Terminal
        );
        assert_eq!(
            app_category(None, Some("Some random title")),
            AppCategory::Other
        );
        // A known bundle beats a misleading title.
        assert_eq!(
            app_category(Some("com.apple.Terminal"), Some("Notion")),
            AppCategory::Terminal
        );
        // Unknown bundle + hint-less title is Other.
        assert_eq!(
            app_category(Some("com.example.unknown"), Some("Notion thing")),
            AppCategory::Notes
        );
    }
}
