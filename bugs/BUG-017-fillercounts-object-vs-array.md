# BUG-017 — fillerCounts is a JSON object but the UI calls .slice() on it; filler panel permanently empty

**Severity:** P2 (silent dead panel)
**Area:** desktop / commands.rs + ui/InsightsScreen.tsx
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `commands.rs` `UsageStats` and `InsightsScreen.tsx:573`

---

## Description

`get_usage_stats` returns `fillerCounts: BTreeMap<String, u32>`, which
serializes to a JSON **object** (camelCase). But `CountList`
(`ui/src/screens/InsightsScreen.tsx:349-355`, used at `:573`) does
`counts.slice(0, 10).map(...)` — treating it as an **array**.

`fillerCounts` arrives as a plain object, so `counts.slice` is `undefined`
→ TypeError thrown during render. The `useEffect` has
`.catch(console.error)` on the invoke, so `usage` stays `null` and the
screen renders its empty state — the crash is masked.

Net effect: the "Filler words removed" panel is **permanently empty**, even
after the user has removed fillers. (The AutoText screen uses the same call
but reads `autotextCounts` only, so it works — but `autotextCounts` has the
same latent bug if it's ever rendered as a list.)

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-desktop/src/commands.rs` | (UsageStats) | `filler_counts: BTreeMap<String, u32>` → JSON object |
| `ui/src/screens/InsightsScreen.tsx` | 349-355 | `CountList` does `counts.slice(0,10).map()` |
| `ui/src/screens/InsightsScreen.tsx` | 573 | `<CountList counts={usage.fillerCounts} ...>` |
| `ui/src/screens/InsightsScreen.tsx` | 401 | `useState` init `{ fillerCounts: {}, autotextCounts: {} }` |

## Reproduction

1. Enable filler removal.
2. Dictate a sentence with filler words ("um", "like").
3. Open Insights.
4. The "Filler words removed" panel shows "No filler words removed yet" even
   though fillers were removed.
5. (Open devtools console — a TypeError on `counts.slice` is logged.)

## Unit test cases (must pass after fix)

```rust
// Backend: fillerCounts must serialize to a sorted array of {word, count}.
#[test]
fn filler_counts_serializes_as_array() {
    let stats = usage_stats_with_fillers(&[("um", 3u32), ("like", 1)]);
    let json = serde_json::to_value(&stats).unwrap();
    let filler = json["fillerCounts"].as_array()
        .expect("fillerCounts must be a JSON array, not an object");
    assert_eq!(filler.len(), 2);
    assert_eq!(filler[0]["word"], "um");    // sorted by count desc
    assert_eq!(filler[0]["count"], 3);
    assert_eq!(filler[1]["word"], "like");
    assert_eq!(filler[1]["count"], 1);
}
```

```tsx
// UI: CountList must accept the agreed shape and render its entries.
// (Pins the fixed contract whichever side you change.)
test("CountList renders object/array counts without throwing", () => {
  render(<CountList counts={fillerCountsFromServer} title="Filler words removed" />);
  expect(screen.getByText("um")).toBeInTheDocument();
  expect(screen.getByText("3")).toBeInTheDocument();
});
```

## Acceptance criteria

- [ ] The "Filler words removed" panel shows the actual filler counts after
      fillers are removed.
- [ ] No TypeError in the devtools console.
- [ ] `autotextCounts` has the same shape and renders correctly.

## How to test (manual / smoke)

1. **Smoke (2 min):** remove some fillers via dictation, open Insights.
   - **Pass:** the filler panel lists the counts (e.g. "um — 3").
   - **Fail:** panel shows the empty state; console shows a TypeError.

2. **Regression (1 min):** the AutoText counts panel still works.
   - **Pass:** autotext counts render; devtools console is clean.
   - **Fail:** AutoText panel broke or console shows errors.

## Fix direction

Pick **one** shape and make both sides agree. Preferred: backend emits a
sorted array of `{word, count}` (the UI already expects an array). Change
`UsageStats.filler_counts` and `autotext_counts` from
`BTreeMap<String, u32>` to `Vec<(String, u32)>` (or a small struct) sorted
by count desc. Update the UI type `StrNum` to match.

## Related

- G011 (camelCase/snake_case IPC mismatch) — same class of wire-shape bug;
  `ipc_contract.rs` should be extended to check nested shapes, not just
  names.
