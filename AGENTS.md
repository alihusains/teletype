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

**Freshness is mandatory.** Before and after meaningful work run
`bash scripts/check-brain-freshness.sh` (read-only). Update the `brain-freshness`
marker and the status/topology sections when the release line, HEAD, or verified
facts change. A `post-commit` hook reminds you when code changed but the brain
did not; it is advisory, so the final report must explicitly say either what was
updated in the brain or "no brain change needed". See §14 of the brain.

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
- tell it, in the task file itself, never to run `git checkout`, `git restore`,
  `git reset`, `git stash`, `git clean`, or `git commit`. A delegated worker ran
  `git checkout --` on five UI files on 2026-10-25 and destroyed uncommitted
  work that had never been committed; git could not recover it.
- give the worker an explicit file-ownership list, and keep parallel workers on
  disjoint files

After delegation:
- review important findings
- promote only validated discoveries
- avoid duplicated or contradictory knowledge
- verify the worker's claim by re-running its commands and reading the diff;
  a settled worker is not proof of work

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

The table below is the routing index. Each role has a canonical definition in
`.agents/agents/<role>.md` (the single source of truth), with generated adapters
for OpenCode (`.opencode/agents/`), Pi (`.pi/prompts/`), and Agent Skills
harnesses (`.agents/skills/<role>/SKILL.md`). See `.agents/agents/REGISTRY.md`
for invocation and run `python3 scripts/generate-project-agents.py --check` to
detect drift. Edit the canonical file and regenerate; change this table when a
specialist is added, retired, or gains a new domain.

| Agent | Role |
|---|---|
| `teletype-product-manager` | Roadmap, issue triage, README/release accuracy, feature-by-feature shipping |
| `teletype-engineering-manager` | Decomposes roadmap items, dispatches the team, enforces verification before "done" |
| `teletype-architect` | Cross-crate design, security and stability review, durable decisions in the brain |
| `teletype-backend-engineer` | Rust: `crates/*`, `evals/`, `Cargo.toml`, `fuzz/` (eval harness also in `crates/teletype-eval/`) |
| `teletype-frontend-engineer` | React/TypeScript: `ui/` and the Tauri IPC JS surface |
| `teletype-qa-engineer` | Tests, eval harness, regression verification, release gate |
| `teletype-deployment-engineer` | CI/release pipelines, signing, notarization, updater manifest, "why did the release not ship" |
| `teletype-productivity-monitor` | Observes the work, writes learnings/skills/tasks, keeps the brain honest |

Routing: hand a multi-part build task to `teletype-engineering-manager`; a single-crate fix to the matching engineer; a "is this really done?" question to `teletype-qa-engineer`; a CI failure, release build failure, signing/notarization, or updater-manifest problem to `teletype-deployment-engineer`; a finished piece of work to `teletype-productivity-monitor` for the knowledge review. All agents share this brain and its precedence rules.

Invoke a role by name: in OpenCode select `@<role>` or ask for it as a subagent; in Pi type `/<role> <task>`; in an Agent Skills harness type `/<role>` or `/skill:<role>`; from any other agent, read `.agents/agents/<role>.md` and follow it.

## Task classification & evolution checkpoints

Classify every task (TRIVIAL / MEANINGFUL / MAJOR) before working. The level
determines which review is required and which counters to bump. Full rules:
`.agents/workflows/task-classification.md`. Checkpoint counters live in
`.agents/state/evolution-state.yaml` (specialist review every 10 meaningful
tasks, full health review every 30, plus immediate triggers). Those two files
are the committed procedure; the `/ali-evolve-agents` and `/ali-agent-health`
skills that used to wrap them are not in this repository, so run the checks
from the workflow files by hand.

## Project skills (.agents/skills/)

The eight team roles are also exposed as generated, user-invoked skills in
`.agents/skills/<role>/SKILL.md` (see `.agents/agents/REGISTRY.md`). The only
hand-written, model-invoked skill is:

| Skill | Trigger | Purpose |
|---|---|---|
| `brag` | `/brag`, "make a launch video" | Turn the project website or code into a short HyperFrames launch video. |

The `/ali-pi-team`, `/ali-verify-pi-work`, `/ali-app-dev-loop`,
`/ali-agent-health` and `/ali-evolve-agents` skills are **not** in this
repository. Do not cite them as loadable skills. The pi-workflow knowledge they
used to hold survives as prose in this file (see "Multi-agent behavior") and as
the task reports committed under `tasks/pi-reports/`.

## Decisions (ADR)

Durable architecture and product decisions live in `PROJECT_BRAIN.md` under
Decisions (D001–D044). Decisions that arrive with a code change and deserve a
standalone record live in `.agents/brain/adr/` as `ADR-NNN-short-name.md`
(decision, context, alternatives, consequences, evidence, status, date). Do not
duplicate a decision in both places: the brain keeps the one-line pointer, the
ADR keeps the full reasoning.

<!-- agentmemory:start -->
## Agent memory (agentmemory)

You have persistent long-term memory via the agentmemory MCP server. Tools: `memory_recall`, `memory_smart_search`, `memory_save`, `memory_sessions`.

- At the START of a task, call `memory_recall` (or `memory_smart_search`) with the task context to load relevant past decisions, fixes, and preferences before asking the user to repeat anything.
- When you learn something durable (a decision, a fix, a gotcha, a user preference, a project convention), call `memory_save` to persist it.
- Prefer recalling over re-deriving, and save concise reusable facts rather than transcripts.
<!-- agentmemory:end -->
