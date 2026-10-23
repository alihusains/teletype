# L004: Teammates read the lead's chat summaries and treat narrated sequencing as binding

**ID:** L004
**Date:** 2026-09-25
**Agent:** OpenCode (team lead)
**Project area:** multi-agent coordination / team dispatch
**Status:** candidate
**Evidence:** Two teammates (omp2, then Pi4) independently declared themselves "blocked, standing by" on task T7b even though their direct task briefs said steps 1-4 were unblocked. Both quoted the phrase "ITN -> Phase B language backend -> T7b" which appeared only in the lead's user-facing progress summary, not in any task description. (T7b brief: start state.rs/pill.tsx/tray.rs/App.tsx now, dictation.rs behind a window grant only.)

## Problem

The lead narrates plan changes to the user in chat (e.g. "revised sequencing: A -> B -> C"). Teammates apparently can see that chat. When a direct dispatch brief gives a different (partial, finer-grained) order, teammates follow the older narrated order and stall.

## Symptoms

- "I am standing by until X and Y complete" messages that contradict the task brief.
- Idle teammates whose briefs explicitly said to start now.
- Two incidents in a row for the same task (omp2 then Pi4), proving it is systematic, not one model's confusion.

## Initial hypotheses

1. Task brief unclear (partially true: sequencing language was in the brief too).
2. Teammates read the lead's chat summaries and over-weight them (confirmed: both quoted the summary phrase verbatim).

## Investigation

- omp2's stall: replied "blocked on Pi's ITN landing + Phase B completing first" - wording traced to the lead's user summary, not the task.
- Pi4's stall: identical wording ("ITN -> Phase B language backend -> T7b") minutes after receiving a brief that said the opposite for steps 1-4.

## Root cause

Narration and dispatch are two channels to the same audience. A summary that simplifies sequencing for the user becomes a stale, over-broad instruction set for teammates.

## Fix

- When sequencing changes, restate it directly to the affected teammate in the dispatch message, explicitly superseding earlier orders ("any sequencing you saw in chat is superseded by this brief").
- Make dispatch briefs lead with the imperative: "START NOW: <exact file>", and require a confirmation reply naming the first concrete edit.
- Keep narrated sequencing descriptions minimal while tasks are in flight.

## Verification

- Corrective message to Pi4 required a one-line confirmation that step 1 (state.rs diff) is underway; follow-up incident would de-validate.

## Reusable lesson

In a shared-chat multi-agent team, the lead's user-facing summaries are de-facto instructions to teammates. Either write them so they cannot be misread as blocks, or immediately follow any plan change with a direct message to every affected slot. Dispatch briefs must be self-contained and lead with what to do now, not with context.
