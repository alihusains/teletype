//! Renders a realistic sample meeting report to a file, for design review.
//!
//! Usage:
//!   cargo run -p teletype-core --example render_sample_report [output.html]
//!
//! Defaults to `sample-meeting-report.html` in the current directory.

use teletype_core::meeting_report::{render_report, ReportSummary};
use teletype_core::meeting::{MeetingSegment, MeetingTranscript, MeetingWord};

fn seg(start_ms: u64, end_ms: u64, text: &str, speaker: &str) -> (MeetingSegment, Vec<MeetingWord>) {
    // Split the text into words and spread them evenly across the segment —
    // good enough for a visual sample; the renderer only needs plausible
    // timings and speaker labels.
    let words = text.split_whitespace().collect::<Vec<_>>();
    let per = if words.is_empty() {
        0
    } else {
        (end_ms - start_ms) / words.len() as u64
    };
    let ws = words
        .iter()
        .enumerate()
        .map(|(i, w)| MeetingWord {
            text: (*w).to_string(),
            start_ms: start_ms + i as u64 * per,
            end_ms: start_ms + (i as u64 + 1) * per,
            speaker: Some(speaker.to_string()),
        })
        .collect();
    (
        MeetingSegment {
            start_ms,
            end_ms,
            text: text.to_string(),
            speaker: Some(speaker.to_string()),
        },
        ws,
    )
}

fn main() {
    let mut segments = Vec::new();
    let mut words = Vec::new();
    let rows: [(&str, u64, u64, &str); 10] = [
        ("Good morning everyone, thanks for jumping in. Let's start with the launch timeline.", 0, 4200, "Alice"),
        ("Sure. Marketing is ready for the fourteenth, but we're still waiting on the pricing page copy.", 4800, 9600, "Bob"),
        ("I can have a first draft of the pricing copy by tomorrow afternoon. Can we review it in the daily?", 10200, 14800, "Alice"),
        ("Works for me. One more thing — support asked for a migration guide for the old API.", 15400, 19600, "Bob"),
        ("I'll assign that to the docs team today and we'll link it from the release notes.", 20200, 24300, "Alice"),
        ("On the engineering side, the beta build is stable. We found two small issues in the export flow.", 25000, 29800, "Carol"),
        ("Both are fixed on main, right? I saw the commits this morning.", 30400, 33400, "Bob"),
        ("Yes, both merged. We'll cut the release candidate on Wednesday and ship on the fourteenth.", 34000, 38600, "Carol"),
        ("Great. I'll update the launch checklist and post it in the channel.", 39200, 42400, "Alice"),
        ("Thanks all — same time next week to do the final go-no-go.", 43000, 46200, "Bob"),
    ];
    for (text, start, end, speaker) in rows {
        let (s, w) = seg(start, end, text, speaker);
        segments.push(s);
        words.extend(w);
    }
    let transcript = MeetingTranscript {
        text: rows
            .iter()
            .map(|r| r.0)
            .collect::<Vec<_>>()
            .join(" "),
        words,
        segments,
        duration_ms: 46_200,
        confidence: 1.0,
        language: Some("en".into()),
        engine: Some("sherpa-parakeet".into()),
    };
    let summary = ReportSummary {
        summary: "Weekly launch sync. The team confirmed the fourteenth as the launch date: marketing is ready, pricing page copy is due tomorrow, a migration guide for the old API is being assigned to the docs team, and the beta build is stable with both export-flow issues fixed on main. The release candidate goes out Wednesday."
            .into(),
        action_items: vec![
            "Alice to draft pricing page copy (due tomorrow afternoon)".into(),
            "Alice to assign migration guide for the old API to the docs team".into(),
            "Carol to cut the release candidate on Wednesday".into(),
            "Alice to update the launch checklist and post it in the channel".into(),
        ],
    };
    let rubric = teletype_core::rubric::find_rubric("general").unwrap_or_else(|| {
        teletype_core::rubric::builtin_rubrics().into_iter().next().unwrap()
    });
    let score = teletype_core::rubric::RubricScore {
        overall: 7.8,
        scores: vec![
            teletype_core::rubric::CriterionScore {
                criterion: "purpose".into(),
                score: 9.0,
                note: "Opened with a clear goal — the launch date — and stayed on topic.".into(),
            },
            teletype_core::rubric::CriterionScore {
                criterion: "decisions".into(),
                score: 8.0,
                note: "Launch date and RC timing were confirmed explicitly.".into(),
            },
            teletype_core::rubric::CriterionScore {
                criterion: "actions".into(),
                score: 8.5,
                note: "Every open item got an owner and a due time.".into(),
            },
            teletype_core::rubric::CriterionScore {
                criterion: "time".into(),
                score: 9.0,
                note: "Covered the agenda in under 45 seconds of audio and wrapped on schedule.".into(),
            },
            teletype_core::rubric::CriterionScore {
                criterion: "engagement".into(),
                score: 7.0,
                note: "All three speakers contributed; Carol answered the build questions directly.".into(),
            },
        ],
        strengths: vec![
            "Decisions were explicit and time-boxed, not deferred.".into(),
            "Action items named owners and deadlines before the call ended.".into(),
        ],
        improvements: vec![
            "The pricing-copy dependency surfaced late — raise cross-team blockers in the first two minutes.".into(),
        ],
    };
    let html = render_report(
        "Product Launch Sync",
        "Monday 9:00 AM",
        &transcript,
        Some(&summary),
        Some(&rubric),
        Some(&score),
        false,
    );
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "sample-meeting-report.html".into());
    std::fs::write(&out, html).expect("write sample report");
    println!("wrote {out} ({} bytes)", std::fs::metadata(&out).unwrap().len());
}
