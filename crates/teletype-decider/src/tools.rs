//! The v1 tool set as data.
//!
//! These are the 10 tools the NeuDecide model can emit. Enum values for
//! apps / snippets / styles are injected at runtime (top 30 by usage);
//! the other enums (transforms, languages, media actions, edit actions)
//! are fixed in the v1 spec.

use crate::tool_json::ToolDef;

/// Fixed enums from the v1 spec.
pub mod enums {
    /// Text transforms for `transform_selection`.
    pub const TRANSFORMS: &[&str] = &["uppercase", "lowercase", "titlecase", "capitalize"];
    /// Languages for `transform_selection`.
    pub const LANGUAGES: &[&str] = &["en", "fr", "de", "ja"];
    /// Media actions for `media`.
    pub const MEDIA_ACTIONS: &[&str] = &[
        "play_pause",
        "next",
        "previous",
        "volume_up",
        "volume_down",
        "mute",
    ];
    /// Edit actions for `edit_text`.
    pub const EDIT_ACTIONS: &[&str] = &[
        "undo_last_dictation",
        "delete_last_sentence",
        "new_line",
        "new_paragraph",
        "select_all",
    ];
}

/// Build the 10 v1 [`ToolDef`]s with runtime-injected enum values.
///
/// See [`crate::tool_json::v1_tools`] for the implementation; this is a
/// convenience re-export so callers can say `tools::v1_tools(...)`.
pub fn v1_tools(apps: &[String], snippets: &[String], styles: &[String]) -> Vec<ToolDef> {
    crate::tool_json::v1_tools(apps, snippets, styles)
}
