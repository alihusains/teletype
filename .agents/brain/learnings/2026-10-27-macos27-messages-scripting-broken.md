# macOS 27 Messages scripting: old send script does not compile, history is not scriptable, names need Contacts

**ID:** L068
**Date:** 2026-10-27
**Agent:** Qoder (live send_message check)
**Area:** teletype-desktop / command_executor / send_message
**Status:** verified (live, on this Mac)

## Problem

The first live `send_message` check failed before any message left the machine:
the script built by `messages_script` could not compile, so the tool returned
`Err` for every send. A second self-send through a corrected script delivered,
but the evidence read still cannot work on this macOS, and spoken contact names
no longer resolve through Messages scripting. A follow-up added a Contacts
resolver, so names work again through a different AppleScript dictionary.

## Evidence

1. **The installed dictionary has no account-level participants or
   conversations.**
   `/System/Applications/Messages.app/Contents/Resources/Messages.sdef` (the
   same file `sdef` reads) exposes only four classes: `account`, `chat`,
   `file transfer`, `participant`. `account` has `id`, `description`,
   `enabled`, `connection status`, `service type` and a `chat` element. It has
   no `participants` and no `conversation` element. `chat` has `id`, `name`,
   `account`, `participants`; it has no `messages` element. There is **no
   message class at all**.

2. **The old form is a compile error.**
   `participant "<to>" of targetService` where `targetService` is an account
   fails with `-2741: syntax error: Expected end of line but found property`.
   The legacy snippet used by the W1 code cannot run on macOS 27. The related
   `every conversation of targetService` and `last message of ...` in the
   evidence script are equally uncompilable.

3. **The app-level form works.**
   `tell application "Messages" to send "text" to participant "e:<handle>"`
   returned exit 0 and the message appeared in the self conversation at
   2026-10-27 09:29 and 09:30 (screenshots of the chat list preview:
   `Teletype live probe A 1793078986`, `Teletype live check 1793079053`).
   The `e:` prefix is the email-handle form Messages shows in its participant
   IDs (`<account-uuid>:e:<address>`); `p:` is the phone form.

4. **Names do not resolve; they become handles.**
   `participant "Sara"` does not look up Contacts. It yields a participant
   whose handle is the literal lowercase string (`participant "Ali Sorathiya"`
   gave handle `ali sorathiya` on a disconnected account). A name-based send
   would therefore target a fabricated, undeliverable handle and could still
   return exit 0. A loop over the 200 app-level participants matching
   `full name`/`name` found no entry for the account's own contact name, so
   the participant list is not a reliable name index either.

5. **Connected-account selection does not help the send target.**
   Selecting the participant object on the connected iMessage account (loop
   with `service type is iMessage` and `connection status is connected`) then
   `send ... to targetBuddy` failed with
   `-1728: Can't get participant id "<uuid>:e:<address>"`. Only the direct
   `participant "<handle>"` reference is accepted.

## Root cause

macOS 27's Messages AppleScript dictionary was reduced. The W1 script was
written against the macOS 10-15 shape (account-owned participants and
conversations with messages), which no longer exists.

## Fix (2026-10-27)

- `messages_script` now emits the direct app-level form:
  `send "<text>" to participant "<to>"`, with the same AppleScript escaping.
- `looks_like_handle` accepts only an email, a `+` number, or a bare number
  (optionally prefixed `e:`/`p:`). Anything else is treated as a spoken name
  and resolved through Contacts before any Messages call.
- `resolve_contact_handle` (macOS) runs a Contacts AppleScript that dumps every
  card whose full name matches as `PERSON` / `PHONE <label> <value>` /
  `EMAIL <label> <value>` lines. `parse_contact_lookup` is pure and prefers a
  mobile/iPhone phone, then email, then any phone; phone values are normalized
  to digits with a leading `+` when present. Cards without a usable handle are
  ignored (a work email plus an empty card is a unique match); two cards with
  handles is `Ambiguous`; no match is `NotFound`; no handles is `NoHandle`.
  Each maps to a short error asking for a phone number or email.
- `evidence_failure_reason` maps `-2741`/`-1728`/`syntax error` to
  "Messages does not expose message history on this macOS" so the result is a
  short, honest `unverified` reason.
- `live_send_message_check` and `live_resolve_contact_check` are `#[ignore]`d
  tests gated on `TELETYPE_LIVE_SMS_TO` / `TELETYPE_LIVE_CONTACT`; they call
  the real executor or Contacts and never run by default. The resolver check
  sends nothing.

## Verification

- Live send regression after the Contacts change:
  `TELETYPE_LIVE_SMS_TO='e:<self>' cargo test -p teletype-desktop --lib
  live_send_message_check -- --ignored` returned
  `Ok("sent message to e:<self> (unverified: evidence read failed: Messages
  does not expose message history on this macOS)")`; the first check's two
  messages were confirmed visually in Messages.
- Live Contacts lookup (read-only, sends nothing):
  `TELETYPE_LIVE_CONTACT='Ali Husain Sorathiya' cargo test -p
  teletype-desktop --lib live_resolve_contact_check -- --ignored --nocapture`
  printed `resolved "Ali Husain Sorathiya" -> Ok("ali.sorathiya@cafu.com")`.
  The real address book has two cards under that name; one has a work email,
  the other has no handles, and the parser correctly treats that as a unique
  match.
- `cargo test -p teletype-desktop --lib` → 395 passed, 0 failed, 2 ignored.
- `cargo clippy -p teletype-desktop --lib --tests` → clean.

## Reusable lesson

Read the app's installed `.sdef` before trusting a legacy AppleScript snippet;
the `sdef` tool itself needs full Xcode, but the file lives in
`Contents/Resources`. Element ownership changed on macOS 27: participants are
application-level, accounts have no participants, and message history is not
scriptable at all, so AppleScript can send but cannot verify. Never construct
a participant from a contact name; it creates a handle, not a lookup. Resolve
names through the Contacts dictionary instead, and keep the preference and
ambiguity rules in pure Rust so they are testable without the address book.

## Linked

- Decision D036 (evidence-based command execution)
- L067 (W1 implementation learnings)
- Follow-up implemented 2026-10-27: `resolve_contact_handle` + pure
  `parse_contact_lookup` restore "text Sara" commands through Contacts,
  live-verified against the real address book.
