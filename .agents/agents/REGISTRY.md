# Teletype build team agent registry

Canonical role definitions live in this directory, one file per role. They are the
single source of truth. Adapters in `.opencode/agents/`, `.pi/prompts/`, and
`.agents/skills/<role>/SKILL.md` are generated from them and stay thin on purpose:
each adapter points back here rather than copying the role body.

After editing a canonical file, regenerate and check drift:

```sh
python3 scripts/generate-project-agents.py
python3 scripts/generate-project-agents.py --check
```

## Roles

| Role | Definition | Owns | Escalates to |
|---|---|---|---|
| `teletype-product-manager` | [file](teletype-product-manager.md) | roadmap, README, CHANGELOG, `docs/**` except signing doc | engineering-manager |
| `teletype-engineering-manager` | [file](teletype-engineering-manager.md) | `tasks/**`, workflow | architect |
| `teletype-architect` | [file](teletype-architect.md) | brain Decisions, `adr/**` | none |
| `teletype-backend-engineer` | [file](teletype-backend-engineer.md) | `crates/**`, `evals/**`, Cargo files, `fuzz/**`; QA owns `crates/teletype-eval/**` | architect |
| `teletype-frontend-engineer` | [file](teletype-frontend-engineer.md) | `ui/**` except QA test dirs | architect |
| `teletype-qa-engineer` | [file](teletype-qa-engineer.md) | tests, eval harness, `scripts/e2e/**` | engineering-manager |
| `teletype-deployment-engineer` | [file](teletype-deployment-engineer.md) | workflows, release checks, signing doc | architect |
| `teletype-productivity-monitor` | [file](teletype-productivity-monitor.md) | learnings, hand-written skills, counters | engineering-manager |

## Invoke

- **OpenCode:** select `@<role>` as the primary agent, ask the primary agent to use
  it as a subagent, or mention `@<role>` in a prompt. Definitions live in
  `.opencode/agents/<role>.md`.
- **Pi:** `/<role> <task>` expands the project prompt template in `.pi/prompts/<role>.md`.
- **Agent Skills harnesses** (Claude Code, OpenCode, Pi, Codex): `/<role>` or
  `/skill:<role>` loads `.agents/skills/<role>/SKILL.md`.
- **Any other agent:** read `.agents/agents/<role>.md` and follow it directly.

All adapters enforce the same rules: `AGENTS.md` precedence, brain freshness before
and after meaningful work, no destructive git commands, and the minimal final
response format.