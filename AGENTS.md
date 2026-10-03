# AGENTS.md

## Purpose

This repository uses a project brain so coding agents can understand the project, preserve useful discoveries, and improve future work.

The project brain is repository-local and tool-agnostic. It must work with multiple coding agents and must remain useful if the project already existed before this file was added.

## Sources of truth

Use this precedence when information conflicts:

1. Direct instructions from the user in the current task.
2. Verified project decisions in `.agents/brain/PROJECT_BRAIN.md`.
3. Verified project learnings in `.agents/brain/learnings/`.
4. Project documentation and source code.
5. Optional company brain, when explicitly configured.
6. Agent assumptions and general knowledge.

Never silently replace a verified project decision with an assumption.

## First-session behavior

Before making meaningful changes:

1. Read `.agents/brain/PROJECT_BRAIN.md` if it exists.
2. Inspect the repository to understand the current implementation.
3. Read relevant existing documentation, configuration, and local instruction files.
4. Identify relevant prior learnings before debugging or designing something similar.
5. Do not assume this is a greenfield project. Preserve existing behavior unless the user asks for a change.

If the project brain is missing, create it using the structure in this repository.

## Working with the user

Ask the user for context when the repository cannot answer an important question.

Good reasons to ask:
- business intent is unclear
- an architectural decision cannot be inferred safely
- an external system or source is referenced but unavailable
- there are multiple reasonable interpretations with materially different outcomes
- a required constraint is missing

Do not ask questions that can be answered by inspecting the repository.

When asking, prefer a small number of targeted questions. State what is known, what is unknown, and what decision the answer will affect.

When the user points to a source, use that source when accessible:
- repository
- documentation
- URL
- local file
- API specification
- screenshot or design
- ticket or issue

Record important external sources in the project brain with the source location and what was learned from it.

Do not claim to have inspected a source that was not actually accessible.

## Project brain maintenance

Keep `.agents/brain/PROJECT_BRAIN.md` concise.

Update the brain when you learn something that is:
- important to understanding the project
- likely to matter in future tasks
- a durable architectural decision
- a recurring integration or operational constraint
- a verified debugging lesson
- a non-obvious convention

Do not record:
- transient task progress
- obvious information already clear from the code
- speculative conclusions
- duplicate knowledge
- credentials, secrets, tokens, private keys, or sensitive personal data

Prefer updating an existing section over creating duplicate knowledge.

## Learnings

Reusable discoveries belong in `.agents/brain/learnings/`.

A learning should capture the reasoning, not only the final answer.

For debugging learnings, record:

- Problem
- Symptoms
- Initial hypotheses
- Investigation steps
- Evidence
- Root cause
- Fix
- Verification
- Reusable lesson

Use this lifecycle:

`candidate -> verified -> deprecated/rejected`

A candidate learning is not authoritative until supported by evidence or confirmed by a human/agent review.

Every useful learning should include:
- unique ID
- title
- date
- agent/tool when known
- project area
- status
- evidence or source

Do not create a learning for every small mistake. Capture durable lessons.

## After completing a task

Before finishing a meaningful task, perform a lightweight knowledge review:

1. Did I discover something future agents need to know?
2. Did I debug something non-obvious?
3. Did I make or confirm an architectural decision?
4. Did I learn an external-system constraint?
5. Did an existing brain entry become outdated?

If yes, update the appropriate brain section or create a learning candidate.

If no, do not create artificial documentation.

## Debugging protocol

When debugging a non-trivial issue:

1. Reproduce or establish the symptom.
2. Form explicit hypotheses.
3. Test the cheapest/highest-signal hypothesis first.
4. Collect evidence before declaring a root cause.
5. Make the smallest appropriate fix.
6. Verify the fix.
7. Capture the investigation as a learning when it is reusable.

Do not rewrite large parts of the system just because the root cause is not immediately obvious.

## Decisions

Durable architecture and product decisions belong in `PROJECT_BRAIN.md` under Decisions.

Record:
- decision
- context
- alternatives considered when useful
- reason
- date
- status

If a new task conflicts with a recorded decision, surface the conflict to the user rather than silently changing the decision.

## Multi-agent behavior

Agents may specialize, but they share the same project brain.

Before delegating:
- give the sub-agent the task context
- point it to the relevant brain sections
- tell it to record reusable discoveries
- tell it not to invent missing project facts

After delegation:
- review important findings
- promote only validated discoveries
- avoid duplicated or contradictory knowledge

A sub-agent's hypothesis is not automatically project truth.

## Optional company brain

This project is designed to accept a future company brain without changing its local format.

When a company brain is configured, treat it as shared guidance and reusable knowledge, not as an automatic replacement for project-specific decisions.

Suggested future configuration location:

`.agents/brain/company/`

Possible future contents:

- company standards
- shared engineering practices
- organization-wide skills
- approved architecture patterns
- reusable verified learnings

Project-specific facts remain in this repository.

If company and project guidance conflict, surface the conflict. Do not silently overwrite project decisions.

## Safety and privacy

Never write secrets or sensitive credentials into the project brain.

Do not copy:
- API keys
- access tokens
- passwords
- private keys
- authentication cookies
- personal data that is unnecessary for the learning

Reference secure systems by name or location instead.

## Minimal final response

At task completion, briefly report:
- what changed
- what was verified
- whether project knowledge was updated
- any important unresolved uncertainty

## Agent team

Project-scoped agent definitions live in `.claude/agents/`. They form the Teletype build team. The lightweight index, ownership map, and lifecycle states live in `.agents/agents/registry.md`; add/change/retire a specialist there and in `.agents/agents/changelog.md`, not just in this table.

| Agent | Role |
|---|---|
| `teletype-product-manager` | Roadmap, issue triage, README/release accuracy, feature-by-feature shipping |
| `teletype-engineering-manager` | Decomposes roadmap items, dispatches the team, enforces verification before "done" |
| `teletype-architect` | Cross-crate design, security and stability review, durable decisions in the brain |
| `teletype-backend-engineer` | Rust: `crates/*` and `evals/` |
| `teletype-frontend-engineer` | React/TypeScript: `ui/` and the Tauri IPC JS surface |
| `teletype-qa-engineer` | Tests, eval harness, regression verification, release gate |
| `teletype-deployment-engineer` | CI/release pipelines, signing, notarization, updater manifest, "why did the release not ship" |
| `teletype-productivity-monitor` | Observes the work, writes learnings/skills/tasks, keeps the brain honest |

Routing: hand a multi-part build task to `teletype-engineering-manager`; a single-crate fix to the matching engineer; a "is this really done?" question to `teletype-qa-engineer`; a CI failure, release build failure, signing/notarization, or updater-manifest problem to `teletype-deployment-engineer`; a finished piece of work to `teletype-productivity-monitor` for the knowledge review. All agents share this brain and its precedence rules.

## Project skills (.agents/skills/)

Reusable `/ali-*` skills for this repo (read the SKILL.md when the trigger fires):

| Skill | Trigger | Purpose |
|---|---|---|
| `ali-pi-team` | `/ali-pi-team` | The build team: decompose a task, delegate MECH units to pi (background), do QUICK units inline, verify before done. |
| `ali-verify-pi-work` | `/ali-verify-pi-work` | Verify a pi "done" claim: re-run its commands, read the real diff, check for crash artifacts. Never trust a pi report alone. |
| `ali-app-dev-loop` | `/ali-app-dev-loop` | Start/restart the Tauri dev app correctly (vite 1420 + binary + llama-server re-sign L006) and confirm the window serves. |
| `ali-agent-health` | `/ali-agent-health` | Validate the agent system + brain for drift: registry/reality mismatch, dead brain references, stale decisions, orphaned or overlapping specialists. A report, not a fixer. |
| `ali-evolve-agents` | `/ali-evolve-agents` | After meaningful work, decide whether the brain, a specialist, or the team needs an evidence-based update. Promote a learning to a gotcha/rule when it recurs; add a specialist only for a genuinely new recurring domain. |

**Convention:** when Ali gives a build task, default to `/ali-pi-team`. After any pi task, run `/ali-verify-pi-work` before reporting done. When the app needs a live check or comes up blank, use `/ali-app-dev-loop`. When the team or brain feels stale, run `/ali-agent-health`; after a big or novel task, run `/ali-evolve-agents`.

## Decisions (ADR)

Durable architecture and product decisions live in `PROJECT_BRAIN.md` under
Decisions (D001–D009). Decisions that arrive with a code change and deserve a
standalone record live in `.agents/brain/adr/` as `ADR-NNN-short-name.md`
(decision, context, alternatives, consequences, evidence, status, date). Do not
duplicate a decision in both places: the brain keeps the one-line pointer, the
ADR keeps the full reasoning.
