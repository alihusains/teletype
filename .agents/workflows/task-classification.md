# Task Classification & Evolution Checkpoints

Deterministic rules that tell an agent **when** it must run a review, so the
maintenance steps are not left to memory. This is the missing determinism layer
on top of the existing `ali-evolve-agents` (what to check) and `ali-agent-health`
(what to validate) skills.

Source: `PROJECT_AGENT_OS_PRODUCTION.md` §3 (task levels), §17 (frequency),
§18 (state). Adapted to the existing `.agents/` layout — no layout change.

## 1. Classify every task first

Before working, assign one level. The level decides which review is required.

### TRIVIAL
Does not materially change project behavior or durable knowledge: typo, comment
wording, formatting, doc wording, answering a question, rerunning a command with
nothing new learned.
- **Review:** none required. (If new evidence emerges, re-classify.)

### MEANINGFUL — at least one of
1. Application/service behavior changed.
2. Code structure or implementation changed materially.
3. Config / infra / CI / deployment behavior changed.
4. Tests or validation strategy changed.
5. A bug or root cause was discovered.
6. A durable gotcha or learning was discovered.
7. A failed approach or reusable pattern was discovered.
8. A business rule, requirement, workflow, or integration contract changed.
9. A dependency or external integration changed materially.
10. A specialist's scope, knowledge, tooling, or escalation boundary changed.
11. A new domain/module was introduced.
12. A recurring agent mistake was discovered.
13. An architectural decision was made or changed.
- **Review:** post-task evolution check (below) + increment counters.

### MAJOR — significantly affects architecture, security, data, infra, or core
business behavior, or multiple domains: schema redesign; auth/security flow;
major API contract change; framework/runtime migration; deployment architecture
change; new bounded domain; large cross-domain feature; incident root-cause;
core dependency removal/replacement.
- **Review:** post-task evolution check + a **deeper specialist/architecture
  review** (see §3, immediate triggers).

When unsure between levels, take the higher one.

## 2. Post-task evolution check (every MEANINGFUL task)

Run the `ali-evolve-agents` skill (its 8 questions + promotion rules are the
checklist). Then, and only if evidence supports it, update the affected brain
entry / specialist / rule / skill / test — never every file. Then update the
counters in §4.

## 3. Checkpoints

Counters live in `.agents/state/evolution-state.yaml` (see §4). A plain Markdown
file cannot run a timer, so thresholds are driven by **meaningful-task
counters**, not wall-clock time.

| Trigger | Review | Reset |
|---|---|---|
| Every MEANINGFUL task | Post-task evolution check (§2) | — |
| `meaningful_tasks_since_specialist_review` reaches **10** | **Specialist Review**: for each ACTIVE specialist check stale refs, ownership, repeated mistakes, missing/unnecessary specialists; update only where evidence supports it | reset counter to 0 |
| `meaningful_tasks_since_full_health_review` reaches **30** | **Full Health Review**: run the `ali-agent-health` skill (indexes, registry↔reality, brain references, decision staleness, learning lifecycle, duplicates, specialist overlap) | reset counter to 0 |
| Before a major release | At least a Full Health Review + validate affected brain/specialist refs | — |

### Immediate triggers (do not wait for the next checkpoint)
Run a deeper review now when any of: architecture change; major security change;
major data/schema change; major dependency/runtime migration; production
incident; a repeated bug or repeated agent mistake; a new major domain; a
specialist validation failure.

## 4. Updating `evolution-state.yaml`

After every MEANINGFUL task:
1. Increment `meaningful_tasks_total`.
2. Increment `meaningful_tasks_since_specialist_review` **and**
   `meaningful_tasks_since_full_health_review`.
3. Perform any immediate-trigger review (§3).
4. When a counter hits its threshold (10 / 30), run that review, set its
   `last_*` date, and reset that counter to 0.
5. Update the relevant `last_*` dates.

If the state file is missing or corrupt, do not guess: repair it from git
history, or mark the counters unknown and run a Full Health Review.

## Do not
- Update every artifact after every task — only the affected ones.
- Run an expensive full review after a trivial task.
- Skip the counter increment because "nothing changed" — the count is what
  makes the 10/30 checkpoints honest.
