# T3a — Profiles core and per-app resolution

Status: verified in main tree.

Added `crates/teletype-core/src/profile.rs` with Clean/Natural/Structured/Transform/Off, budgets (15/30/45/explicit/0), §6.3 prompt lines, one app-key mapping table, override precedence, capability fallback, and an info payload. Threaded profile resolution into pipeline/PromptContext/engine; added Settings `polish_profile` and `polish_app_profiles` with legacy serde; exposed profile data via the existing `get_polish_prompt_info` command payload.

Precedence: encoded explicit override -> per-app map (bundle id -> display name -> category key) -> app table row -> global profile (Natural). Budget: stricter explicit custom limit wins only when it differs from the shipped 40 default. Capability: cleanup-only + Natural/Structured -> Clean with reason "`{model_id} can only fix errors; used Clean instead of {Label}.`". Transform is never auto-selected.

Verification: profile filter 19/19; final combined core 1368 lib, desktop 607, fmt clean. Lead decisions: pipeline profile fields are resolved internally but not yet populated from desktop category state; no dedicated `get_polish_profile_info` handler (stable payload is on `get_polish_prompt_info`); hard model routing/fallback display remain desktop gaps. No brain change needed.
