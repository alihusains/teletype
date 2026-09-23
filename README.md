# Project Agent Brain Starter

This starter adds a lightweight, repository-local brain for coding agents.

## Install into an existing project

Copy:

- `AGENTS.md` to the repository root
- `CLAUDE.md` to the repository root (Claude Code reads this and imports `AGENTS.md`)
- `.agents/brain/PROJECT_BRAIN.md` to `.agents/brain/PROJECT_BRAIN.md`
- `.agents/brain/learnings/` for reusable debugging and discovery records

Then start your coding agent and ask it:

> Read AGENTS.md and PROJECT_BRAIN.md. Inspect this existing repository, fill the brain using facts you can verify from the codebase, and ask me only for important missing context. Do not invent project facts.

## Why this layout

`AGENTS.md` is the agent-facing entrypoint and workflow contract.

`PROJECT_BRAIN.md` is the durable project memory.

`learnings/` stores detailed investigations and reusable discoveries.

This keeps the system compatible with different coding agents instead of putting the entire brain inside one vendor-specific file.

## Existing repositories

This does not assume a greenfield project. The first initialization pass should inspect the existing code, docs, configuration, scripts, and deployment setup, then fill only what can be verified.

## Future company brain

A future company brain can be plugged into the `company_brain` section without changing the local knowledge format.

Project-specific facts remain local to the repository.
