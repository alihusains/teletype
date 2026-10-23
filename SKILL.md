---
name: etoro-public-api-operations
description: Operate the eToro Public API through the etoro-public-api-mcp MCP server. Use when the user wants to discover eToro Public API routes, understand what a route does, or execute a Public API operation (get portfolio, search instruments, open or close a trade, place or cancel an order, transfer money, etc.). Covers MCP installation, route discovery, authentication, demo vs real accounts, request building, execution, and safety rules for money-moving operations.
---

# eToro Public API Operations

Operate the **eToro Public API** end-to-end using the `etoro-public-api-mcp` MCP server: discover routes, explain them, and execute real API calls on behalf of the user.

> **This skill is pre-configured for this eToro Public API deployment.**
> MCP server URL: `https://mcp.public-api.etoro.com`
> Skill version: `1.21.3`

The MCP server refreshes the Public API OpenAPI document every few minutes and exposes seventeen tools — three for discovery, two for execution, one identity/scopes lookup, one watchlists lookup, one all-accounts balance lookup, one instrument overview, one portfolio aggregation, one public-trader profile lookup, one positions/orders lookup, one trading-history lookup, two for opening a new trade (`prepare-trade` + `place-trade`), and two for closing an existing position (`prepare-close` + `place-close`):

| Tool | Purpose |
|------|---------|
| `get-tags` | Returns **only** the catalog's tag groups: a `tags` map (tag -> route count), `totalRouteCount`, `untaggedRouteCount`, and the current `skillVersion`. Its size tracks the number of GROUPS, not the number of routes, so it is the cheap way to orient. **Start here**, then call `get-all-routes` with the tag you picked — see Phase 1. Counts follow the same partner visibility rules as `get-all-routes`. **No arguments** |
| `get-all-routes` | Returns the current **`skillVersion`** and routes as `routeId -> "<METHOD> <path> - <summary>"` (deprecated routes are prefixed `[DEPRECATED] `). **Narrow it with the optional `query` (keywords) and `tag` (one group) arguments** — see Phase 1. Always returns a `tags` map (tag -> route count, the menu of valid `tag` values), `totalRouteCount`, a `deprecatedRouteIds` list, a `mutatingRouteIds` list (routes that actually change state), a `readOnlyPostRouteIds` list (POST routes that are semantically **read-only** — run them with `execute-read`, no confirmation needed), and — when you filtered — a `filterNote` saying how far it narrowed. Also returns a `supersededBy` map (deprecated routeId -> the routeId that REPLACES it). Per-route **scopes and rate limits are not here**; they are in `get-route-spec`. **No `apiKey` argument** — partner routes appear automatically when the connection carries a partner `x-api-key` (see Partner routes below), adding `[PARTNER] `-prefixed routes plus `partnerRouteIds` + `partnerNote` |
| `get-route-spec` | Returns the full OpenAPI spec for one route: a **Deprecated** banner when applicable, parameters, request body, responses, referenced schemas, an **Authentication** section, the **Required scopes**, and a **Rate limit** section (base limit/window + whether the budget is shared across a group of endpoints). A deprecated route's banner **names the route that replaces it** when one is published. The spec's **closing line names the execute tool** to run the route with — follow it. **No `apiKey` argument** — partner routes resolve automatically when the connection carries a partner `x-api-key` |
| `execute-read` | **Executes a read**: every GET route, plus the small set of **read-semantics POST routes** (cost/what-if previews, eligibility dry-runs, bulk lookups — POST only carries the request body, nothing changes state; they are listed in `readOnlyPostRouteIds` and never in `mutatingRouteIds`). Returns the API's raw response: `statusCode`, `isSuccess`, `body` (verbatim and **whole**, up to **16 MB**; a body over 16 MB is not returned at all, see Read the result), `rateLimit*` info (when present), and the `xRequestId` that was sent. Arguments: `path` (the route's RELATIVE path with path parameters substituted), an optional `query` object, and — **only for a read-semantics POST route** — an optional raw-JSON-**string** `body`. The server verifies the route against the live catalog and refuses a body on anything else, so nothing state-changing can ever go through this tool. **No confirmation gate.** **No credential arguments** — credentials come from the MCP connection's headers (see Authentication) |
| `execute-write` | **Executes a state-changing route** — `method` is one of POST/PUT/PATCH/DELETE, with an optional raw-JSON-**string** `body`. Same `path`/`query` arguments as `execute-read`, and the same connection-header credentials. Subject to the money-moving **confirmation gate** (Phase 5). Read-semantics POST routes belong on `execute-read` instead — the gate does not apply to them even here. To retry the SAME operation idempotently after a timeout/`5xx`, pass back the `xRequestId` from the failed result |
| `get-my-portfolio-summary` | **One call, the connected user's whole portfolio** — a server-side aggregation that returns the user's condensed portfolio with zero route discovery: `totals` (total value, available/frozen cash, balance, unrealized P&L, used margin), `holdings` sorted by value descending (each with symbol/name/logo, invested, value, P&L, units, rates, leverage, exposure, position count), `copiedTraders` (with usernames and avatars) and `pendingOrders`. Arguments — **all optional**: `account` (`"real"` default / `"demo"`), `includePositions` (default `false` — adds per-position rows under each holding; large portfolios add tens of KB), `includeOrders` (default `true`), `includeCopiedTraders` (default `true`; `false` also skips the upstream calls that resolve them). **Read-only and idempotent — no confirmation gate.** Credentials work exactly like the execute tools — they ride on the connection; an anonymous call is challenged with `401` + `WWW-Authenticate`. Every price and P&L comes from ONE consistent snapshot — never re-price the result against live quotes. `warnings` (`null` when fully complete) lists degraded enrichment; the money numbers remain authoritative. `statusCode` is 200 on success, the upstream status when the portfolio fetch failed, `0` on timeout; a `429` carries `retryAfterSeconds` — see Phase 0 |
| `get-my-balances` | **Every account the user holds, in ONE call** — Trading (with its sub-accounts), Cash, Crypto, Options, MoneyFarm, Spaceship and Plaee. Use it for "how much money do I have", "what is my balance", "how much can I spend", and for any funds OUTSIDE the Trading account; `get-my-portfolio-summary`'s totals are Trading-only. The aggregator's response is relayed **verbatim** under `balances` — this tool converts nothing, sums nothing and subtotals nothing. **Read `fieldGuide` before reading any number**: the spendable figure is in a DIFFERENT field per account type — `equityDetails.available` (Trading, Cash, Options, MoneyFarm, Plaee), `equityDetails.spendableBalanceInFiat` (Crypto), and **none at all** for Spaceship, which has no `equityDetails`. `totalBalance` and each row's `balance`/`displayBalance` are **portfolio value, not spendable cash** — except on a Cash account, where `balance` equals `available`. **Currency trap:** every `equityDetails` amount is in that account's OWN native currency (the row's `currency`), never in `displayCurrency`, and the object carries no currency marker — convert with the row's `exchangeRate` before comparing. Sub-accounts arrive as `accountType: "Trading"` with `subType: "subAccount"`, named by `equityDetails.username`; only APPROVED ones appear, and `notes` says so. Arguments — **all optional**: `displayCurrency` (default `"USD"` — affects `displayBalance`/`totalBalance` only), `accountTypes` (comma-separated filter; omit for all), `includeSubAccounts` (default `true` — leave it on), `includeZeroBalances` (default `false`). **Read-only and idempotent — no confirmation gate.** Needs the `etoro-public:money.balance:read` scope, a SEPARATE grant not implied by `real:read`. Credentials ride on the connection; an anonymous call is challenged with `401` + `WWW-Authenticate`. A `502` or `statusCode` `0` means the balance could NOT be read — never a zero balance — see Phase 0 |
| `get-instruments-overview` | **1 to 100 instruments in ONE call — the FIRST tool for any question about an instrument or market**: current price, spread, recent performance, whether and how the connected user can trade it. Use it BEFORE `prepare-trade` when exploring a trade, and instead of composing market-data routes via `execute-read`. **Batch-first: 1 and 100 instruments cost the SAME single call — pass every instrument of interest at once, never loop it per instrument.** Arguments: `symbols` (ticker array) and/or `instrumentIds` (id array) — at most 100 combined — or `query` (free-text name/ticker search for when the exact symbol is unknown; mutually exclusive with the other two; top matches come back in search relevance order). Returns per instrument: `market` (id, symbol, name, logos), `quote` (`ask`/`bid`/`spread`/`asOf`), `performance` (`previousClose` + `dailyChangePercent`/`weeklyChangePercent`/`monthlyChangePercent` computed from official closing prices against the live rate) and `eligibility` (allowOpenPosition, min position exposure, max units per order, allowed long/short leverages, MIT/entry-order/trailing-SL support, W-8BEN, fractional/whole units). **Eligibility reflects the CONNECTION's account — there is no account argument.** Unknown identifiers come back in `notFoundSymbols`/`notFoundInstrumentIds`, never as an error while at least one instrument resolved; `warnings` names degraded sections (quote/performance/eligibility/metadata) whose fields are then null while everything else stays authoritative. **Read-only and idempotent — no confirmation gate**; quotes move, so re-call rather than cache. Credentials ride on the connection; an anonymous call is challenged with `401` + `WWW-Authenticate`. On a 429 wait `retryAfterSeconds` |
| `get-my-profile-and-scopes` | **Who is connected, in ONE call — the FIRST tool for "who am I", "which account is this" or "what can this connection do"**. Returns `profile` with the user's account ids (`gcid`; `realCid`/`demoCid` — the "cid" account-scoped routes take), `username`, `firstName`/`middleName`/`lastName`, `playerLevel` (1 Bronze, 2 Platinum, 3 Gold, 4 Internal, 5 Silver, 6 PlatinumPlus, 7 Diamond), `gender` (0 Unknown, 1 Male, 2 Female), `language`, `dateOfBirth`, `avatarUrl` and `scopes` — the OAuth scopes ACTUALLY granted to this connection's token/key — plus `authChannel` (`"bearer"`/`"keys"`). **No arguments** — the credential on the connection is the input. The profile carries personal data (name, date of birth): use it to answer, do not repeat it back unless asked. Scope names are self-describing (`etoro-public:trade.real:write` = may place real-account trades); two non-obvious rules: `real:write`/`real:read` are full-account umbrellas that INCLUDE the matching `trade.real` permissions, while `money.balance:read` and `market-data:read` are separate grants NOT implied by an account umbrella. Scopes reflect the connection's GRANT, not user-level account state (a trading block still surfaces only at placement). **Read-only and idempotent — no confirmation gate.** Credentials ride on the connection; an anonymous call is challenged with `401` + `WWW-Authenticate` — see Phase 3.5 |
| `get-my-watchlists` | **The connected user's watchlists in ONE call — the FIRST tool for "what is on my watchlist" / "show my watchlists"**. Relays the Public API watchlists document (watchlist BFF V2, `GET /api/v1/watchlists`) VERBATIM under `watchlists`: `status`, `isSucceeded`, the `watchlists` array (`watchlistId`, `name`, `Gcid`, `watchlistType`, `totalItems`, `isDefault`, `isUserSelectedDefault`, `watchlistRank`, `items`, optional `relatedAssets`/`dynamicUrl`), optional `exception`, and `meta` (pagination and limits). Plus the standard envelope (`statusCode`, `error`, `retryAfterSeconds`, `xRequestId`, `authChannel`). **No arguments** — the credential on the connection is the input. Needs any one of `etoro-public:watchlist:read` / `watchlist:write` or a real/demo read/write umbrella. **Read-only and idempotent — no confirmation gate.** Credentials ride on the connection; an anonymous call is challenged with `401` + `WWW-Authenticate`. For pagination / related-assets query options beyond route defaults, use `execute-read` on the same path |
| `get-trader-profile-summary` | **Public profile of 1 to 100 eToro investors in ONE call — the FIRST tool for any question about a trader, popular investor or copy-trading candidate**: who they are, performance, risk score, copier statistics and portfolio composition. **Batch-first: 1 and 100 usernames cost the SAME single call — pass every trader of interest at once, never loop it per trader.** Arguments: `usernames` (**required**, 1–100, case-insensitive, duplicates collapsed) and `period` (optional ranking period, default `CurrMonth`; other whitelisted values include `OneMonthAgo`, `ThreeMonthsAgo`, `SixMonthsAgo`, `CurrYear`, `OneYearAgo`, `LastYear`, `LastTwoYears` — an unknown value relays the upstream `400`). Returns per trader: `profile` (name, country, avatar, AUM tier, industry/sector tags), `performance` (gain for the period, annualized return, drawdowns, win ratio, profitable weeks/months), `risk` (current and max daily/monthly risk score, 1 lowest–10 highest), `copiers` (copiers, copiers gain, copy investment share) and `portfolio` (exposure, leverage mix, long-position share, top traded instrument, activity). **Gains and percentages are FRACTIONS (0.0432 = 4.32%); AUM is whole USD.** This is PUBLIC data about OTHER traders — for the connected user's own holdings use `get-my-portfolio-summary`. Usernames the rankings surface will not answer for come back in `notFoundUsernames` — the upstream deliberately does not distinguish nonexistent, private and unranked (a single-username lookup relays that as `statusCode` 404 with no error). **Read-only and idempotent — no confirmation gate.** Credentials ride on the connection; an anonymous call is challenged with `401` + `WWW-Authenticate` — see Phase 0.75 |
| `get-my-positions-and-orders` | **The account's CURRENT positions and pending orders in ONE call** — both directly opened and held via copy-trading (mirrors). `direct` carries positions/orders opened by the account itself; `mirrors` is a list grouped by `mirrorId`, each carrying that copied trader's positions/orders — kept SEPARATE from `direct` because the underlying API itself never mixes them; only the numeric `mirrorId` is given here, use `get-my-portfolio-summary` for a copied trader's username/avatar. Each position has `pnl`/`pnlPercent` — UNREALIZED, live mark-to-market P&L — plus `stopLossRate`/`takeProfitRate` (null when not set). Each order has a `status` plus its own requested SL/TP rates; pass `includeOrderDetail=true` to add a richer per-order `detail` object (numeric status/error code, `executions`) fetched with one extra call PER order, capped at 10 orders — omit it unless that extra detail is actually needed. Arguments: `account` (`"real"` default / `"demo"`), `includeOrderDetail` (default `false`). All money amounts are in `accountCurrency`. **Read-only and idempotent — no confirmation gate.** Credentials ride on the connection; an anonymous call is challenged with `401` + `WWW-Authenticate`. On a 429 wait `retryAfterSeconds` |
| `get-my-trading-history` | **CLOSED trades (trading history) in ONE call, paginated.** Each trade carries symbol/name, direction, leverage, open/close time and rate, units, investment, fees, and `netProfit` — the REALIZED, closed profit/loss, a different concept from `get-my-positions-and-orders`' `pnl` (unrealized, still-open). `netProfit` does NOT include fees — fees is a separate, additional cost, so the fully-loaded result is `netProfit` minus `fees`. `parentPositionId` (non-zero) is the position ID of the copied trader's own position that this trade mirrors, confirming a copy-trading fill; `isMirrorTrade` is just that check as a boolean. `stopLossRate`/`takeProfitRate` are always present as numbers on this route — never null or omitted — so a near-zero placeholder (e.g. `0.0001`) can mean "not really set" rather than an active stop. Arguments: `account` (`"real"` default / `"demo"`), `minDate` (format YYYY-MM-DD, defaults to 90 days ago), `page` (default 1), `pageSize` (default 50, capped at 200). The upstream API has NO total-count field, so `hasMore` is a heuristic (true when a full page came back) — a false positive costs one extra, empty next call. This route answers `404` for a MALFORMED request, not "no trades found" — an empty result set is a `200` with an empty list. A `minDate` spanning much more than 90 days is known to be slower upstream and can time out (`statusCode` 0) — prefer a recent `minDate` and treat a timeout as a signal to narrow the range, not retry unchanged. **Read-only and idempotent — no confirmation gate.** Credentials ride on the connection; an anonymous call is challenged with `401` + `WWW-Authenticate`. On a 429 wait `retryAfterSeconds` |
| `prepare-trade` | **Validates a proposed OPEN order against LIVE data and mints a short-lived signed confirmation token** — ALWAYS the first step for opening a position; never open positions via `execute-write` when this tool exists. **Read-only and idempotent — nothing is placed.** Arguments: `account` (**REQUIRED, no default** — `"real"`/`"demo"`; ask the user when unclear, recommend demo for first tries), `direction` (`"buy"` long / `"sellShort"` short), exactly ONE of `symbol`/`instrumentId`, `orderType` (`"mkt"` default / `"mit"` requires `triggerRate` / `"limitIOC"` requires `limitRate` within 10% of market), `leverage` (default 1; above 1 requires `stopLossRate`), exactly ONE of `amount`/`units`/`contracts`, `stopLossRate` (required for leverage>1, `sellShort` or trailing), `takeProfitRate`, `stopLossType` (`"fixed"` default / `"trailing"`). Checks with REAL API data: the user's instrument eligibility (direction, leverage values, order type, min position exposure/margin, max units, SL/TP bounds), the live ask/bid quote, the full cost breakdown (markup, marketSpread, transactionFee, overnightFee, overWeekendFee, sdrt), and the account's **available balance** vs amount + costs. Returns `verdict` (`"ready"`/`"rejected"`), `reasons` (hard failures — each alone withholds the token), `warnings` (soft findings: unverifiable enrichment, W-8BEN, market-closed behavior), a `confirmation` block to SHOW THE USER (market identity + logos, direction, sizing, leverage, `estimatedUnits`, quote, cost lines, balance with `availableCash`/`required`/`sufficient`/`source`, eligibility summary), and — ONLY on `"ready"` — `token` + `expiresAt` (**TTL ~2 minutes**). A passing preparation does NOT guarantee acceptance — user-level restrictions (e.g. trading blocks) surface only at execution. Credentials ride on the connection; an anonymous call is challenged with `401` + `WWW-Authenticate` — see Phase 0.5 |
| `place-trade` | **Places the EXACT order sealed in a `prepare-trade` token** — the destructive half of the pair, subject to the **confirmation gate**: call it ONLY after showing the user `prepare-trade`'s `confirmation` block and getting explicit approval, and only ONCE per approval. Single argument: `token`. The server verifies the signed token first (stateless — any pod, any region; bound to the connection's credentials; an expired, tampered or wrong-connection token is rejected in-band and NOTHING is sent), POSTs the prepared order with the idempotency key sealed in the token, then **polls the order server-side** (up to ~30 seconds) so you never poll orders-lookup yourself. Returns `outcome` (`executed`/`partiallyFilled`/`rejected`/`cancelled`/`expired`/`pending`/`unknown`), the upstream `status` (`id`, `name`, `errorCode`, `errorMessage`, `decodedHint` — `623` = a user-level trade block `prepare-trade` cannot see), `positions` with fills (avgPrice, units, fees), `orderId` + `referenceId`, and `followUp` guidance for `pending`/`unknown`. `pending` = the order was ACCEPTED and is waiting (market closed / trigger not reached) — NEVER place again; retrying with the SAME token after `unknown` is safe (idempotent by construction) — see Phase 0.5 |
| `prepare-close` | **Validates a proposed CLOSE of an existing position and mints a short-lived signed confirmation token** — ALWAYS the first step for closing a position; never close positions via `execute-write` when this tool exists. **Read-only and idempotent — nothing is closed.** Arguments: `account` (**REQUIRED, no default** — `"real"`/`"demo"`), `positionId` (**required** — from `get-my-positions-and-orders` or a prior trade result), `unitsToDeduct` (optional — omit for a FULL close; when given must be `> 0` and cannot exceed the position's own units). Confirms the position belongs to the connected account (an unknown or not-owned `positionId` means `verdict: "rejected"` with no token). Copied (copy-trading / mirror) positions can ONLY be closed in full — `unitsToDeduct` on one of those is `verdict: "rejected"` with no token. For a partial close of a **direct** position, checks the instrument allows partial closes and that the remaining exposure after the close would not fall below the instrument's minimum. Returns `verdict` (`"ready"`/`"rejected"`), `reasons` (hard failures — each alone withholds the token), a `confirmation` block to SHOW THE USER (`positionId`, market identity, direction, leverage, units, `requestedUnitsToDeduct` when partial, open rate, current rate, invested amount, unrealized P&L), and — ONLY on `"ready"` — `token` + `expiresAt` (**TTL ~2 minutes**, same signing/expiry posture as `prepare-trade`). The token is bound to this connection's credentials and carries a distinct Kind so it can NEVER be redeemed by `place-trade` (or an open token by `place-close`). Credentials ride on the connection; an anonymous call is challenged with `401` + `WWW-Authenticate` — see Phase 0.6 |
| `place-close` | **Places the EXACT close sealed in a `prepare-close` token** — the destructive half of the pair, subject to the **confirmation gate**: call it ONLY after showing the user `prepare-close`'s `confirmation` block and getting explicit approval, and only ONCE per approval. Single argument: `token`. The server verifies the signed token first (rejects anything not a valid `prepare-close` token — including a `prepare-trade` token — the SAME vague way as a tampered token, so nothing is disclosed about why), POSTs the close request, then **polls the close order server-side** (up to ~30 seconds). Returns `outcome` (`executed`/`partiallyFilled`/`rejected`/`cancelled`/`expired`/`pending`), `positionId`, `orderId`, `unitsClosed`, `errorCode`/`errorMessage`/`decodedHint` (e.g. `620` = already executed, `631`/`632` = already closed / already pending close, `719` = position does not belong to this account, `720` = remaining amount too low, `776`/`779` = partial/units-close validation failure — `623` = account blocked from trading), and `followUp` guidance for `pending`. If NO response is received at all the outcome is `unknown` (never falsely `rejected`) — retrying with the SAME token is safe. `pending` means the close was ACCEPTED and has not reached a final state yet — do NOT close again; check later via `get-my-positions-and-orders` — see Phase 0.6 |

> **Older server deployments:** if `execute-read` comes back as an unknown tool, the deployment predates it — use `execute-get` for GET routes and `execute-write` for the read-semantics POSTs (the confirmation gate does not apply to those routes).

## Partner routes (eToro partners only)

eToro **partner** applications get extra, partner-only routes on top of the public catalog: KYC (verification, documents, screening, questionnaires), sub-account management, cash accounts, user registration/verification, and deposit operations.

- **There is nothing for you to pass.** Partner routes appear automatically when the MCP **connection** carries a partner application's `x-api-key` header — the same header the user already configures to authenticate (see Installing the MCP Server). Partner status is a property of the caller, not of a tool call.
- If the connection's key is **not** a partner key — or there is no key at all — you simply get the public catalog. That is not an error and there is nothing to retry: unknown, non-partner and absent all look identical.
- If a user says they are a partner but partner routes are missing, the fix is in their **client configuration**, not in the call: their MCP server entry needs their partner application's `x-api-key`. Tell them that, and to verify the key with their eToro contact.
- Executing a partner route is unchanged — it authenticates exactly like any other route (see Authentication).

## Keeping this skill up to date

This skill is distributed as a file you downloaded once, so it can fall behind the server. The copy you are reading is **version `1.21.3`**.

- On your **first `get-all-routes` call** of a session, compare its `skillVersion` field to `1.21.3` above.
- If the server's `skillVersion` is **newer**, tell the user once (do not repeat, do not block any work):
  > A newer version of the eToro Public API skill is available (you have `1.21.3`, latest is `<skillVersion>`). Re-download it from `https://mcp.public-api.etoro.com/skill` and replace your local `etoro-public-api-operations` skill.
- If they match, say nothing and proceed normally.

This is an informational nudge only — a slightly stale skill still works; never refuse or delay an operation because of it.

## Installing the MCP Server

This MCP server uses **streamable HTTP** transport (stateless) and lives at:

```
https://mcp.public-api.etoro.com
```

(this deployment's MCP endpoint). It works with **any MCP-capable client or IDE**. Most clients accept the same JSON — register the URL under `mcpServers`:

```json
{
  "mcpServers": {
    "etoro-public-api": {
      "url": "https://mcp.public-api.etoro.com",
      "headers": {
        "x-user-key": "<the user's personal x-user-key>",
        "x-api-key": "sdgdskldFPLGfjHn1421dgnlxdGTbngdflg6290bRjslfihsjhSDsdgGHH25hjf"
      }
    }
  }
}
```

**Credentials live on the MCP server entry, not in tool calls.** The route catalog (`get-all-routes` / `get-route-spec`) works with no `headers` block at all — add it only when you need to execute API calls. A client that authenticates the connection with OAuth instead sends its own `Authorization` header and needs no `headers` block.

Where that config lives, per client:

| Client / IDE | Where to add it |
|--------------|-----------------|
| **Claude Code** (CLI) | `claude mcp add --transport http etoro-public-api https://mcp.public-api.etoro.com`, or add the block to `~/.claude.json` / project `.mcp.json` |
| **Claude Desktop** | `claude_desktop_config.json` (Settings → Developer → Edit Config) |
| **Cursor** | `~/.cursor/mcp.json` (global) or project `.cursor/mcp.json` |
| **VS Code** (GitHub Copilot, agent mode) | `.vscode/mcp.json` or the `mcp` section of user `settings.json` — use a `"servers"` key instead of `"mcpServers"` |
| **Visual Studio 2022** (17.14+) | `.mcp.json` in the solution / `%USERPROFILE%` |
| **Windsurf** | `~/.codeium/windsurf/mcp_config.json` |
| **JetBrains IDEs** (IntelliJ, Rider, PyCharm, WebStorm, GoLand…) | AI Assistant → Settings → Tools → MCP → add an HTTP/SSE server with this URL |
| **Cline / Roo Code** (VS Code extensions) | MCP Servers → Configure → add the `mcpServers` block |
| **Zed** | `context_servers` in `settings.json` |

**Clients that only support stdio** (no HTTP transport) can bridge to the URL with the [`mcp-remote`](https://www.npmjs.com/package/mcp-remote) adapter:

```json
{
  "mcpServers": {
    "etoro-public-api": {
      "command": "npx",
      "args": ["-y", "mcp-remote", "https://mcp.public-api.etoro.com"]
    }
  }
}
```

After installation the seventeen tools appear as `get-tags`, `get-all-routes`, `get-route-spec`, `execute-read`, `execute-write`, `get-my-profile-and-scopes`, `get-instruments-overview`, `get-my-portfolio-summary`, `get-my-balances`, `get-my-watchlists`, `get-trader-profile-summary`, `get-my-positions-and-orders`, `get-my-trading-history`, `prepare-trade`, `place-trade`, `prepare-close` and `place-close`. The route catalog needs no authentication at all; credentials are needed only to execute real eToro Public API calls, and they travel on the connection's own headers rather than as tool arguments (see Authentication below).

## When to Use

- "What can I do with the eToro Public API?" / "Is there a route that does X?"
- "Explain how route X works" / "What do I need to send to X?"
- "Who am I?" / "What can my keys do?" / "Which scopes / accounts do I have?" → `get-my-profile-and-scopes`
- "How much money do I have?" / "What's my balance?" / "How much can I spend?" / "What's in my sub-accounts / crypto / cash account?" → `get-my-balances`
- "What is on my watchlist?" / "Show my watchlists" → `get-my-watchlists`
- "Who is trader X?" / "How good is popular investor Y?" / "Compare these traders" / "Should I copy Z?" → `get-trader-profile-summary`
- "Get my portfolio" / "Search instrument Y" / "What is the price of Z?"
- "What are my open positions/orders?" / "Do I have anything with a copied trader?" → `get-my-positions-and-orders`
- "What trades have I closed?" / "What's my trading history / profit or loss on past trades?" → `get-my-trading-history`
- "Open a trade", "Buy 100$ of BTC" (opening a NEW position goes through `prepare-trade` → `place-trade` — Phase 0.5)
- "Close my position", "Close half of my BTC position", "Sell out of AAPL" (closing an EXISTING position goes through `prepare-close` → `place-close` — Phase 0.6), "Cancel order N"
- Any task that requires calling the eToro Public API with the user's credentials

## Core Workflow

Always follow these phases in order. Never skip discovery — route ids and schemas change as the API evolves, and the MCP always reflects the current document. **Five exceptions:** questions about the user's OWN portfolio start at Phase 0, opening a NEW trade starts at Phase 0.5, closing an EXISTING position starts at Phase 0.6, questions about OTHER public traders start at Phase 0.75, and questions about a specific instrument or market start at `get-instruments-overview` (see Phase 1) — none of them needs any discovery at all.

### Phase 0 — The user's own portfolio (skip discovery)

For any question about the user's OWN account — holdings, balance, portfolio value, P&L, copied traders, pending orders, "what do I hold?" — call `get-my-portfolio-summary` FIRST and skip route discovery entirely: one call returns the condensed portfolio that would otherwise take several routes and several round-trips to assemble. Phases 1–2 remain the path for everything the summary does not carry (market data, trade history, other traders' live holdings, closing or cancelling anything — opening a NEW trade has its own discovery-free path, Phase 0.5, and OTHER traders' public profiles have theirs, Phase 0.75).

- **All arguments optional:** `account` (`"real"` default / `"demo"`), `includePositions` (default `false` — per-position rows under each holding; on large portfolios they add tens of KB, so leave it off unless the user asks about individual positions), `includeOrders` (default `true` — pending orders), `includeCopiedTraders` (default `true` — copied traders with usernames and avatars; `false` also skips the upstream calls that resolve them).
- **Every price and P&L in the response comes from one consistent snapshot** — report them as they are; never re-price holdings against live quotes, which mixes snapshots into numbers that no longer add up.
- **`warnings` is `null` when the response is fully complete.** When present, it names enrichment that degraded (symbols, display names, logos, usernames, avatars, pending orders) — the money numbers remain authoritative, so answer with them and mention what is degraded.
- **Credentials work exactly like the execute tools** — they ride on the connection (Phase 3); an anonymous call is challenged with `401` + `WWW-Authenticate`.

**Balances across ALL accounts — `get-my-balances`.** `get-my-portfolio-summary` covers the **Trading account only**. For "how much money do I have", "what's my balance", "how much can I spend", or anything about Cash, Crypto, Options, MoneyFarm, Spaceship, Plaee or Trading **sub-accounts**, call `get-my-balances` instead — also discovery-free, also all-optional arguments.

- **The response is relayed verbatim and nothing is computed for you.** No conversion, no summing, no subtotals. Read `fieldGuide` first: it names the spendable field for each account type present, because it differs per type (`equityDetails.available` for Trading/Cash/Options/MoneyFarm/Plaee, `equityDetails.spendableBalanceInFiat` for Crypto, none for Spaceship). Never assume `available` exists.
- **`totalBalance` and `displayBalance` are portfolio value, not spendable cash** — except on a Cash account, where `balance` equals `available`.
- **Every `equityDetails` amount is in the account's OWN currency**, never in `displayCurrency`, with no currency marker on the object. A GBP cash account reports `available: 31055.9` (GBP) next to `displayBalance: 41786.95` (USD). Convert with the row's `exchangeRate` before comparing or quoting one number — and never add a Crypto spendable figure to a Trading one: they cannot fund each other.
- **Sub-accounts** come back as `accountType: "Trading"` with `subType: "subAccount"`, named by `equityDetails.username`. Only APPROVED ones appear, so report what came back and do not present it as the user's complete sub-account list — `notes` states this.
- **A `502` or `statusCode` `0` is a failed read, never a zero balance.** Say the balance could not be retrieved and retry; do not report a figure.
- Needs `etoro-public:money.balance:read` — a separate grant, not implied by `real:read`. A `403` says so in the `error`.

**Positions/orders specifically → `get-my-positions-and-orders`.** The summary's `includePositions`/`includeOrders` flags already fold positions and pending orders in alongside account totals — a lightweight inclusion. `get-my-positions-and-orders` is the dedicated, richer view of the same data: `direct` (the account's own positions/orders) and `mirrors` (one entry per copy-trading mirror) stay structurally separate, every position/order comes back regardless of size, and `includeOrderDetail=true` adds a per-order `detail` object (numeric status/error code, fills) the summary never carries. Reach for it when the question is specifically about open positions or pending orders — especially mirror-level detail or order execution detail — rather than the account overall.

**Closed trades specifically → `get-my-trading-history`.** Everything else in Phase 0 describes what the account holds NOW; this tool answers "what did I close, and did I make or lose money on it?" instead. Each trade's `netProfit` is the REALIZED, final profit/loss — never confuse it with `get-my-positions-and-orders`' `pnl`, which is unrealized and only exists while a position is still open. Results are paginated (`page`/`pageSize`, default 50 capped at 200) and `minDate` defaults to 90 days back; the upstream API carries no total-count field, so `hasMore` is a heuristic (true when a full page came back). Prefer a recent `minDate` — a much wider range is known to be slower upstream and can time out.

### Phase 0.5 — Opening a NEW trade (skip discovery)

For opening a NEW position — "buy X", "short Y", "invest 100$ in Z" — use the dedicated pair `prepare-trade` → `place-trade` and skip route discovery entirely. **This REPLACES the old `execute-write` open-order flow**: never open positions via `execute-write` when these tools exist (the old flow remains the fallback ONLY on server deployments that predate the pair — see Troubleshooting and the Worked Example fallback). Closing an EXISTING position has its own dedicated pair too (Phase 0.6). Cancelling a pending order or editing an existing position/order are NOT covered here and still stay on the regular discover-and-`execute-write` path (Phases 1–6), pending their own dedicated tools.

**The confirmation gate — non-negotiable, in this exact order:**

1. **`prepare-trade`** — validate the order against live data and get the confirmation block (+ token when ready).
2. **SHOW THE USER the `confirmation` block** — market (name + symbol), direction, sizing, leverage, `estimatedUnits`, the live quote, the cost lines, the balance check (`availableCash` vs `required`), plus every entry in `warnings`.
3. **Get explicit approval** — a clear "yes" to THIS confirmation, not a standing instruction.
4. **`place-trade` with the `token`** — ONE call per approval, never batched or looped.

**Parameters (`prepare-trade`):** `account` is **required with no default** (`"real"`/`"demo"`) — if the user did not explicitly choose, ASK, and recommend demo for first tries. `direction` is `"buy"` (long) or `"sellShort"` (short). Identify the instrument with exactly ONE of `symbol`/`instrumentId`; size with exactly ONE of `amount`/`units`/`contracts`. `orderType` defaults to `"mkt"`; `"mit"` requires `triggerRate`, `"limitIOC"` requires `limitRate` (within 10% of the market price). `leverage` defaults to 1; any higher value requires `stopLossRate` — as do `sellShort` and `stopLossType: "trailing"`. `takeProfitRate` and `stopLossType` (`"fixed"` default / `"trailing"`) are optional.

**Read the result:**

- `verdict: "ready"` — every hard check passed against REAL API data (the user's instrument eligibility, live quote, cost breakdown, available balance vs amount + costs), and `token` + `expiresAt` are present. Show the confirmation and get approval.
- `verdict: "rejected"` — `reasons` lists the hard failures and there is NO token. Show the user the reasons; fix the order or drop it. **Do not fall back to `execute-write` to force the order through.**
- `warnings` are soft findings: enrichment that could not be verified (e.g. the balance is unreadable with this connection's scopes), a W-8BEN requirement, market-closed behavior. The token is still issued — surface them to the user alongside the confirmation.
- **The token expires in ~2 minutes** — get the user's approval promptly and call `place-trade` before `expiresAt`. If it expires: re-run `prepare-trade`, re-show the FRESH confirmation (quotes and costs move), and get approval again. Never retry-loop an expired token.
- A `"ready"` verdict does NOT guarantee execution: user-level restrictions (e.g. a trading block — upstream error `623`) surface only at placement.

**After `place-trade`:** the server polls the order for you (up to ~30 seconds) and returns a final `outcome`. `pending` means the order was ACCEPTED and is waiting — market closed, or trigger rate not reached, possibly for hours: tell the user, do **NOT** place again, and check later via `get-my-portfolio-summary` or the orders-lookup route on `execute-read` (the result carries `orderId` and `referenceId` for that). After `unknown` (no response received), re-calling `place-trade` with the SAME token is safe — the idempotency key is sealed inside the token, so the order cannot double-execute.

### Phase 0.6 — Closing an EXISTING position (skip discovery)

For closing all or part of an already-open position — "close my position", "sell out of AAPL", "close half of my BTC position" — use the dedicated pair `prepare-close` → `place-close` and skip route discovery entirely. **This REPLACES the old `execute-write` close-order flow**: never close positions via `execute-write` when these tools exist. Cancelling a pending order is NOT covered here and still stays on the regular discover-and-`execute-write` path until it has its own dedicated tool; opening a NEW position is Phase 0.5 (its own dedicated `prepare-trade`/`place-trade` pair, not `execute-write`).

**The confirmation gate — non-negotiable, in this exact order (identical shape to Phase 0.5):**

1. **`prepare-close`** — confirm the position and get the confirmation block (+ token when ready).
2. **SHOW THE USER the `confirmation` block** — market (name + symbol), direction, units (and `requestedUnitsToDeduct` for a partial close), open rate, current rate, invested amount, unrealized P&L.
3. **Get explicit approval** — a clear "yes" to THIS confirmation, not a standing instruction.
4. **`place-close` with the `token`** — ONE call per approval, never batched or looped.

**Parameters (`prepare-close`):** `account` is **required with no default** (`"real"`/`"demo"`) — if the user did not explicitly choose, ASK. `positionId` is **required** — resolve it via `get-my-positions-and-orders` or a prior trade result; never invent it. `unitsToDeduct` is optional: omit it for a FULL close (the default); when the user wants a PARTIAL close, pass the units to close (`> 0`, and no more than the position's own units). Copied (copy-trading / mirror) positions cannot be partially closed — omit `unitsToDeduct` and close in full.

**Read the result:**

- `verdict: "ready"` — the position was found on this account (and, for a partial close, it is a direct position, the instrument allows it and the remainder clears the minimum), and `token` + `expiresAt` are present. Show the confirmation and get approval.
- `verdict: "rejected"` — `reasons` lists the hard failures (e.g. an unknown or not-owned `positionId`, a partial close of a copied position, an invalid `unitsToDeduct`) and there is NO token. Show the user the reasons; fix or drop the request. **Do not fall back to `execute-write` to force it through.**
- **The token expires in ~2 minutes** — get the user's approval promptly and call `place-close` before `expiresAt`. If it expires: re-run `prepare-close`, re-show the FRESH confirmation, and get approval again.
- The token is bound to THIS connection and to the close intent specifically — it can never be redeemed by `place-trade`, and a `prepare-trade` token can never be redeemed by `place-close`.

**After `place-close`:** the server polls the close order for you (up to ~30 seconds) and returns a final `outcome` (`executed`/`partiallyFilled`/`rejected`/`cancelled`/`expired`/`pending`). `pending` means the close was ACCEPTED and has not reached a final state yet: tell the user, do **NOT** close again, and check later via `get-my-positions-and-orders`. If NO response was received at all, the outcome is `unknown` rather than a false `rejected` — re-calling `place-close` with the SAME token is safe. A rejected outcome's `decodedHint` explains common causes (already executed, already closed, already pending close, not owned, remaining amount too low) when the upstream error code is recognized; the verbatim `errorCode`/`errorMessage` remain authoritative either way.

### Phase 0.75 — Public trader profiles (skip discovery)

For any question about OTHER eToro traders — "who is X?", "how risky is Y?", "how many copiers does Z have?", "compare these popular investors", vetting a copy-trading candidate — call `get-trader-profile-summary` FIRST and skip route discovery entirely: one call returns each trader's public profile, performance, risk score, copier statistics and portfolio composition.

- **Batch-first:** 1 and 100 usernames cost the SAME single call — pass every trader of interest in one `usernames` array; never loop the tool per trader.
- **`period` selects the ranking window** (default `CurrMonth`; whitelisted values include `OneMonthAgo`, `ThreeMonthsAgo`, `SixMonthsAgo`, `CurrYear`, `OneYearAgo`, `LastYear`, `LastTwoYears`). Every gain, drawdown and risk maximum in the response describes THAT window — name it when reporting numbers.
- **Gains and percentages are fractions** (`0.0432` = 4.32%); `aumValue` is whole USD. Report them converted, never as raw fractions.
- **`notFoundUsernames` is a domain outcome, not an error:** the rankings surface deliberately does not distinguish nonexistent, private and unranked users (a single-username call relays it as `statusCode` 404 with no `error`). Say "not available" — never guess which of the three it is.
- **This tool covers public PROFILE data.** A trader's live holdings breakdown stays on the discovery path (the people/portfolio routes via Phases 1–2); the connected user's OWN portfolio is Phase 0.
- **Credentials work exactly like the execute tools** — they ride on the connection (Phase 3); an anonymous call is challenged with `401` + `WWW-Authenticate`.

### Phase 1 — Discover

- **Instrument questions skip discovery too:** any question about a specific instrument or market — its price, spread, recent performance, whether/how the user can trade it — goes to `get-instruments-overview` FIRST (1 to 100 instruments in ONE call — pass them all at once, never loop per instrument; `symbols`/`instrumentIds`, or `query` when the exact symbol is unknown), and BEFORE `prepare-trade` when exploring a trade. Fall through to route discovery only for market data it does not carry (candles/history, order books, feeds).

**Default flow: `get-tags` → `get-all-routes` with that exact tag.** Two small calls, neither of which can miss. `get-tags` returns only the tag groups and their route counts, so its size tracks the number of groups (a couple of dozen) rather than the number of routes — it costs the same whether the API has 100 routes or 1000. You then pass a tag back **verbatim**; it is matched exactly, so a paraphrase silently matches nothing.

Prefer this over guessing keywords, since a `query` no route matches in full only returns the closest guesses. **This is a preference, not a restriction** — calling `get-all-routes` with no filters at all is always available and always fine (see below).

1. Call `get-tags`. Pick the group(s) whose name fits the user's intent, then call `get-all-routes` with `tag: "<that exact tag>"` — or several comma-separated when the intent spans groups.
   - Check `untaggedRouteCount`: if it is non-zero, some routes carry no tag and are unreachable by tag alone. When no tag fits, fall back to `query`.
2. `get-all-routes` filters — **both optional; call with neither to get the entire catalog**:
   - **`query`** — keywords matched against the route's id, method, path, summary and tags. ALL of them must match, so add words to narrow and drop words to widen: `query: "position"` casts wide, `query: "close position"` homes in. The route that fits the keywords best is listed **first** — occasionally one lacking a keyword, which `filterNote` then says; the other matches follow. When no route contains every keyword you get **the closest routes instead** (at most 3, best first) and `filterNote` says so — they are suggestions, not matches: read each summary before using one, and widen if none fits.
   - **`tag`** — one API group, e.g. `tag: "Trading - Real"`. Valid values are the keys of the `tags` map (from `get-tags`, or from any `get-all-routes` response — every one carries it). Combine with `query` to narrow further.
     - **Several groups in one call:** pass them **comma-separated** and a route in *any* of them comes back — `tag: "Trading - Demo,Trading - Real"`. Use this whenever the intent spans groups the catalog splits (demo vs real trading, the two Sub-Accounts groups, the SSO groups) instead of calling once per group. Tags are OR'd; `query` still ANDs on top, so `query: "order"` + `tag: "Trading - Demo,Trading - Real"` means *"order routes in either trading group"*.
   - **No filter at all** returns the whole catalog and is a **perfectly good call** — the right one for open-ended questions like *"what can I do with this API?"*, when no tag fits the intent, or whenever you just want to see everything. Filtering is *cheaper*, not *more correct*. Never leave a route unfound — or tell the user an operation is unsupported — when one unfiltered call would settle it.
3. Read `filterNote` when you filtered: it tells you how many of how many routes matched. **If nothing looks right, widen before concluding the route does not exist** — drop a keyword, try a different tag, or call `get-all-routes` with no filters at all. A miss is a search problem far more often than a missing route.
4. Partner operations (KYC, sub-accounts, cash accounts, registration, deposits) appear on their own when the connection carries a partner `x-api-key` — nothing to pass, and `partnerNote` explains the status when it does. Filters apply to the partner catalog too, and `get-tags` counts follow the same rule. See Partner routes above.
5. Every route path is RELATIVE and the execute tools already know where to send it: you always pass the relative path (e.g. `/api/v1/me`), never a full URL. The API's own address is not your concern — the MCP is the only endpoint you talk to.
6. Match the user's intent against the route summaries (e.g., for "open a trade" look for order-creation routes; for instrument lookup look for instrument/market-data routes).
7. If several routes could fit, list the candidates with their summaries and let the user pick — do not guess between materially different operations.

### Phase 2 — Understand the route

1. Call `get-route-spec` with the chosen `routeId`. A **partner-only** route (prefixed `[PARTNER] ` / listed in `partnerRouteIds`) resolves on the same connection that listed it; on a connection without a partner key it is simply not found.
2. Read carefully:
   - **Deprecated banner** — if the spec opens with a `⚠️ DEPRECATED` banner (or the route id appeared in `deprecatedRouteIds` / was prefixed `[DEPRECATED] ` in `get-all-routes`), do not just warn: the banner and the catalog's `supersededBy` map **name the route that replaces it**. Switch to the replacement and read ITS spec instead — especially for trading or money-moving routes. When no replacement is named the route is being retired with no successor; only proceed on it if the user confirms.
   - **Method + path** — confirms the route's path (when executing you pass the RELATIVE path to `execute-read` / `execute-write`; where it is sent is the MCP's business).
   - **Closing line** — names the ONE execute tool to run this route with (`execute-read` for GETs and read-semantics POSTs, `execute-write` for state-changing routes). Follow it instead of deriving the tool from the HTTP verb.
   - **Authentication** section — confirm the required headers (see Authentication below).
   - **Required scopes** section — the scopes that grant access (any one of them). Cross-check these against the scopes reported by `get-my-profile-and-scopes` (Phase 3.5) before executing, so you fail fast instead of hitting a `403`.
   - **Rate limit** section — the base limit/window for the route. If it says **shared**, that single budget is pooled across a group of endpoints (listed), so calling any of them spends the same quota — do **not** assume each endpoint has its own limit when planning a batch or loop. Pace requests accordingly and prefer the demo account for high-volume experimentation.
   - `parameters` — headers, path and query parameters (note which are `required`).
   - `requestBody` schema + example — required fields, enums, mutually exclusive fields.
   - `responses` — success shape and error contract (`ProblemDetails`).
3. If the user only wanted to understand the route — summarize it in plain language (purpose, required inputs, what comes back, auth) and stop here.

### Phase 3 — Authentication (only when executing)

**Credentials are NOT tool arguments.** They travel on the MCP connection's own HTTP headers, so the agent never handles them: they cannot land in a transcript, be echoed back, or be logged as part of a tool call. There is nothing for you to pass and nothing for you to ask for mid-conversation — if a call reports missing credentials, the user's **client configuration** needs fixing.

Exactly ONE of two mutually exclusive channels, mirroring the API's own contract:

| Option | Headers on the MCP server entry | Notes |
|--------|----------------------------------|-------|
| **A — API key pair** | `x-user-key` (the user's personal API key — **secret**) together with `x-api-key` (the application API key — an **identifier, not a secret**; this deployment already has one configured (`sdgdskldFPLGfjHn1421dgnlxdGTbngdflg6290bRjslfihsjhSDsdgGHH25hjf`), so `x-api-key` may be omitted). | Only valid **together**. Long-lived credentials — the default when the user has an API key. |
| **B — OAuth Bearer token** | `Authorization: Bearer <access-token>`, set by the client itself when the connection is OAuth-authenticated (e.g. a connector). | Sent **ALONE**. Tokens **expire** — on a `401` the client refreshes and retries; do not retry-loop. |

- **Never mix the channels.** The API rejects `Authorization` combined with either key header with `422`; the execute tools reject the combination **in-band, before anything is sent**.
- Both channels resolve to the same scopes and permissions — every route accepts either.
- **`x-request-id` is handled for you** — the execute tools generate a fresh GUID per call and return it as `xRequestId`. It is also the idempotency key on order routes: to retry the *same* operation after a network failure, pass the previous result's `xRequestId` back as the `xRequestId` argument; for a *new* operation never reuse one.

If an execute call comes back with an error saying the request carried no eToro credentials, tell the user to add the headers to their MCP server entry (see Installing the MCP Server) or to connect with OAuth — then stop. **Do not ask the user to paste a key into the chat**, and never invent, guess, or reuse credentials from another user or session.

**If the user does paste a key anyway**, treat it as a **secret**: do not echo it back, do not store it in files, do not include it in summaries or logs, and tell them it belongs in the client configuration rather than the conversation. It is a long, opaque token (often ~260 characters) that frequently ends in `-`, `_`, or `__`, and **every character is significant** — when they copy it into their configuration it must go in byte-for-byte, or the API returns `401 Unauthorized — "malformed x-user-key container (Unterminated string in JSON)"`.

### Phase 3.5 — Know your credentials: scopes & accounts (`get-my-profile-and-scopes`)

Once you have the credentials (either option), your **first call should be `get-my-profile-and-scopes`** (no arguments; on older server deployments where it is unknown, fall back to `execute-read` with `path` `/api/v1/me`). It works with any valid credentials — key pair or Bearer token alike — and is the fastest way to:

- **Confirm the credentials work** — a `statusCode` 200 means they are valid (401 means they are not, or the Bearer token expired).
- **Discover the granted scopes** — the `scopes` array tells you exactly what these credentials may do:

  | Scope | Grants |
  |-------|--------|
  | `etoro-public:real:read` | Read real-account data (umbrella — includes the `trade.real:read` permission) |
  | `etoro-public:real:write` | Trade / modify on the **real** account (umbrella — includes the `trade.real` permissions) |
  | `etoro-public:trade.real:read` / `:write` | Read / place real-account trades specifically |
  | `etoro-public:demo:read` / `:write` | The demo-account mirrors (umbrellas over `trade.demo`) |
  | `etoro-public:market-data:read` | Market data (separate grant — NOT implied by an account umbrella) |
  | `etoro-public:money.balance:read` | Balance (separate grant — NOT implied by an account umbrella) |
  | `etoro-public:user-info:read` | Read profile / user info |

- **Learn the account identifiers** — `realCid` and `demoCid`, needed by account-scoped routes — plus who the user is (username, name, avatar) and `authChannel` (how the connection authenticated).

Use the result to gate what you attempt: if the user asks to place a **real** order but the credentials only carry `etoro-public:demo:write`, tell them up front instead of triggering a `403`. Recommend it whenever the user is unsure what their credentials can do.

### Phase 4 — Demo vs Real

Many trading routes exist in two variants, e.g. `createDemoOrder` (`/api/v2/trading/execution/demo/orders`) and `createRealOrder` (`/api/v2/trading/execution/orders`).

- If the user explicitly said real/demo — use that.
- If the user did NOT specify — **ask** which account to use, and recommend **demo** for first-time tries.
- Read-only routes (portfolio, market data, search — including the read-semantics POSTs in `readOnlyPostRouteIds`) need no such gate.

### Phase 5 — Build and confirm

1. Build the execute-tool call: the route's RELATIVE `path` with path parameters substituted, the `query` object, the `method` (writes), and a JSON `body` string that satisfies the schema. You never pass a full URL — the MCP knows where to send the request.
2. Validate against the spec before sending:
   - All `required` fields present. Credentials are NOT part of the call — they come from the connection (Phase 3) — and `x-request-id` is attached by the tool automatically.
   - Enum values exactly as the schema defines (e.g., `action: open`, `transaction: buy`, `orderType: mkt`).
   - Mutually exclusive constraints respected (order size must use **exactly one** of `amount`, `units`, `contracts`).
   - Resolve referenced data first (e.g., `instrumentId` via an instrument-lookup route) rather than guessing ids.
3. **Confirmation gate — for any money-moving operation** (creating/cancelling orders, opening/closing positions, transfers, withdrawals, anything POST/PUT/DELETE on real-account trading or money routes): show the user the exact resolved request (path, method, body, account type) and get an explicit "yes" BEFORE sending. No credential appears in the call, so there is nothing to mask. This applies identically to **every `execute-write` call** on such routes. Reads need no confirmation — GETs and the read-semantics POST routes alike (`readOnlyPostRouteIds` / absent from `mutatingRouteIds`; the server verifies them read-only before sending).

### Phase 6 — Execute and report

Execute through the MCP execute tools. The call carries no credentials — they ride on the connection.

A read — `execute-read`:

```json
{
  "path": "/api/v1/me"
}
```

A read-semantics POST — also `execute-read` (no `method`, no confirmation; `body` is the JSON request body **as a string**):

```json
{
  "path": "/api/v2/trading/info/costs",
  "body": "{ ...the order to price, per schema, JSON-escaped into a string... }"
}
```

A write — `execute-write` (note: `body` is the JSON request body **as a string**):

```json
{
  "path": "/api/v2/trading/execution/orders",
  "method": "POST",
  "body": "{ ...body per schema, JSON-escaped into a string... }"
}
```

Read the result:

- `statusCode` / `isSuccess` / `body` are the API's response, relayed **faithfully** — a `400`/`401`/`403`/`429`/`5xx` arrives here, never as a tool error.
- `error` is set ONLY for tool-level failures: invalid arguments (bad path/method, mixed credentials — nothing was sent), a **connection token that failed verification** (some deployments verify the token before forwarding, so this arrives as an `error` rather than a `401`; nothing was sent — read the message, since "expired or revoked" means re-authorize while "could not be verified right now" means retry shortly), or `statusCode` `0` = timeout / API unreachable (no HTTP response was received; for a write, retry the SAME operation passing back the returned `xRequestId`).
- `rateLimitLimit` / `rateLimitRemaining` / `rateLimitReset` / `rateLimitPolicy` relay the API's rate-limit headers **when present** (some deployments enforce limits before the request reaches the API, in which case successful calls may carry no rate-limit fields) — when `rateLimitRemaining` is available, read it to self-throttle *before* you hit a `429`. A `429` always carries `retryAfterSeconds` either way.
- `body` comes back **whole**, however long, up to the 16 MB limit below, and `bodyTruncated` is `false`. A large body is costly to read in full: when you need only part of a big list, ask for less (query filters, pagination, date ranges, or a dedicated tool). Should `bodyTruncated` ever be `true`, the server cut the body (a marker is appended at the tail) and it is not complete JSON — never reason over it as complete; narrow the request and re-fetch.
- **A body over 16 MB is not returned at all.** `body` is `null`, `statusCode` is still the API's status, and `error` says the body was over the server's size limit. Do not repeat the same request: narrow it — a smaller `pageSize` / page size, the route's filter parameters from `get-route-spec`, or a dedicated tool where one exists (e.g. `get-instruments-overview` for instrument data). For `execute-write` the request WAS sent and the API acted on it according to `statusCode` — check the outcome with a read before doing anything else.
- `xRequestId` is the id that was sent — keep it if you may need to idempotently retry this exact operation.

Report the outcome faithfully:
- **2xx** — show the response fields that matter (e.g., `orderId`, `referenceId`) and explain what happens next (e.g., a market order executes asynchronously; check the portfolio/orders-lookup route to see the resulting position).
- **400** — show the `ProblemDetails` from `body` and map it back to the schema field that failed; fix and re-confirm before retrying.
- **401** — credentials problem: the connection's `x-user-key` / `x-api-key` are wrong, or its Bearer token is invalid/expired. Tell the user to fix the headers on their MCP server entry (or re-authorize the connection); do not loop retries.
- **429** — rate limited; wait `retryAfterSeconds` before retrying. Remember a shared limit is drained by the whole endpoint group (see the route's Rate limit section), not by this endpoint alone.
- **5xx** — server-side; report it, and if retrying an order pass the SAME `xRequestId` back so idempotency protects against a double execution.

## Worked Example — "Buy 100$ of BTC"

This is a NEW open order, so it takes the Phase 0.5 pair — **no route discovery at all**:

1. Ask: demo or real account? (`account` is required with no default — recommend demo for first tries.)
2. Call `prepare-trade`:

```json
{
  "account": "demo",
  "direction": "buy",
  "symbol": "BTC",
  "amount": 100.0
}
```

   (`orderType` defaults to `"mkt"`, `leverage` to 1; `symbol` XOR `instrumentId`, and exactly one of `amount`/`units`/`contracts`.)
3. Read the `verdict`. `"rejected"` → show the user `reasons` and stop. `"ready"` → show the user the `confirmation` block — market, direction, the $100 sizing, `estimatedUnits`, the live quote, the cost lines, the balance check — plus any `warnings`, and ask for explicit approval.
4. On an explicit "yes" — promptly, the token expires in ~2 minutes — call `place-trade` with the `token`.
5. Report the `outcome`: the fills from `positions` (avgPrice, units, fees) when `executed`; the accepted-and-waiting explanation when `pending` (never place again — follow up later via `get-my-portfolio-summary` or orders-lookup); `status.errorMessage`/`decodedHint` when `rejected`.

**Fallback — server deployments predating `prepare-trade`/`place-trade`** (they come back as unknown tools). Use the old `execute-write` flow:

1. `get-all-routes` with `query: "order"` → find order creation (`createDemoOrder` / `createRealOrder` — `POST /api/v2/trading/execution/[demo/]orders`). The order identifies the instrument by **exactly one of `symbol` or `instrumentId`** (providing both is rejected), so for a well-known ticker you can pass `symbol` (e.g. `"BTC"`) **directly in the order body — no separate instrument lookup is needed**.
2. (Optional) If you specifically need the numeric `instrumentId`, look it up via `execute-read` with `path` `/api/v1/instruments/BTC` — call `get-route-spec` first to confirm its required scopes and parameters.
   (Also optional: preview the costs first — `POST /api/v2/trading/info/costs` is a read-semantics POST, so `execute-read` with the order as `body` returns the what-if fee breakdown with no confirmation needed.)
3. `get-route-spec createRealOrder` (or `createDemoOrder`) → build the body. Identify the instrument with `symbol` (or one resolved `instrumentId`, never both). Size by money uses `amount` (and `orderCurrency`), leaving `units`/`contracts` null:

```json
{
  "action": "open",
  "transaction": "buy",
  "symbol": "BTC",
  "orderType": "mkt",
  "leverage": 1,
  "amount": 100.0,
  "orderCurrency": "usd"
}
```

4. Show the user the full resolved request — path, method, body, account type — and ask for explicit approval.
5. Call `execute-write` with `method` `"POST"` and the body string (a fresh `x-request-id` GUID is generated for you); report `orderId`/`referenceId` from `body`; suggest checking the order status via the orders-lookup route or the portfolio route.

## Safety Rules (non-negotiable)

1. **Never execute a money-moving operation without showing the exact request and getting explicit user approval first** — this covers every `execute-write` call on a trading or money route, **and every `place-trade`/`place-close` call: never call either without first showing the user its `prepare-` counterpart's `confirmation` block and getting explicit approval on it**. (`execute-read` routes and `prepare-trade`/`prepare-close` themselves are exempt — they are read-only.)
2. **Never invent credentials, instrument ids, position ids, or order ids** — resolve them via the API or get them from the user.
3. **Default to the demo variant** when the user has not explicitly chosen real.
4. **One order per approval** — do not batch or loop trading operations under a single confirmation, and **never more than ONE `place-trade` or `place-close` call per approval**: a further placement/close needs a fresh `prepare-` → show → approve cycle. (The one exception is the same-token retry after outcome `unknown`, which is idempotent by construction.)
5. **Mask the user's `x-user-key` and any Bearer access token** everywhere — tool calls and requests shown to the user, summaries, error reports.
6. On ambiguous intent (e.g., "close my position" with several open positions), list the options and let the user choose.

## Raw MCP Access (no MCP client configured)

You can call the MCP server directly over HTTP (JSON-RPC). Responses arrive as a server-sent event line (`data: {...}`); the tool payload is in `result.content[0].text`.

List tools:

```bash
curl -sS -X POST https://mcp.public-api.etoro.com \
  -H "Content-Type: application/json" -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

Call a tool:

```bash
curl -sS -X POST https://mcp.public-api.etoro.com \
  -H "Content-Type: application/json" -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get-route-spec","arguments":{"routeId":"createRealOrder"}}}'
```

Execute a route (read):

```bash
curl -sS -X POST https://mcp.public-api.etoro.com \
  -H "Content-Type: application/json" -H "Accept: application/json, text/event-stream" \
  -H "x-user-key: <x-user-key>"   -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"execute-read","arguments":{"path":"/api/v1/me"}}}'
```

Partners: send the partner application's key as an `-H "x-api-key: <partner-x-api-key>"` header on the MCP call — partner routes are then included automatically.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `get-all-routes` returns an error entry about cache loading | The MCP just started and has not loaded the swagger yet | Retry in a few seconds |
| An execute tool returns `body: null` with an `error` about the response size limit | The API's answer was larger than this server's limit (16 MB for `execute-read` / `execute-write`), so it was not read — typically a list or search with a page size in the thousands | Narrow the request: a smaller `pageSize`, the route's filter parameters, or a dedicated tool (`get-instruments-overview` for instruments). Repeating the same request gets the same answer. After `execute-write`, check the outcome with a read first — the request was sent |
| Route id not found in `get-route-spec` | Route ids change with the API document | Re-run `get-all-routes` and use a current id; partial names and `<METHOD> <path>` also work |
| `get-all-routes` returns no routes and a `filterNote` saying nothing matched | The `query`/`tag` filter was too narrow — no route shares a keyword with the query, or a `tag` (matched exactly) names no group | Call `get-tags` and re-run with a tag from that menu, change the keywords, or re-run with no filters. Do **not** tell the user the operation is unsupported until you have checked the unfiltered catalog |
| `filterNote` says the listed routes are the closest, not matches | No route contains every keyword of the `query`, so the server ranked the nearest ones instead | Read the summaries: use one only if it really fits the intent. If none does, pick a tag from `get-tags` or re-run with no filters — never execute a suggested route just because it came first |
| A route the user expects is missing, and it is a partner operation (KYC, sub-accounts, cash accounts, registration, deposits) | The connection is not carrying a partner application's `x-api-key` | Ask the user to put their partner `x-api-key` on their MCP server entry — there is no tool argument to pass, and nothing to retry |
| `statusCode` 401 in the execute result | Invalid credentials on the connection — bad `x-user-key` / `x-api-key`, or an invalid/expired Bearer token | Ask the user to fix the headers on their MCP server entry, or re-authorize the connection; do not loop retries |
| The MCP call itself returns HTTP `401` with a `WWW-Authenticate` header | The connection presented no credentials and the deployment offers OAuth | The client should follow the `resource_metadata` URL and authorize; a key-pair user should add `x-user-key` / `x-api-key` headers instead |
| Execute tool returns an error about carrying BOTH an Authorization header and an x-user-key header | The connection presents both channels (the API rejects the combination with `422`) | Configure exactly one: OAuth, or the key pair — nothing was sent |
| Execute tool returns `"The MCP request carried no eToro credentials"` | The connection has no credential headers and is not OAuth-authenticated | Add `x-user-key` + `x-api-key` to the MCP server entry, or connect with OAuth. Nothing to pass as a tool argument |
| Execute tool returns an `error` saying the OAuth access token is expired, revoked or not valid | The deployment verifies the connection's Bearer token before forwarding, and this token failed — nothing was sent. Normally the server answers a dead token with HTTP `401` `invalid_token` instead, which your MCP client handles itself (it refreshes the token, or asks the user to reconnect); this in-band form only remains for a request too large to pre-check | Ask the user to re-authorize the MCP connection; do **not** loop retries. Identical in meaning to a `statusCode` 401, just caught one hop earlier |
| Execute tool returns an `error` saying the token **could not be verified right now** | The token check itself is temporarily unavailable (not a credentials problem). The request is refused rather than forwarded unverified — the server never forwards a token it could not check | Wait a few seconds and retry the same call. Do **not** tell the user to re-authorize — their credentials are fine. If it persists across several retries, the MCP server's validation dependency is down and the user should be told that |
| Execute tool returns `statusCode` `0` with an `error` about a timeout | No HTTP response was received (timeout / connectivity) — the request may or may not have reached the API | For a write, retry the SAME operation passing back the returned `xRequestId` (the idempotency key on order routes); for a read, simply retry |
| A `401`/`403`/`429` shows up in `statusCode`/`body`, not in `error` | Expected — upstream responses are relayed faithfully; `error` is reserved for tool-level failures | Interpret the status per the list in Phase 6 |
| 401 `malformed x-user-key container (Unterminated string in JSON)` | The `x-user-key` in the client configuration was truncated or altered (e.g. a trailing `_` / `__` dropped) | Re-copy it into the configuration **verbatim** — never retype, trim, reformat or line-wrap it |
| 403 Forbidden on an operation | The credentials lack the required scope | Call `get-my-profile-and-scopes` (or `GET /api/v1/me` via `execute-read` on older deployments), check the `scopes` array, and use / request credentials (keys or an access token) carrying the needed scope |
| `execute-read` returns an error saying the path is not a read-semantics POST route | The `body` argument was used on a state-changing (or unknown) route | Run state-changing routes through `execute-write` with the confirmation gate; for a GET route omit `body`; if the catalog was still loading, retry in a few seconds |
| `execute-read` comes back as an unknown tool | The server deployment predates the tool rename | Use `execute-get` for GETs and `execute-write` for read-semantics POSTs (no confirmation gate applies to those routes) |
| `get-my-portfolio-summary` comes back as an unknown tool | The server deployment predates this tool | Fall back to discovery: find the portfolio routes via `get-all-routes` and read them with `execute-read` |
| `get-my-balances` comes back as an unknown tool | The server deployment predates this tool | Fall back to `execute-read` with `path` `/api/v1/balances` and `query` `{"expand": "equityDetails", "includeSubAccounts": "true"}` — the same payload, without the `fieldGuide` and `notes` the tool adds |
| `get-my-balances` returns a `403` | The connection's credentials lack `etoro-public:money.balance:read` — a SEPARATE grant that `real:read` does not imply | Check `get-my-profile-and-scopes`, then use or request a credential carrying that scope. Do **not** conclude the data is unreachable, and do not ask for a partner scope — this is not a partner route |
| `get-my-balances` returns a `502` or `statusCode` `0` | The balance aggregator did not answer — the balance could not be READ | Tell the user the balance is currently unavailable and retry. **Never report it as a zero balance** |
| `get-instruments-overview` comes back as an unknown tool | The server deployment predates this tool | Fall back to discovery: resolve instruments, quotes and eligibility via the market-data and trading-info routes on `execute-read` |
| `get-my-profile-and-scopes` comes back as an unknown tool | The server deployment predates this tool | Fall back to `execute-read` with `path` `/api/v1/me` — the same fields, just without the `authChannel` the tool adds |
| `get-my-positions-and-orders` comes back as an unknown tool | The server deployment predates this tool | Fall back to `get-my-portfolio-summary` with `includePositions`/`includeOrders` (lighter-weight, no mirror-level or per-order execution detail), or discover the instrument-breakdown route via `get-all-routes` and read it with `execute-read` |
| `get-my-trading-history` comes back as an unknown tool | The server deployment predates this tool | Fall back to discovery: find the trade/history route via `get-all-routes` and read it with `execute-read`, paginating with its `page`/`pageSize` query parameters yourself |
| `get-my-trading-history` returns a 404 | For THIS route, HTTP 404 means a malformed request, not "no trades found" | Check minDate/page/pageSize — a 404 on this route means malformed input, not "no trades found"; an empty result set comes back as a `200` with an empty list instead |
| `get-my-portfolio-summary` returns `warnings` | Enrichment (symbols, names, logos, usernames, avatars, pending orders) degraded upstream — the portfolio data itself came back complete | Use the money numbers as-is — they are authoritative; mention the degraded fields, do not retry-loop |
| `prepare-trade` / `place-trade` comes back as an unknown tool | The server deployment predates the trading pair | Fall back to the old flow: preview costs/eligibility via the read-semantics POST routes on `execute-read`, then place the order via `execute-write` with the Phase 5 confirmation gate (see the Worked Example fallback) |
| `place-trade` returns an `error` saying the token is expired or invalid | The token's ~2-minute TTL elapsed, the token was altered, or it was minted on a different connection — nothing was sent | Re-run `prepare-trade`, re-show the FRESH `confirmation` block, and get explicit approval again — never retry-loop an expired token |
| `prepare-trade` returns `verdict: "rejected"` | At least one hard check failed (`reasons`) — no token is minted | Show the user `reasons` and fix or drop the order. Do **not** force the order through `execute-write` |
| `place-trade` returns `outcome: "pending"` | The order was ACCEPTED and is waiting (market closed, or trigger rate not reached) — not a failure | Tell the user; check later via `get-my-portfolio-summary` or the orders-lookup route on `execute-read`. Do **NOT** place again |
| `prepare-close` / `place-close` comes back as an unknown tool | The server deployment predates the close pair | Fall back to discovery: find the close-order route via `get-all-routes` and place it via `execute-write` with the Phase 5 confirmation gate |
| `place-close` returns an `error` saying the token is expired, invalid or not a valid `prepare-close` token | The token's ~2-minute TTL elapsed, it was altered, minted on a different connection, or it is a `prepare-trade` token (wrong Kind) — nothing was sent | Re-run `prepare-close`, re-show the FRESH `confirmation` block, and get explicit approval again |
| `prepare-close` returns `verdict: "rejected"` | At least one hard check failed (`reasons`) — e.g. the `positionId` was not found on this account, a copied (mirror) position was asked to close partially, or `unitsToDeduct` is invalid or exceeds the position's units | Show the user `reasons` and fix or drop the request. Do **not** force it through `execute-write` |
| `place-close` returns `outcome: "pending"` | The close was ACCEPTED and has not reached a final state yet — not a failure | Tell the user; check later via `get-my-positions-and-orders`. Do **NOT** close again |
| `place-close` returns `outcome: "unknown"` | No HTTP response was received for the close request — it may or may not have gone through, and (unlike orders) there is no way to look up a close by anything but its `orderId`, which was never received | Tell the user it could not be confirmed; the SAME token is safe to retry while still valid, or check later via `get-my-positions-and-orders` |
| `place-close` returns `outcome: "rejected"` with `errorCode` `620`/`631`/`632` | The position was already executed, already closed, or already has a pending close order | Tell the user — nothing further to retry; re-run `get-my-positions-and-orders` to see its current state |
| 400 about order size | More than one of `amount`/`units`/`contracts` set | Use exactly one sizing field |
| MCP endpoint unreachable | Wrong URL | Confirm you are using `https://mcp.public-api.etoro.com` |
