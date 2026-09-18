//! Application context: which app the user is dictating into.
//!
//! Detection is best-effort and platform-provided; this module normalizes raw
//! identifiers into a stable, testable shape. `Unknown` is a valid answer —
//! the pipeline must never invent context.

use serde::{Deserialize, Serialize};

/// The kind of work the active application is most likely doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AppType {
    Email,
    Chat,
    Social,
    Coding,
    Document,
    Browser,
    Terminal,
    #[default]
    Unknown,
}

/// The active application, normalized.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationContext {
    /// Stable id: bundle id on macOS, process name on Windows, or `""`.
    pub application_id: String,
    /// Human-readable name, e.g. "Gmail".
    pub application_name: String,
    pub application_type: AppType,
    /// Window title when the platform exposes it reliably.
    pub window_title: Option<String>,
    /// 0.0..=1.0 — how confident the detection is.
    pub confidence: f32,
}

impl ApplicationContext {
    pub fn unknown() -> Self {
        Self::default()
    }

    pub fn is_known(&self) -> bool {
        !self.application_id.is_empty() && self.application_type != AppType::Unknown
    }
}

/// Normalizes a raw (id, name) pair into an [`ApplicationContext`].
///
/// The table is deliberately coarse: a few well-known apps get a category,
/// everything else falls back to name heuristics, and anything unrecognized
/// becomes `Unknown` with low confidence.
pub fn normalize(application_id: &str, application_name: &str) -> ApplicationContext {
    let id = application_id.to_ascii_lowercase();
    let name = application_name.to_ascii_lowercase();

    let (application_type, confidence) = classify(&id, &name);
    let name = if application_name.is_empty() {
        application_id.to_string()
    } else {
        application_name.to_string()
    };
    ApplicationContext {
        application_id: application_id.to_string(),
        application_name: name,
        application_type,
        window_title: None,
        confidence,
    }
}

fn classify(id: &str, name: &str) -> (AppType, f32) {
    // Known apps by bundle id / process name.
    const EMAIL: &[&str] = &[
        "com.google.gmail",
        "gmail",
        "com.microsoft.outlook",
        "outlook",
        "thunderbird",
        "com.apple.mail",
        "mail",
    ];
    const CHAT: &[&str] = &[
        "slack",
        "com.slackmac.slack",
        "com.slackmac.bundled",
        "teams",
        "com.microsoft.teams",
        "com.tencent.qq",
        "discord",
        "com.discord.app",
        "com.apple.imessage",
        "messages",
        "telegram",
        "com.telegram.desktop",
        "com.protonmail.messenger",
        "whatsapp",
    ];
    const SOCIAL: &[&str] = &[
        "com.linkedin.linkedin",
        "linkedin",
        "com.twitter.tweetdeck",
        "x",
        "mastodon",
        "com.tencent.wechat",
        "wechat",
    ];
    const CODING: &[&str] = &[
        "com.microsoft.vslite",
        "code",
        "com.jetbrains.intellij",
        "intellij",
        "com.jetbrains.pycharm",
        "pycharm",
        "com.apple.dt.xcode",
        "xcode",
        "org.sublimehq.sublimetext",
        "subl",
        "com.google.itools.ide",
        "androidstudio",
        "com.torusoid.sublime_merge",
        "gitkraken",
        "com.github.stove",
        "zed",
        "dev.zed.Zed",
        "com.rustrover.rustrover",
        "clion",
        "goland",
    ];
    const DOCUMENT: &[&str] = &[
        "com.microsoft.word",
        "word",
        "com.apple.pages",
        "pages",
        "com.google.docs",
        "docs",
        "com.notion.id",
        "notion",
        "com.apple.keynote",
        "keynote",
        "com.microsoft.powerpoint",
        "powerpoint",
        "com.microsoft.excel",
        "excel",
        "com.apple.numbers",
        "numbers",
        "com.apple.textedit",
        "textedit",
        "com.obsidian",
        "obsidian",
        "com.obsidian.mac",
        "com.affinity.publisher",
    ];
    const BROWSER: &[&str] = &[
        "com.apple.safari",
        "safari",
        "com.google.chrome",
        "chrome",
        "com.mozilla.firefox",
        "firefox",
        "company.thebrowser.browser",
        "arc",
        "com.brave.Browser",
        "brave",
        "com.vivaldi.vivaldi",
        "vivaldi",
        "com.microsoft.edgemac",
        "msedge",
        "com.operasoftware.Opera",
        "opera",
    ];
    const TERMINAL: &[&str] = &[
        "com.apple.terminal",
        "terminal",
        "com.googlecode.iterm2",
        "iterm",
        "dev.warp.Warp-Stable",
        "warp",
        "com.mitchellh.ghostty",
        "ghostty",
        "dev.zed.Zed",
        "com.torch.openconsole",
        "com.microsoft.vscode",
    ];

    let hay = format!("{id}\x1f{name}");
    let tokens: Vec<&str> = hay.split('\x1f').filter(|s| !s.is_empty()).collect();
    let find = |list: &[&str]| list.iter().any(|k| tokens.iter().any(|t| t == k));

    if find(EMAIL) {
        (AppType::Email, 0.95)
    } else if find(CHAT) {
        (AppType::Chat, 0.95)
    } else if find(SOCIAL) {
        (AppType::Social, 0.9)
    } else if find(CODING) {
        (AppType::Coding, 0.95)
    } else if find(DOCUMENT) {
        (AppType::Document, 0.9)
    } else if find(BROWSER) {
        (AppType::Browser, 0.9)
    } else if find(TERMINAL) {
        (AppType::Terminal, 0.9)
    } else if id.is_empty() && name.is_empty() {
        (AppType::Unknown, 0.0)
    } else {
        // Detected an app but don't know its category.
        (AppType::Unknown, 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_apps_get_categories() {
        assert_eq!(
            normalize("com.google.Chrome", "Google Chrome").application_type,
            AppType::Browser
        );
        assert_eq!(
            normalize("com.google.gmail", "Gmail").application_type,
            AppType::Email
        );
        assert_eq!(normalize("Slack", "slack").application_type, AppType::Chat);
        assert_eq!(
            normalize("com.microsoft.VSCode", "Code").application_type,
            AppType::Coding
        );
        assert_eq!(
            normalize("com.apple.Terminal", "Terminal").application_type,
            AppType::Terminal
        );
        assert_eq!(
            normalize("com.notion.id", "Notion").application_type,
            AppType::Document
        );
        assert_eq!(
            normalize("com.linkedin.LinkedIn", "LinkedIn").application_type,
            AppType::Social
        );
    }

    #[test]
    fn unknown_app_is_honest() {
        let ctx = normalize("com.example.foo", "Foo");
        assert_eq!(ctx.application_type, AppType::Unknown);
        assert!(ctx.confidence < 0.9);
    }

    #[test]
    fn empty_is_unknown() {
        let ctx = normalize("", "");
        assert!(!ctx.is_known());
        assert_eq!(ctx.confidence, 0.0);
    }

    #[test]
    fn gmail_via_name_only() {
        let ctx = normalize("", "Gmail");
        assert_eq!(ctx.application_type, AppType::Email);
    }
}
