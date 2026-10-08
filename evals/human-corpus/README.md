# Human-recorded eval corpus

Purpose: produce the first accuracy/latency numbers Teletype can quote for
**real human speech**. The existing `evals/corpus/` is `say`-synthesised and
only validates the harness plumbing (the README says so explicitly).

## What to record

20 utterances, one file each, spoken **naturally** — not read like a
teleprompter. Natural pace, real pauses, and for the filler cases, real
"um"s. That is the point: we are measuring how the pipeline handles a human.

- File: `human-01.wav` … `human-20.wav` in this directory
- Format: 16 kHz mono WAV (the harness resamples otherwise, but matching the
  live path makes the numbers cleaner)
- Length: a few seconds to ~30 s per file; `human-18` is deliberately long

## Recording on this Mac

Use the built-in Voice Memos app (or `afrecord` in a terminal):

```bash
# afrecord: 16 kHz mono WAV, exactly what the live dictation path uses
afrecord human-01.wav -d 0.1 -f WAVE -d LEI16@16000 -c 1
# speak, then Ctrl+C to stop
```

Or record in Voice Memos and export: Voice Memos exports AAC/m4a, so convert:

```bash
afconvert -f WAVE -d LEI16@16000 -c 1 input.m4a human-01.wav
```

## The 20 lines to speak

Speak each line once, naturally, into the microphone. The reference text
below is **what the words should be** — when you are done recording, write
down any word you actually said differently and we will correct the manifest
to match what you *meant* (the reference is the intended text, lowercase,
no trailing period — same convention as the synthetic corpus).

| # | File | Tag | Line to speak |
|---|---|---|---|
| 1 | human-01.wav | plain | The deploy went out this morning and everything is working fine |
| 2 | human-02.wav | plain | Can you pick up the lunch order for the team from the cafe downstairs |
| 3 | human-03.wav | plain | The meeting was moved to Thursday at three in the afternoon |
| 4 | human-04.wav | filler | Um the thing is um I think we should just ship it and uh fix the bugs later |
| 5 | human-05.wav | filler | So basically I mean the whole point is actually that we need more time to test it properly |
| 6 | human-06.wav | filler | Er so like um the standup is at nine and I really need to be there so I will be late for the other one |
| 7 | human-07.wav | list | Things to do today are fix the login bug, update the docs, and review the pull request |
| 8 | human-08.wav | list | The steps are first clone the repo, second run the setup script, and third start the dev server |
| 9 | human-09.wav | numbers | The release is scheduled for the fourteenth of March at two thirty in the afternoon |
| 10 | human-10.wav | numbers | Our error budget is ninety nine point nine five percent and we have used up forty two percent of it |
| 11 | human-11.wav | names | Ask Priya Sharma about the Kafka consumer in the payments service |
| 12 | human-12.wav | names | The S3 bucket for the ETL pipeline is in the us east one region |
| 13 | human-13.wav | question | Did the migration finish before the maintenance window closed |
| 14 | human-14.wav | multi | The build failed on CI. The error is in the auth module. I will take a look at it after lunch |
| 15 | human-15.wav | awkward | The semicolon in the SQL query is the one that is causing the timeout |
| 16 | human-16.wav | awkward | We need to upgrade from version two to version three before the end of the quarter |
| 17 | human-17.wav | email | Hey Sam, the contract is ready for review, please sign it before the end of the day and send it back to legal |
| 18 | human-18.wav | long | The quarterly review is on the fourteenth and we need to cover the launch of the mobile app, the results of the customer research, the state of the platform migration, the hiring plan for the second half of the year, and the budget proposal for next year. I will send the agenda by Friday and everyone should come prepared with a short update on their area. Please keep your updates under five minutes so we have time for discussion |
| 19 | human-19.wav | casual | Oh wait no never mind about that, actually just forget the whole thing and we can talk about it tomorrow |
| 20 | human-20.wav | casual | That is a great idea, let us do it that way and I will send the details over in a bit |

## Tag coverage (why these 20)

- **plain** (1-3): baseline accuracy on clean speech
- **filler** (4-6): filler-word removal is a core pipeline feature — this is
  where Teletype earns its keep
- **list** (7-8): spoken-list detection + bullet rendering
- **numbers** (9-10): ITN (inverse text normalization) — numbers, dates,
  percentages — this is the weakest measured area (ITN 2b still open)
- **names** (11-12): proper nouns and acronyms (Priya Sharma, Kafka, S3, ETL)
- **question** (13): question mark restoration
- **multi** (14): multi-sentence with pauses
- **awkward** (15-16): punctuation-heavy phrasing that confuses ASR
- **email** (17): a realistic dictation use case (email body)
- **long** (18): ~30 s sustained speech, tests drift over a longer take
- **casual** (19-20): conversational, self-correction, no clear topic

## After recording

1. Drop the 20 `.wav` files here.
2. We add 20 lines to `evals/corpus/manifest.jsonl` with `"human": true`
   (or keep a separate `evals/human-corpus/manifest.jsonl` and run the
   harness against it — decide when the files arrive).
3. Run measure mode:

   ```bash
   cargo run -p teletype-eval --bin eval -- \
       --corpus evals/human-corpus \
       --asr parakeet \
       --model <path-to-parakeet-model> \
       --report evals/human-report.md \
       --json evals/human-report.json \
       --thresholds evals/thresholds.json
   ```

4. The report's human-case WER/latency is the first number we can quote
   publicly.
