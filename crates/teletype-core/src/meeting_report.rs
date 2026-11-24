//! Self-contained HTML report for a finished meeting.
//!
//! The report is a single file with inline CSS and no JavaScript: no
//! external requests, no fonts, no build step. It can be opened from disk,
//! emailed, or archived next to the meeting audio.

use serde::{Deserialize, Serialize};

use crate::meeting::{format_clock, MeetingTranscript};

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
/// Layout: header → **Overview** (the LLM summary when available, else a
/// mechanical "what happened" line) → **Action items** → **Details**
/// (start / duration / words / WPM / segments / speakers / language /
/// engine) → **Transcript** → footer.
pub fn render_report(
    title: &str,
    started_at: &str,
    transcript: &MeetingTranscript,
    summary: Option<&ReportSummary>,
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
    html.push_str(&format!("<title>{}</title>\n", escape_html(title)));
    html.push_str("<style>\n");
    html.push_str(
        "body{font-family:-apple-system,BlinkMacSystemFont,\"Segoe UI\",Roboto,Helvetica,Arial,sans-serif;\
         margin:0;background:#f6f7f9;color:#1a1d21;line-height:1.55}\
         .wrap{max-width:860px;margin:0 auto;padding:32px 24px 64px}\
         header{border-bottom:2px solid #e3e6ea;padding-bottom:16px;margin-bottom:24px}\
         h1{font-size:26px;margin:0 0 6px;font-weight:650}\
         .meta{color:#5b6570;font-size:13px}\
         .meta span{margin-right:16px}\
         .card{background:#fff;border:1px solid #e3e6ea;border-radius:10px;padding:18px 20px;margin-bottom:18px}\
         .card h2{font-size:14px;text-transform:uppercase;letter-spacing:.06em;color:#5b6570;margin:0 0 10px;font-weight:600}\
         .card h2 + h2{margin-top:16px}\
         .summary p{margin:0 0 12px}\
         .actions{margin:0;padding-left:20px}\
         .actions li{margin-bottom:6px}\
         .kv{display:grid;grid-template-columns:150px 1fr;row-gap:6px;column-gap:12px;font-size:14px;margin:0}\
         .kv dt{color:#5b6570}\
         .kv dd{margin:0;font-variant-numeric:tabular-nums}\
         .seg{display:flex;gap:12px;padding:7px 0;border-bottom:1px solid #eef0f3}\
         .seg:last-child{border-bottom:none}\
         .ts{flex:0 0 52px;color:#8a94a0;font-variant-numeric:tabular-nums;font-size:13px;padding-top:1px}\
         .who{flex:0 0 90px;font-weight:600;font-size:14px}\
         .txt{flex:1;white-space:pre-wrap;word-break:break-word}\
         footer{color:#8a94a0;font-size:12px;margin-top:28px}\
         @media print{body{background:#fff}.card{border:none;padding:8px 0}}",
    );
    html.push_str("</style>\n</head>\n<body>\n<div class=\"wrap\">\n");

    html.push_str("<header>\n");
    html.push_str(&format!("<h1>{}</h1>\n", escape_html(title)));
    html.push_str("<div class=\"meta\">");
    html.push_str(&format!("<span>{}</span>", escape_html(started_at)));
    html.push_str(&format!(
        "<span>{} duration</span>",
        format_clock(transcript.duration_ms)
    ));
    html.push_str(&format!("<span>{word_count} words</span>"));
    if wpm > 0 {
        html.push_str(&format!("<span>{wpm} wpm</span>"));
    }
    if let Some(lang) = &transcript.language {
        html.push_str(&format!("<span>{lang}</span>"));
    }
    html.push_str("</div>\n</header>\n");

    // Overview: the LLM summary (what the meeting was about) when a
    // provider ran; otherwise a mechanical line so the section is never
    // empty.
    html.push_str("<div class=\"card summary\">\n<h2>Overview</h2>\n");
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

    // Details: the meeting's measurable facts.
    html.push_str("<div class=\"card\">\n<h2>Details</h2>\n<dl class=\"kv\">\n");
    html.push_str(&format!("<dt>Started</dt><dd>{}</dd>\n", escape_html(started_at)));
    html.push_str(&format!(
        "<dt>Duration</dt><dd>{}</dd>\n",
        format_clock(transcript.duration_ms)
    ));
    html.push_str(&format!("<dt>Words</dt><dd>{word_count}</dd>\n"));
    html.push_str(&format!("<dt>Words per minute</dt><dd>{wpm}</dd>\n"));
    html.push_str(&format!(
        "<dt>Transcript segments</dt><dd>{}</dd>\n",
        transcript.segments.len()
    ));
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
        html.push_str("<p><em>No speech was captured.</em></p>\n");
    }
    for seg in &transcript.segments {
        html.push_str("<div class=\"seg\">");
        html.push_str(&format!("<span class=\"ts\">{}</span>", format_clock(seg.start_ms)));
        if let Some(who) = &seg.speaker {
            html.push_str(&format!(
                "<span class=\"who\" style=\"color:{}\">{}</span>",
                speaker_color(who),
                escape_html(who)
            ));
        } else {
            html.push_str("<span class=\"who\"></span>");
        }
        html.push_str(&format!("<span class=\"txt\">{}</span>", escape_html(&seg.text)));
        html.push_str("</div>\n");
    }
    html.push_str("</div>\n");

    html.push_str("<footer>");
    html.push_str("Generated by Teletype");
    if let Some(engine) = &transcript.engine {
        html.push_str(&format!(" · ASR engine: {engine}"));
    }
    html.push_str("</footer>\n");

    html.push_str("</div>\n</body>\n</html>\n");
    html
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
        let html = render_report("Team Sync", "Monday 9:00 AM", &transcript(), None);
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
        let html = render_report("<title-injection>", "12:00", &t, Some(&summary));
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &amp; &quot;quotes&quot;"));
        assert!(!html.contains("<b>bold</b>"));
        assert!(html.contains("&lt;b&gt;bold&lt;/b&gt; &amp; friends"));
        assert!(!html.contains("<title-injection>"));
        assert!(html.contains("&lt;title-injection&gt;"));
    }

    #[test]
    fn report_shows_speakers_with_stable_colors() {
        let html = render_report("Sync", "9:00", &transcript(), None);
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
        let html = render_report("Sync", "9:00", &transcript(), Some(&summary));
        assert!(html.contains("Overview"));
        assert!(html.contains("Discussed the launch plan."));
        assert!(html.contains("Ship by Friday"));
        assert!(html.contains("Action items"));
    }

    #[test]
    fn report_without_summary_falls_back_to_mechanical_overview() {
        let html = render_report("Sync", "9:00", &transcript(), None);
        assert!(html.contains("Overview"));
        // The mechanical line carries the meeting's facts.
        assert!(html.contains("4 words spoken across 2 segment(s)"));
        assert!(!html.contains("Action items"));
    }

    #[test]
    fn report_details_section_lists_meeting_facts() {
        let html = render_report("Sync", "9:00", &transcript(), None);
        assert!(html.contains("Details"));
        assert!(html.contains("<dt>Started</dt><dd>9:00</dd>"));
        assert!(html.contains("<dt>Duration</dt><dd>0:03</dd>"));
        assert!(html.contains("<dt>Words</dt><dd>4</dd>"));
        assert!(html.contains("<dt>Transcript segments</dt><dd>2</dd>"));
        assert!(html.contains("<dt>Speakers</dt><dd>2 — Alice, Bob</dd>"));
        assert!(html.contains("<dt>Language</dt><dd>en</dd>"));
        assert!(html.contains("<dt>ASR engine</dt><dd>test-engine</dd>"));
    }

    #[test]
    fn report_handles_empty_transcript() {
        let t = MeetingTranscript::default();
        let html = render_report("Empty", "9:00", &t, None);
        assert!(html.contains("No speech was captured."));
    }

    #[test]
    fn report_shows_clock_and_stats() {
        let mut t = transcript();
        t.duration_ms = 3_723_000;
        let html = render_report("Long", "9:00", &t, None);
        assert!(html.contains("1:02:03 duration"));
        assert!(html.contains("4 words"));
        assert!(html.contains("ASR engine: test-engine"));
    }
}
