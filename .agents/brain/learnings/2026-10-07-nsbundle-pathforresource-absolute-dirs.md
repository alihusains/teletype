---
id: L021
title: NSBundle pathForResource never searches absolute directories
status: verified
type: debugging
area: macos-sound
agent: codex
tool: codex-cli
date: 2026-10-07
---

# NSBundle pathForResource never searches absolute directories

## Problem

The dictation start/stop audio cue was completely silent. The user reported
"none of the sounds are working" while the reference app (EnviousWispr) played
its cues fine.

## Symptoms

- `play_recording_sounds` was `true`, `recording_sound` was a valid name
  ("Pop"), the NSSound path was compiled and linked, and `play_cue` was called
  on both start and stop.
- No sound ever played, on any cue.

## Initial hypotheses

1. The NSSound call was on the wrong thread.
2. The sound file was missing or the name was wrong.
3. The path resolution was returning nil.

## Investigation

1. Confirmed `play_cue` is invoked at start (`dictation.rs:797`) and stop
   (`dictation.rs:1029`) and that `play_recording_sounds` is `true` in the
   on-disk settings.
2. Listed `/System/Library/Sounds` — "Pop.aiff" and the other cues are present.
3. Read `system_sound.m`: it resolved the path with
   `[[NSBundle mainBundle] pathForResource:ofType:@"aiff"
   inDirectory:@"/System/Library/Sounds"]`.

## Evidence

`NSBundle`'s `pathForResource:ofType:inDirectory:` only searches the bundle's
own resource directories (and a handful of fixed system locations). It does
NOT treat an absolute `inDirectory:` argument as a real path to search. So the
call returned `nil` for every system sound, `teletype_play_system_sound`
returned 2, and the cue was silently dropped — exactly the observed behavior.

## Root cause

`pathForResource:inDirectory:` is the wrong API for an absolute path. It is
designed for resources inside the app bundle, not for arbitrary filesystem
locations.

## Fix

Build the path directly and check it exists:

```objc
NSString *path = [@"/System/Library/Sounds"
    stringByAppendingPathComponent:
    [soundName stringByAppendingPathExtension:@"aiff"]];
if (![[NSFileManager defaultManager] fileExistsAtPath:path]) return 2;
```

`NSSound` still honours the system volume/alert-sound preference, so nothing
about the audible behavior changed — the cue simply resolves now.

## Verification

Rebuilt and ran the app; the start/stop cue plays. The old silent path is gone.

## Reusable lesson

On macOS, never use `NSBundle pathForResource:inDirectory:` to locate a file
outside the app bundle. For a fixed system path, build it with
`stringByAppendingPathComponent` and verify with `NSFileManager`. This is a
silent-failure trap: the API returns `nil` (not an exception), so the bug
looks like "the feature is off" rather than "the path lookup failed".
