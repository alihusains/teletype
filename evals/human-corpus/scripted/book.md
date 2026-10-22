# Human corpus book — one file, ≈12 minutes across 2–3 takes

**Protocol (read once, then ignore):**
- Read the numbered paragraphs below, top to bottom, as one continuous take.
- Pause roughly **one second** between paragraphs (that's your segment boundary).
- Names marked fake are deliberately invented (Siobhan, Dele, Carol, Priya, Marisol);
  the work systems (SCA, TWG, SAR, Bedrock, Rovo, SwiftCon) are your real vocabulary.
- Read code like you actually read code aloud (POST, v2, lookup, 422, p95).
- Tasks 9 and 10 are NOT read: they are prompts. Answer them yourself in 2–5 sentences.
- Three full takes: **take 1 = warm-up** (you can still keep it), takes 2–3 = the measure.
- Rest your voice between takes (≥10 min). Same mic distance as normal use.

---

### 1 (warm-up)
Good morning. I have been meaning to get these notes written down, and now I have
three days of half-thoughts in one short message. I will talk a little about the
release, the plan, and where I am currently stuck.

### 2 (email)
To Carol, from siobhan dot lane at acme dot com. Subject: Q4 load forecast. The
numbers land this afternoon, so if anything is out by more than two percent,
ping me before the stakeholder review, not after, and finance needs the final
sheet in the shared drive by four o'clock.

### 3 (numbers)
By the numbers: the budget is forty eight thousand five hundred and two point
seven five, six percent over plan split across two teams. The deploy window is
the twenty eighth at half past two, with a fallback quarter-hour decision, and
the base URL must stay under seven hundred milliseconds until the ninety
fifth percentile.

### 4 (code)
In the config, set retry to false, and rename deepEqualStrict to strictEqual
only in the routes folder. The endpoint is POST /v2/users/lookup, and it
returns a 422 when the body lacks the api key. On staging, point the base URL
at https://staging-dev-ic-ai and run the two part three fixture suite before
we cut over.

### 5 (work terms)
Siobhan is blocked on the SCA mailbox again. Ask Carol to approve the TWG run
in Jira, then re-run the registry comparator in the SwiftCon sandbox, not
production. Dele should get the Confluence SAR page before the security
review, and if Rovo search is stale, hit the refresh endpoint by hand.

### 6 (self-corrections — read exactly as written, pauses and all)
The release freeze is originally moving to Wednesday the fourteenth. No wait,
scratch that. The freeze is Friday at six, not the whole of the month. The
war room is at nine thirty home time, except make it ten for anyone outside
APAC, and let's keep the bridge to thirty minutes.

### 7 (list)
Here is Thursday's list, in order. First, rotate the API tokens for the chat
gateway. Then push the two point two workflow model to Bedrock and confirm the
Nova micro embedding. And finally, post the status summary to Confluence
before the six o'clock cutoff.

### 8 (casual chat)
Quick one. Can you look at the pull request when you have a second, because
the router keeps falling back to the slow model and our p95 is spiking again.
I did not touch the fallback thresholds, so it is probably the stale index.
Oh — and check Priya's one-liner, she flagged the off-by-one, which is
probably why all sorts of people are DMing me about it.

### 9 (TASK — answer in 2–3 sentences, do not read)
Explain the change you made last week that protects the chatbot from prompt
injection style attacks, for someone who has not read the ticket.

### 10 (TASK — answer in 3–5 sentences, do not read)
Walk me through, without looking at any screen, the exact steps you followed
for the last model version cutover in production.

### 11 (dense proper nouns)
For the compliance note: expand TWG as the CLI tool, list the MCP connector,
mark SAR as the security assessment request, set GLM to five point three,
note the Nova micro embedding from the Bedrock catalog, and attach the Rovo
search snapshot next to the IC Markets load document.

### 12 (cool-down)
That is everything for now. If anything above needs a second pair of eyes
before standup, ping me in chat, and I will follow the raw notes straight
after lunch.
