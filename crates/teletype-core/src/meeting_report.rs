//! Self-contained HTML report for a finished meeting.
//!
//! The report is a single file with inline CSS and no JavaScript: no
//! external requests, no fonts, no build step. It can be opened from disk,
//! emailed, or archived next to the meeting audio.

use serde::{Deserialize, Serialize};

use crate::meeting::{format_clock, MeetingTranscript};
use crate::rubric::{Rubric, RubricScore};

/// LLM-derived highlights, when a provider ran the map-reduce pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReportSummary {
    pub summary: String,
    pub action_items: Vec<String>,
}

/// Speaker label palette: assigned by first appearance, so the same speaker
/// always gets the same color in one report.
const SPEAKER_COLORS: [&str; 8] = [
    "#0b6e4f",
    "#8a3ffc",
    "#b3261e",
    "#1256a3",
    "#8a6d00",
    "#a3326b",
    "#0f766e",
    "#57534e",
];

/// Light background tints matching `SPEAKER_COLORS` (avatar fills).
const SPEAKER_TINTS: [&str; 8] = [
    "#e6f2ec",
    "#f1e9fd",
    "#f9e9e8",
    "#e8eff8",
    "#f5f0dd",
    "#f8e9f1",
    "#e6f2f1",
    "#efedeb",
];

/// Avatar initial for a speaker: "Speaker 3" → "3", "Alice" → "A".
fn speaker_initial(name: &str) -> String {
    let trimmed = name.trim();
    if let Some(rest) = trimmed.strip_prefix("Speaker ") {
        rest.chars()
            .next()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".into())
    } else {
        trimmed
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".into())
    }
}

/// Escapes text for safe inclusion in HTML.
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Renders the report. `started_at` is a pre-formatted local time string
/// (the core stays free of time-zone handling).
///
/// When `live` is true the report is a still-updating document (a meeting
/// is still recording): a no-JS `<meta http-equiv="refresh">` makes a
/// browser tab re-read the file every few seconds, and a LIVE badge marks
/// the header — the report can be opened and watched while it runs.
///
/// Layout: header → **Overview** (the LLM summary when available, else a
/// mechanical "what happened" line) → **Action items** → **Score** (when a
/// rubric was selected: the LLM's per-criterion scores, or the rubric
/// listed unscored) → **Details** (start / duration / words / WPM /
/// segments / speakers / language / engine) → **Transcript** → footer.
pub fn render_report(
    title: &str,
    started_at: &str,
    transcript: &MeetingTranscript,
    summary: Option<&ReportSummary>,
    rubric: Option<&Rubric>,
    rubric_scores: Option<&RubricScore>,
    live: bool,
) -> String {
    // Unique speakers by first appearance: stable color + the Details row.
    let speakers: Vec<String> = {
        let mut seen: Vec<String> = Vec::new();
        for s in transcript
            .segments
            .iter()
            .filter_map(|s| s.speaker.clone())
        {
            if !seen.contains(&s) {
                seen.push(s);
            }
        }
        seen
    };
    let speaker_color = |name: &str| -> &'static str {
        let idx = speakers.iter().position(|s| s == name).unwrap_or(0);
        SPEAKER_COLORS[idx % SPEAKER_COLORS.len()]
    };
    let speaker_tint = |name: &str| -> &'static str {
        let idx = speakers.iter().position(|s| s == name).unwrap_or(0);
        SPEAKER_TINTS[idx % SPEAKER_TINTS.len()]
    };

    let word_count = transcript.words.len();
    let wpm = if transcript.duration_ms > 0 {
        (word_count as f64 / (transcript.duration_ms as f64 / 60_000.0)) as i64
    } else {
        0
    };

    let mut html = String::with_capacity(64 * 1024);
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    html.push_str("<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    if live {
        // No JS allowed in the report; a meta refresh keeps an open tab
        // current while the meeting is still recording.
        html.push_str("<meta http-equiv=\"refresh\" content=\"3\">\n");
    }
    html.push_str(&format!("<title>{}</title>\n", escape_html(title)));
    html.push_str("<style>\n");
    html.push_str(
        "body{font-family:Inter,-apple-system,BlinkMacSystemFont,\"Segoe UI\",Roboto,Helvetica,Arial,sans-serif;\
         margin:0;background:#f4f5f7;color:#070709;line-height:1.6;font-size:15px}\
         .wrap{max-width:840px;margin:0 auto;padding:36px 28px 72px}\
         .live-badge{display:inline-flex;align-items:center;gap:7px;background:#070709;color:#fff;\
         border-radius:100px;padding:6px 14px;font-size:11px;font-weight:600;letter-spacing:.08em;\
         text-transform:uppercase;margin-bottom:14px;box-shadow:rgba(0,0,0,.18) 0 4px 12px}\
         .live-dot{width:7px;height:7px;border-radius:50%;background:#ff453a;animation:live-pulse 1.2s ease-in-out infinite}\
         @keyframes live-pulse{0%,100%{opacity:1}50%{opacity:.3}}\
         .hero{background:linear-gradient(135deg,#f5f1ff 0%,#eaf2fb 55%,#e7f4f0 100%);\
         border:1px solid #e4e0f0;border-radius:18px;padding:30px 32px;margin-bottom:22px}\
         h1{font-size:32px;margin:0 0 16px;font-weight:700;letter-spacing:-.025em;line-height:1.15}\
         .chips{display:flex;flex-wrap:wrap;gap:8px}\
         .chip{background:rgba(255,255,255,.88);border:1px solid rgba(255,255,255,.95);\
         box-shadow:rgba(16,55,132,.06) 0 1px 2px;border-radius:100px;padding:5px 13px;\
         font-size:12.5px;font-weight:550;color:#3c3c43;font-variant-numeric:tabular-nums}\
         .card{background:#fff;border:1px solid #e6e7eb;border-radius:16px;padding:24px 26px;margin-bottom:18px;\
         box-shadow:rgba(16,55,132,.04) 0 1px 3px}\
         .card h2{font-size:12px;text-transform:uppercase;letter-spacing:.09em;color:#60606c;margin:0 0 14px;font-weight:700}\
         .card h2 + h2{margin-top:20px}\
         .overview-card{border-left:4px solid #702fd2}\
         .overview-card p{margin:0 0 12px;font-size:16px;line-height:1.65}\
         .actions{margin:0;padding:0;list-style:none}\
         .actions li{position:relative;padding-left:28px;margin-bottom:9px;font-size:14.5px}\
         .actions li::before{content:\"\";position:absolute;left:2px;top:3.5px;width:14px;height:14px;\
         border-radius:4.5px;background:#f3edfd;border:1.5px solid #702fd2}\
         .score-overall{display:flex;align-items:baseline;gap:10px;margin-bottom:20px}\
         .score-num{font-size:44px;font-weight:750;letter-spacing:-.03em;font-variant-numeric:tabular-nums}\
         .score-of{color:#60606c;font-size:13px;font-weight:550}\
         .crit{margin-bottom:16px}\
         .crit-head{display:flex;justify-content:space-between;gap:12px;margin-bottom:5px}\
         .crit-name{font-size:14px;font-weight:650}\
         .crit-val{font-size:13px;font-weight:700;font-variant-numeric:tabular-nums}\
         .bar{height:6px;border-radius:100px;background:#eef0f3;overflow:hidden}\
         .bar-fill{height:100%;border-radius:100px}\
         .crit-note{font-size:13px;color:#60606c;margin-top:5px;line-height:1.5}\
         .score-list{margin:0 0 4px;padding-left:18px;font-size:14px}\
         .score-list li{margin-bottom:5px}\
         .rubric-list{margin:0;padding-left:18px;font-size:13.5px;color:#60606c}\
         .rubric-list li{margin-bottom:6px}\
         .stats{display:grid;grid-template-columns:repeat(4,1fr);gap:10px;margin-bottom:20px}\
         .stat{background:#faf9fc;border:1px solid #eceaf3;border-radius:14px;padding:14px 12px;text-align:center}\
         .stat-val{font-size:24px;font-weight:700;letter-spacing:-.02em;font-variant-numeric:tabular-nums}\
         .stat-label{font-size:10.5px;text-transform:uppercase;letter-spacing:.08em;color:#60606c;font-weight:700;margin-top:3px}\
         .kv{display:grid;grid-template-columns:130px 1fr;row-gap:8px;column-gap:12px;font-size:14px;margin:0}\
         .kv dt{color:#60606c;font-weight:550}\
         .kv dd{margin:0;font-variant-numeric:tabular-nums;font-weight:500}\
         .seg{display:flex;gap:12px;padding:10px 8px;border-bottom:1px solid #f0f1f4;border-radius:8px}\
         .seg:hover{background:#faf9fc}\
         .seg:last-child{border-bottom:none}\
         .ts{flex:0 0 46px;color:#8b8b8b;font-variant-numeric:tabular-nums;font-size:12.5px;padding-top:8px}\
         .avatar{flex:0 0 32px;width:32px;height:32px;border-radius:50%;display:flex;align-items:center;\
         justify-content:center;font-size:13px;font-weight:700;margin-top:2px}\
         .who{flex:0 0 94px;font-weight:650;font-size:14px;padding-top:7px}\
         .txt{flex:1;white-space:pre-wrap;word-break:break-word;font-size:15px;line-height:1.6;padding-top:7px}\
         .empty{color:#8b8b8b;font-style:italic}\
         footer{color:#8b8b8b;font-size:12.5px;margin-top:34px;display:flex;align-items:center;\
         justify-content:center;gap:8px}\
         .brand-dot{width:7px;height:7px;border-radius:50%;background:#702fd2;display:inline-block}\
         @media (max-width:640px){.stats{grid-template-columns:repeat(2,1fr)}.hero{padding:22px}}\
         @media print{body{background:#fff;font-size:12.5px}.wrap{padding:0;max-width:none}\
         .card{border:none;box-shadow:none;padding:10px 0;break-inside:avoid}\
         .hero{background:#f5f1ff;border:none}\
         .live-badge{display:none}h1{font-size:26px}.seg:hover{background:none}}",
    );
    html.push_str("</style>\n</head>\n<body>\n<div class=\"wrap\">\n");

    if live {
        html.push_str("<div class=\"live-badge\"><span class=\"live-dot\"></span>Live — updates as the meeting continues</div>\n");
    }

    html.push_str("<header>\n");
    html.push_str("<div class=\"hero\">\n");
    html.push_str(&format!("<h1>{}</h1>\n", escape_html(title)));
    html.push_str("<div class=\"chips\">");
    html.push_str(&format!("<span class=\"chip\">{}</span>", escape_html(started_at)));
    html.push_str(&format!(
        "<span class=\"chip\">{} duration</span>",
        format_clock(transcript.duration_ms)
    ));
    html.push_str(&format!("<span class=\"chip\">{word_count} words</span>"));
    if wpm > 0 {
        html.push_str(&format!("<span class=\"chip\">{wpm} wpm</span>"));
    }
    if let Some(lang) = &transcript.language {
        html.push_str(&format!("<span class=\"chip\">{lang}</span>"));
    }
    html.push_str("</div>\n</div>\n</header>\n");

    // Overview: the LLM summary (what the meeting was about) when a
    // provider ran; otherwise a mechanical line so the section is never
    // empty.
    html.push_str("<div class=\"card overview-card\">\n<h2>Overview</h2>\n");
    match summary {
        Some(s) if !s.summary.trim().is_empty() => {
            html.push_str(&format!("<p>{}</p>\n", escape_html(&s.summary)));
        }
        _ => {
            let line = if transcript.segments.is_empty() {
                format!(
                    "This meeting ran for {} with no speech captured.",
                    format_clock(transcript.duration_ms)
                )
            } else {
                format!(
                    "This meeting ran for {} with {word_count} words spoken across {} segment(s) at {wpm} wpm.",
                    format_clock(transcript.duration_ms),
                    transcript.segments.len()
                )
            };
            html.push_str(&format!("<p>{}</p>\n", escape_html(&line)));
        }
    }
    if let Some(s) = summary {
        if !s.action_items.is_empty() {
            html.push_str("<h2>Action items</h2>\n<ul class=\"actions\">\n");
            for item in &s.action_items {
                html.push_str(&format!("<li>{}</li>\n", escape_html(item)));
            }
            html.push_str("</ul>\n");
        }
    }
    html.push_str("</div>\n");

    // Score: the meeting judged against the selected rubric — the LLM's
    // per-criterion scores when a provider ran, else the rubric listed
    // unscored so the user sees what it *would* be judged on.
    if let Some(r) = rubric {
        html.push_str("<div class=\"card\">\n");
        html.push_str(&format!("<h2>Score — {}</h2>\n", escape_html(&r.name)));
        match rubric_scores {
            Some(s) => {
                let overall_color = score_color(s.overall);
                html.push_str("<div class=\"score-overall\">\n");
                html.push_str(&format!(
                    "<span class=\"score-num\" style=\"color:{}\">{}</span>\n",
                    overall_color,
                    format_score(s.overall)
                ));
                html.push_str("<span class=\"score-of\">out of 10</span>\n</div>\n");
                for cs in &s.scores {
                    let color = score_color(cs.score);
                    let width = ((cs.score.clamp(0.0, 10.0) / 10.0) * 100.0).round();
                    html.push_str("<div class=\"crit\">\n");
                    html.push_str("<div class=\"crit-head\">");
                    html.push_str(&format!("<span class=\"crit-name\">{}</span>", escape_html(&cs_note_name(r, &cs.criterion))));
                    html.push_str(&format!("<span class=\"crit-val\" style=\"color:{}\">{}/10</span>", color, format_score(cs.score)));
                    html.push_str("</div>\n");
                    html.push_str("<div class=\"bar\"><div class=\"bar-fill\" style=");
                    html.push_str(&format!("width:{width:.0}%;background:{color}"));
                    html.push_str("\"</div></div>\n");
                    if !cs.note.trim().is_empty() {
                        html.push_str(&format!("<div class=\"crit-note\">{}</div>\n", escape_html(&cs.note)));
                    }
                    html.push_str("</div>\n");
                }
                if !s.strengths.is_empty() {
                    html.push_str("<h2>Strengths</h2>\n<ul class=\"score-list\">\n");
                    for x in &s.strengths {
                        html.push_str(&format!("<li>{}</li>\n", escape_html(x)));
                    }
                    html.push_str("</ul>\n");
                }
                if !s.improvements.is_empty() {
                    html.push_str("<h2>Improvements</h2>\n<ul class=\"score-list\">\n");
                    for x in &s.improvements {
                        html.push_str(&format!("<li>{}</li>\n", escape_html(x)));
                    }
                    html.push_str("</ul>\n");
                }
            }
            None => {
                html.push_str("<p class=\"empty\"><em>Not scored — no local language model is loaded.</em></p>\n");
                html.push_str("<ul class=\"rubric-list\">\n");
                for c in &r.criteria {
                    html.push_str(&format!(
                        "<li><b>{}</b> — {}</li>\n",
                        escape_html(&c.name),
                        escape_html(&c.description)
                    ));
                }
                html.push_str("</ul>\n");
            }
        }
        html.push_str("</div>\n");
    }

    // Details: headline stat tiles + the remaining meeting facts.
    html.push_str("<div class=\"card\">\n<h2>Details</h2>\n");
    html.push_str("<div class=\"stats\">\n");
    html.push_str(&format!(
        "<div class=\"stat\"><div class=\"stat-val\">{}</div><div class=\"stat-label\">Duration</div></div>\n",
        format_clock(transcript.duration_ms)
    ));
    html.push_str(&format!("<div class=\"stat\"><div class=\"stat-val\">{word_count}</div><div class=\"stat-label\">Words</div></div>\n"));
    html.push_str(&format!("<div class=\"stat\"><div class=\"stat-val\">{wpm}</div><div class=\"stat-label\">WPM</div></div>\n"));
    html.push_str(&format!(
        "<div class=\"stat\"><div class=\"stat-val\">{}</div><div class=\"stat-label\">Segments</div></div>\n",
        transcript.segments.len()
    ));
    html.push_str("</div>\n<dl class=\"kv\">\n");
    html.push_str(&format!("<dt>Started</dt><dd>{}</dd>\n", escape_html(started_at)));
    if !speakers.is_empty() {
        html.push_str(&format!(
            "<dt>Speakers</dt><dd>{} — {}</dd>\n",
            speakers.len(),
            escape_html(&speakers.join(", "))
        ));
    }
    if let Some(lang) = &transcript.language {
        html.push_str(&format!("<dt>Language</dt><dd>{lang}</dd>\n"));
    }
    if let Some(engine) = &transcript.engine {
        html.push_str(&format!("<dt>ASR engine</dt><dd>{engine}</dd>\n", engine = escape_html(engine)));
    }
    html.push_str("</dl>\n</div>\n");

    html.push_str("<div class=\"card\">\n<h2>Transcript</h2>\n");
    if transcript.segments.is_empty() {
        html.push_str("<p class=\"empty\"><em>No speech was captured.</em></p>\n");
    }
    for seg in &transcript.segments {
        html.push_str("<div class=\"seg\">");
        html.push_str(&format!("<span class=\"ts\">{}</span>", format_clock(seg.start_ms)));
        if let Some(who) = &seg.speaker {
            html.push_str(&format!(
                "<span class=\"avatar\" style=\"background:{};color:{}\">{}</span>",
                speaker_tint(who),
                speaker_color(who),
                escape_html(&speaker_initial(who))
            ));
            html.push_str(&format!(
                "<span class=\"who\" style=\"color:{}\">{}</span>",
                speaker_color(who),
                escape_html(who)
            ));
        } else {
            html.push_str("<span class=\"avatar\" style=\"background:#efedeb;color:#57534e\">?</span>");
            html.push_str("<span class=\"who\"></span>");
        }
        html.push_str(&format!("<span class=\"txt\">{}</span>", escape_html(&seg.text)));
        html.push_str("</div>\n");
    }
    html.push_str("</div>\n");

    html.push_str("<footer>");
    html.push_str("<span class=\"brand-dot\"></span>");
    html.push_str("Generated by Teletype");
    if let Some(engine) = &transcript.engine {
        html.push_str(&format!(" · ASR engine: {engine}"));
    }
    html.push_str("</footer>\n");

    html.push_str("</div>\n</body>\n</html>\n");
    html
}

/// Accent color for a 0–10 score: green (good), amber (fair), red (poor).
fn score_color(score: f32) -> &'static str {
    if score >= 7.0 {
        "#0b6e4f"
    } else if score >= 4.0 {
        "#b45309"
    } else {
        "#b3261e"
    }
}

/// Formats a score for display: 8.0 → "8", 7.5 → "7.5".
fn format_score(score: f32) -> String {
    let s = score.clamp(0.0, 10.0);
    if (s - s.round()).abs() < 0.05 {
        format!("{}", s.round() as i64)
    } else {
        format!("{:.1}", (s * 10.0).round() / 10.0)
    }
}

/// Display name for a criterion id; falls back to the raw id when the LLM
/// returns an unknown one (the report must never drop a score silently).
fn cs_note_name(rubric: &Rubric, criterion_id: &str) -> String {
    rubric
        .criteria
        .iter()
        .find(|c| c.id == criterion_id)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| criterion_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::{MeetingSegment, MeetingWord};

    fn transcript() -> MeetingTranscript {
        MeetingTranscript {
            text: "hello world second part".into(),
            words: vec![
                MeetingWord {
                    text: "hello".into(),
                    start_ms: 0,
                    end_ms: 400,
                    speaker: Some("Alice".into()),
                },
                MeetingWord {
                    text: "world".into(),
                    start_ms: 450,
                    end_ms: 900,
                    speaker: Some("Alice".into()),
                },
                MeetingWord {
                    text: "second".into(),
                    start_ms: 2_400,
                    end_ms: 2_900,
                    speaker: Some("Bob".into()),
                },
                MeetingWord {
                    text: "part".into(),
                    start_ms: 2_950,
                    end_ms: 3_300,
                    speaker: Some("Bob".into()),
                },
            ],
            segments: vec![
                MeetingSegment {
                    start_ms: 0,
                    end_ms: 900,
                    text: "hello world".into(),
                    speaker: Some("Alice".into()),
                },
                MeetingSegment {
                    start_ms: 2_400,
                    end_ms: 3_300,
                    text: "second part".into(),
                    speaker: Some("Bob".into()),
                },
            ],
            duration_ms: 3_300,
            confidence: 1.0,
            language: Some("en".into()),
            engine: Some("test-engine".into()),
        }
    }

    #[test]
    fn report_is_self_contained() {
        let html = render_report("Team Sync", "Monday 9:00 AM", &transcript(), None, None, None, false);
        // No external resources of any kind.
        assert!(!html.contains("http://"));
        assert!(!html.contains("https://"));
        assert!(!html.to_lowercase().contains("<script"));
        assert!(!html.to_lowercase().contains("<link"));
        assert!(!html.to_lowercase().contains("src="));
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<meta charset=\"utf-8\">"));
    }

    #[test]
    fn report_escapes_user_content() {
        let mut t = transcript();
        t.segments = vec![MeetingSegment {
            start_ms: 0,
            end_ms: 100,
            text: "<script>alert('x')</script> & \"quotes\"".into(),
            speaker: None,
        }];
        let summary = ReportSummary {
            summary: "<b>bold</b> & friends".into(),
            action_items: vec!["ship <the> thing".into()],
        };
        let html = render_report("<title-injection>", "12:00", &t, Some(&summary), None, None, false);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &amp; &quot;quotes&quot;"));
        assert!(!html.contains("<b>bold</b>"));
        assert!(html.contains("&lt;b&gt;bold&lt;/b&gt; &amp; friends"));
        assert!(!html.contains("<title-injection>"));
        assert!(html.contains("&lt;title-injection&gt;"));
    }

    #[test]
    fn report_shows_speakers_with_stable_colors() {
        let html = render_report("Sync", "9:00", &transcript(), None, None, None, false);
        assert!(html.contains("Alice"));
        assert!(html.contains("Bob"));
        // Both speakers colored; Alice first in the palette.
        assert!(html.contains("color:#0b6e4f\">Alice"));
        assert!(html.contains("color:#8a3ffc\">Bob"));
    }

    #[test]
    fn report_includes_summary_and_actions() {
        let summary = ReportSummary {
            summary: "Discussed the launch plan.".into(),
            action_items: vec!["Ship by Friday".into()],
        };
        let html = render_report("Sync", "9:00", &transcript(), Some(&summary), None, None, false);
        assert!(html.contains("Overview"));
        assert!(html.contains("Discussed the launch plan."));
        assert!(html.contains("Ship by Friday"));
        assert!(html.contains("Action items"));
    }

    #[test]
    fn report_without_summary_falls_back_to_mechanical_overview() {
        let html = render_report("Sync", "9:00", &transcript(), None, None, None, false);
        assert!(html.contains("Overview"));
        // The mechanical line carries the meeting's facts.
        assert!(html.contains("4 words spoken across 2 segment(s)"));
        assert!(!html.contains("Action items"));
    }

    #[test]
    fn report_details_section_lists_meeting_facts() {
        let html = render_report("Sync", "9:00", &transcript(), None, None, None, false);
        assert!(html.contains("Details"));
        // Headline numbers render as stat tiles.
        assert!(html.contains("stat-val\">0:03<")); // duration 3_300 ms
        assert!(html.contains("stat-val\">4<")); // words
        assert!(html.contains("stat-val\">2<")); // segments
        // The remaining facts stay in the key/value list.
        assert!(html.contains("<dt>Started</dt><dd>9:00</dd>"));
        assert!(html.contains("<dt>Speakers</dt><dd>2 — Alice, Bob</dd>"));
        assert!(html.contains("<dt>Language</dt><dd>en</dd>"));
        assert!(html.contains("<dt>ASR engine</dt><dd>test-engine</dd>"));
    }

    #[test]
    fn report_handles_empty_transcript() {
        let t = MeetingTranscript::default();
        let html = render_report("Empty", "9:00", &t, None, None, None, false);
        assert!(html.contains("No speech was captured."));
    }

    #[test]
    fn report_shows_clock_and_stats() {
        let mut t = transcript();
        t.duration_ms = 3_723_000;
        let html = render_report("Long", "9:00", &t, None, None, None, false);
        assert!(html.contains("1:02:03 duration"));
        assert!(html.contains("4 words"));
        assert!(html.contains("ASR engine: test-engine"));
    }

    #[test]
    fn report_transcript_rows_carry_speaker_avatars() {
        let html = render_report("Sync", "9:00", &transcript(), None, None, None, false);
        // Each speaker gets a colored avatar with an initial (A / B).
        assert!(html.contains("<span class=\"avatar\" style=\"background:#e6f2ec;color:#0b6e4f\">A</span>"));
        assert!(html.contains("<span class=\"avatar\" style=\"background:#f1e9fd;color:#8a3ffc\">B</span>"));
    }

    #[test]
    fn report_live_auto_refreshes_without_js() {
        let html = render_report("Live", "9:00", &transcript(), None, None, None, true);
        // The browser tab re-reads the file every 3 s while the meeting runs.
        assert!(html.contains("<meta http-equiv=\"refresh\" content=\"3\">"));
        assert!(html.contains("Live — updates as the meeting continues"));
        // Still no JavaScript — the refresh is a meta tag.
        assert!(!html.to_lowercase().contains("<script"));
        // The final report must not auto-refresh or carry the badge.
        let final_html = render_report("Final", "9:00", &transcript(), None, None, None, false);
        assert!(!final_html.contains("http-equiv"));
        assert!(!final_html.contains("<div class=\"live-badge\">"));
    }

    #[test]
    fn report_score_section_renders_criteria_and_overall() {
        let rubric = crate::rubric::find_rubric("sales").unwrap();
        let score = crate::rubric::RubricScore {
            overall: 7.5,
            scores: vec![
                crate::rubric::CriterionScore {
                    criterion: "needs-discovery".into(),
                    score: 8.0,
                    note: "Asked three probing questions before pitching.".into(),
                },
                crate::rubric::CriterionScore {
                    criterion: "next-steps".into(),
                    score: 4.0,
                    note: "Ended without a concrete next step.".into(),
                },
            ],
            strengths: vec!["Strong rapport".into()],
            improvements: vec!["Close was vague".into()],
        };
        let html = render_report("Call", "9:00", &transcript(), None, Some(&rubric), Some(&score), false);
        assert!(html.contains("Score — Sales call"));
        assert!(html.contains("7.5")); // overall
        assert!(html.contains("out of 10"));
        // Criterion names resolve from ids; bars carry the score width.
        assert!(html.contains("Needs discovery"));
        assert!(html.contains("width:80%"));
        assert!(html.contains("width:40%"));
        assert!(html.contains("Strengths"));
        assert!(html.contains("Improvements"));
    }

    #[test]
    fn report_unscored_rubric_lists_criteria() {
        let rubric = crate::rubric::find_rubric("support").unwrap();
        let html = render_report("Call", "9:00", &transcript(), None, Some(&rubric), None, false);
        assert!(html.contains("Score — Customer support"));
        assert!(html.contains("Not scored — no local language model is loaded."));
        // The criterion name is HTML-escaped (& → &amp;).
        assert!(html.contains("Greeting &amp; empathy"));
        // No score bars without a score (the CSS defines .bar-fill; the
        // element must not appear).
        assert!(!html.contains("<div class=\"bar\">"));
    }

    #[test]
    fn report_without_rubric_has_no_score_section() {
        let html = render_report("Sync", "9:00", &transcript(), None, None, None, false);
        assert!(!html.contains("Score —"));
    }
}
