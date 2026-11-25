//! Meeting scoring rubrics.
//!
//! A rubric is a named set of criteria a meeting can be scored against —
//! e.g. a **sales call** is judged on discovery and objection handling, a
//! **performance review** on specific feedback and a development plan.
//! The user picks one (or none) at the start of a meeting; when a local
//! language model is available, finalization scores the transcript
//! against the rubric and the score appears in the report and the app.
//!
//! The core owns the rubric *definitions* and the *score shape* only —
//! scoring itself (the LLM call) lives in the desktop engine, and the
//! report renderer just draws whatever score it is given.

use serde::{Deserialize, Serialize};

/// One scorable criterion of a rubric.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RubricCriterion {
    /// Stable slug (e.g. `"needs-discovery"`); the LLM's score references
    /// it, so it must not change under a shipped rubric.
    pub id: String,
    /// Human name ("Needs discovery").
    pub name: String,
    /// What "good" looks like — included in the scoring prompt.
    pub description: String,
}

/// A scoring rubric: the meeting type plus its criteria.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rubric {
    /// Stable id (e.g. `"sales"`); the UI select and `meeting_start` use it.
    pub id: String,
    /// Display name ("Sales call").
    pub name: String,
    /// One-line description of when to use this rubric.
    pub description: String,
    pub criteria: Vec<RubricCriterion>,
}

/// The LLM's score for one criterion (0–10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriterionScore {
    /// The criterion's `id`.
    pub criterion: String,
    /// 0 (not demonstrated) to 10 (exemplary).
    pub score: f32,
    /// One or two sentences citing what was (not) observed.
    pub note: String,
}

/// The full score of a meeting against a rubric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RubricScore {
    /// Overall 0–10 assessment (not required to be the mean of the
    /// criteria — the LLM weighs them).
    pub overall: f32,
    /// One score per criterion, in rubric order.
    pub scores: Vec<CriterionScore>,
    /// What went well, with specifics.
    pub strengths: Vec<String>,
    /// What to improve, with specifics.
    pub improvements: Vec<String>,
}

/// The built-in rubrics. Order is the UI's display order; `"general"`
/// first because it is the default.
pub fn builtin_rubrics() -> Vec<Rubric> {
    vec![
        Rubric {
            id: "general".into(),
            name: "General meeting".into(),
            description: "Any meeting: purpose, decisions, and follow-through.".into(),
            criteria: vec![
                RubricCriterion {
                    id: "purpose".into(),
                    name: "Clear purpose & agenda".into(),
                    description:
                        "The meeting opens with a clear goal or agenda and stays on topic.".into(),
                },
                RubricCriterion {
                    id: "decisions".into(),
                    name: "Decisions made".into(),
                    description:
                        "Open questions are resolved or explicitly parked with an owner.".into(),
                },
                RubricCriterion {
                    id: "actions".into(),
                    name: "Action items".into(),
                    description:
                        "Concrete next steps with owners (and dates where relevant) are assigned before wrapping up.".into(),
                },
                RubricCriterion {
                    id: "time".into(),
                    name: "Time management".into(),
                    description:
                        "The meeting stays focused, covers what it set out to cover, and ends on time.".into(),
                },
                RubricCriterion {
                    id: "engagement".into(),
                    name: "Engagement & participation".into(),
                    description:
                        "Participants are heard and build on each other; no one dominates or checks out.".into(),
                },
            ],
        },
        Rubric {
            id: "sales".into(),
            name: "Sales call".into(),
            description: "Outbound/inbound sales: discovery, value, and next steps.".into(),
            criteria: vec![
                RubricCriterion {
                    id: "rapport".into(),
                    name: "Rapport & opening".into(),
                    description:
                        "Professional greeting, brief rapport, and a clear reason for the call.".into(),
                },
                RubricCriterion {
                    id: "needs-discovery".into(),
                    name: "Needs discovery".into(),
                    description:
                        "Asks probing questions about the customer's situation, goals, and pain before pitching.".into(),
                },
                RubricCriterion {
                    id: "value".into(),
                    name: "Value presentation".into(),
                    description:
                        "Ties product capabilities to the customer's stated needs, with specifics rather than features.".into(),
                },
                RubricCriterion {
                    id: "objections".into(),
                    name: "Objection handling".into(),
                    description:
                        "Acknowledges and addresses concerns (price, timing, alternatives) without arguing.".into(),
                },
                RubricCriterion {
                    id: "next-steps".into(),
                    name: "Next steps & close".into(),
                    description:
                        "Ends with a concrete, agreed next step (demo, proposal, date) rather than a vague follow-up.".into(),
                },
            ],
        },
        Rubric {
            id: "support".into(),
            name: "Customer support".into(),
            description: "Support ticket call: empathy, diagnosis, and resolution.".into(),
            criteria: vec![
                RubricCriterion {
                    id: "empathy".into(),
                    name: "Greeting & empathy".into(),
                    description:
                        "Warm opening; acknowledges the customer's frustration or inconvenience early.".into(),
                },
                RubricCriterion {
                    id: "understanding".into(),
                    name: "Problem understanding".into(),
                    description:
                        "Restates the issue accurately and asks targeted questions to isolate the cause.".into(),
                },
                RubricCriterion {
                    id: "resolution".into(),
                    name: "Resolution quality".into(),
                    description:
                        "Reaches a real fix or a clear escalation path with the customer's consent.".into(),
                },
                RubricCriterion {
                    id: "communication".into(),
                    name: "Communication clarity".into(),
                    description:
                        "Explains steps in plain language, avoids jargon, and checks understanding.".into(),
                },
                RubricCriterion {
                    id: "wrap-up".into(),
                    name: "Follow-up & wrap-up".into(),
                    description:
                        "Summarizes what was done, sets expectations for anything pending, and closes politely.".into(),
                },
            ],
        },
        Rubric {
            id: "performance".into(),
            name: "Performance review".into(),
            description: "1:1 or review: feedback, growth, and a development plan.".into(),
            criteria: vec![
                RubricCriterion {
                    id: "results".into(),
                    name: "Results & goals review".into(),
                    description:
                        "Reviews concrete outcomes against goals, with evidence rather than generalities.".into(),
                },
                RubricCriterion {
                    id: "feedback".into(),
                    name: "Specific feedback".into(),
                    description:
                        "Feedback cites specific behaviors and examples, balanced across strengths and gaps.".into(),
                },
                RubricCriterion {
                    id: "growth".into(),
                    name: "Growth areas".into(),
                    description:
                        "Names 1–3 development areas clearly, without dumping everything at once.".into(),
                },
                RubricCriterion {
                    id: "plan".into(),
                    name: "Development plan".into(),
                    description:
                        "Agrees on concrete actions, resources, and a review date for the growth areas.".into(),
                },
                RubricCriterion {
                    id: "dialogue".into(),
                    name: "Two-way dialogue".into(),
                    description:
                        "The person being reviewed speaks a meaningful share and their view is heard.".into(),
                },
            ],
        },
    ]
}

/// Finds a built-in rubric by id (e.g. `"sales"`).
pub fn find_rubric(id: &str) -> Option<Rubric> {
    builtin_rubrics()
        .into_iter()
        .find(|r| r.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_rubrics_are_unique_and_nonempty() {
        let rubrics = builtin_rubrics();
        assert!(rubrics.len() >= 4);
        let mut ids: Vec<&str> = rubrics.iter().map(|r| r.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), rubrics.len(), "rubric ids must be unique");
        for r in &rubrics {
            assert!(!r.criteria.is_empty());
            let mut cids: Vec<&str> = r.criteria.iter().map(|c| c.id.as_str()).collect();
            cids.sort_unstable();
            cids.dedup();
            assert_eq!(cids.len(), r.criteria.len(), "criterion ids must be unique");
            for c in &r.criteria {
                assert!(!c.name.trim().is_empty());
                assert!(!c.description.trim().is_empty());
            }
        }
    }

    #[test]
    fn find_rubric_resolves_known_ids() {
        assert!(find_rubric("sales").is_some());
        assert!(find_rubric("support").is_some());
        assert!(find_rubric("performance").is_some());
        assert!(find_rubric("general").is_some());
        assert!(find_rubric("nope").is_none());
        assert!(find_rubric("").is_none());
    }

    #[test]
    fn rubric_score_round_trips() {
        let score = RubricScore {
            overall: 7.5,
            scores: vec![CriterionScore {
                criterion: "needs-discovery".into(),
                score: 8.0,
                note: "Asked three probing questions.".into(),
            }],
            strengths: vec!["Strong opening".into()],
            improvements: vec!["Close was vague".into()],
        };
        let json = serde_json::to_string(&score).unwrap();
        let back: RubricScore = serde_json::from_str(&json).unwrap();
        assert_eq!(score, back);
    }
}
