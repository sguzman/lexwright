# Incident 2026-10-03: editor geometry jitter and EOF crash

**Status:** resolved  
**Severity:** high for the project stage  
**Affected subsystem:** core editor  
**Primary resolution:** `bd76f4b` — `Decouple editor width from dynamic status bar`  
**Secondary resolution:** `a15c992` — `Clamp ASCII cursor indices at EOF`

## Summary

Lexwright's core editor became violently unstable while typing. Normal edits, especially around spaces and wrapping, repeatedly caused the entire paragraph to reflow left/right. The effect looked like a renderer, Harper, wrapping, autosave, or asynchronous-analysis problem.

It was not.

The primary defect was that **dynamic status-bar text was changing the width of the parent egui `Ui`**, and Lexwright was deriving the editor width from that already-mutated parent layout. The editor therefore changed width while the application window itself had not changed width.

The trace captured the editor jumping from approximately **946 px to 982.5 px** and later returning to 946 px. Every such geometry transition forced a full text rewrap. This was the "violent jerk."

A second independent defect was discovered immediately afterward: Lexwright's optimized ASCII character-to-byte fast path failed to preserve egui/String `TextBuffer` semantics at end-of-file. egui may transiently request a character index one past the current end; stock behavior clamps to `text.len()`. Lexwright returned the raw index. In release mode that produced an out-of-bounds slice and a crash when Delete/Insert-style editing requested byte 535 from a 534-byte ledger.

Both defects were introduced by Lexwright code.

## User impact

This incident was especially damaging because the editor is the load-bearing component of Lexwright.

Observed effects included:

- violent whole-paragraph horizontal/line-wrap movement while typing
- the same visible failure persisting with Harper, structure, and morphology disabled
- repeated false confidence that a fix had solved the problem
- hours of user time spent pulling builds and re-testing essentially the same defect
- loss of confidence in continued development
- a separate crash after pressing Delete/Insert
- substantial engineering time spent modifying unrelated subsystems before measuring the actual geometry

The user repeatedly reported that the failure was unchanged. Those reports were correct. The debugging process did not respond to that evidence quickly enough.

## Primary root cause: status-bar text controlled editor geometry

The top bar contained dynamic strings such as:

```text
words 90
```

and, while the background analyzer was stale:

```text
words 90 · analyzing
```

The editor and status bar shared a parent egui `Ui`.

Lexwright rendered the status row first and only afterward asked that parent UI how much width remained / was available for the editor. In egui, oversized horizontal content can expand the parent UI's maximum rectangle. The longer analysis-status string therefore changed the width subsequently offered to the editor.

The resulting loop was approximately:

```text
user types
  -> document revision becomes newer than analysis revision
  -> status text grows: "words N" -> "words N · analyzing"
  -> parent Ui max rectangle grows
  -> editor receives a wider viewport
  -> paragraph rewraps

~160 ms later
  -> analysis catches up
  -> status text shrinks: "words N · analyzing" -> "words N"
  -> parent Ui width contracts
  -> editor receives the old narrower viewport
  -> paragraph rewraps again
```

The editor was therefore being resized by **analysis status text**, not by the application window.

### Trace evidence

The geometry recorder finally made the defect undeniable.

Representative frames showed:

```text
editor width: 946.0 px
wrap width:   938.0 px

then:

editor width: 982.5 px
wrap width:   975.0 px
```

Later the values returned toward:

```text
editor width: 946.0 px
wrap width:   938.0 px
```

The window had not been resized. A ~36.5 px editor-width swing is more than enough to change line breaks throughout a paragraph.

This was the first piece of evidence that directly measured the property producing the visual failure.

## Primary fix

Commit `bd76f4b` decoupled editor geometry from dynamic status text.

The editor viewport width is now captured **before the status bar is rendered**. That width is then treated as an editor invariant for the frame.

Additionally, the visible status label no longer changes from:

```text
words N
```

to:

```text
words N · analyzing
```

The visible label remains geometry-stable. Stale/current analysis state belongs in the hover tooltip instead.

### Permanent rule

**Status, telemetry, notifications, badges, save state, analyzer state, and other ancillary UI are not allowed to determine editor geometry.**

A status surface may change text, color, icon, or tooltip without moving the writing surface.

If a future top-bar feature can change editor width merely by changing its label, the design is wrong.

## Secondary root cause: ASCII fast path violated upstream TextBuffer semantics

Lexwright added an O(1) index path for ASCII-only English.

For ASCII, character index and byte index are numerically equal, so the implementation originally returned:

```rust
char_index.0
```

directly.

That was incomplete.

egui's normal String/TextBuffer implementation clamps character indices at EOF. Editor cursor/delete machinery can legitimately ask for one past the current character count during intermediate editing state.

The optimized Lexwright path did not clamp.

The observed crash attempted to index byte **535** in a string with length **534**.

### Secondary fix

Commit `a15c992` changed the ASCII fast path to preserve stock semantics:

```rust
char_index.0.min(self.text.len())
```

A regression test now deliberately exercises one-past-end insertion/deletion behavior.

### Permanent rule

**An optimization must preserve the behavioral contract of the implementation it replaces, including edge cases that are not obvious from the happy path.**

"ASCII index == byte index" was numerically true but semantically incomplete.

Performance work does not receive permission to weaken correctness.

## Why diagnosis took far too long

The largest failure in this incident was not the original UI bug. It was the debugging process.

The project spent too long treating plausible hypotheses as diagnoses.

The failure was repeatedly attributed to:

- Harper taking too long
- Harper reprocessing too much text
- Harper diagnostics appearing/disappearing
- custom TextEdit layouters
- word wrapping being enabled/disabled
- stale analysis revisions
- underline painting
- layout-vs-paint coupling
- finite vs infinite desired width
- stock vs custom egui sizing behavior
- save/analysis timing

Several of those areas did contain real bugs or weaknesses and were improved, but **none explained the continuing primary jerk**.

The user's repeated statement that the problem was "still just as fucked" was high-value evidence: fixes that allegedly targeted the root cause were not changing the symptom.

The correct response should have been instrumentation much earlier.

## Failed / insufficient fix sequence

The following commits are part of the debugging history and should remain useful archaeological markers:

- `f30d3a7` — underline Harper diagnostics and attempt to stabilize layout
- `75f1628` — make viewport wrapping a permanent editor invariant
- `99e9408` — make Harper incremental and layout-independent
- `b3b9a98` — preserve Harper diagnostics across live edits
- `0a74e6d` / `1d75d7d` / `d288eb4` — stabilize/rebase live Harper rendering
- `51f3f6a` — restore stock egui layout for normal writing
- `2584e1e` — restore known-good add_sized editor geometry
- `d048c7c` — use finite viewport width for TextEdit
- `f6e0d1a` / `d352536` — add and format editor geometry tracing
- `bd76f4b` — **actual primary fix: decouple editor width from dynamic status bar**
- `a15c992` — **secondary fix: clamp ASCII cursor indices at EOF**

The important lesson is not that the intermediate work was valueless. The important lesson is that **passing CI and improving plausible subsystems did not establish that the user-visible bug had been fixed**.

## Instrumentation that broke the incident open

The editor geometry recorder writes to:

```text
$XDG_STATE_HOME/lexwright/editor-trace.tsv
```

or:

```text
~/.local/state/lexwright/editor-trace.tsv
```

It records edit-adjacent frames with fields including:

- editor rectangle position and size
- galley position and size
- wrap width
- row count
- cursor index / row / column
- document revision
- queued save revision
- saved revision
- analysis revision
- Harper revision
- expansion hit count

The recorder exists because subjective reports such as "the whole paragraph violently jerks" are real observations but do not identify which internal quantity moved.

The trace converted the symptom into:

> editor width changes by ~36.5 px without a window resize.

At that point the search space collapsed.

## New debugging protocol

For load-bearing editor failures, use the following escalation policy.

### 1. First reproduction: inspect the narrowest plausible path

A single obvious bug may receive a targeted fix.

### 2. First failed fix: reassess the hypothesis

Do not merely add another patch to the same theory.

### 3. Second failed fix: instrument before further architecture changes

If the same externally visible failure survives two targeted fixes, stop speculative modification.

Record the state that could physically produce the symptom.

For geometry/reflow bugs, record geometry.  
For latency bugs, record timing and queue depth.  
For text corruption, record exact mutation deltas and revisions.  
For persistence bugs, record save/snapshot generations.

### 4. Do not declare resolution from compilation or CI

`cargo check`, tests, Clippy, and rustfmt establish code health. They do not establish that a reported interaction bug is gone.

Resolution requires the original user-visible reproduction to stop occurring.

### 5. Preserve the diagnostic instrument after resolution

Do not immediately delete a recorder that found a severe editor defect. Keep it until the surrounding architecture has survived further feature work.

## Editor geometry invariants established by this incident

1. **The editor viewport is load-bearing state.** Ancillary UI does not control it.
2. **Dynamic status strings must not change wrap width.**
3. **Editor width must be derived from stable container geometry, not from a parent rectangle already mutated by siblings.**
4. **Background analyzer state may change paint or metadata, never writing-surface geometry.**
5. **The top bar must be treated as a hostile geometry surface unless explicitly isolated.**
6. **A stable application window must imply a stable editor width unless the user explicitly changes layout.**
7. **Any unexplained editor-width change during ordinary typing is a bug and should be visible in telemetry.**

## Optimization invariants established by the crash

1. Fast paths must preserve upstream API semantics, not merely common-case arithmetic.
2. Release-mode behavior must not rely on `debug_assert!` for safety.
3. Character/byte optimizations require boundary tests at:
   - index 0
   - EOF
   - one past EOF when the upstream API permits/clamps it
   - non-ASCII fallback
   - insert
   - delete
   - replace
4. A performance optimization that can crash the editor is a failed optimization.

## Process failures to remember

This incident was embarrassing because the project owner was repeatedly asked to validate fixes that had not been demonstrated to affect the actual failing quantity.

Specific process mistakes:

- declaring fixes too confidently before user QA
- changing multiple interacting layout/analyzer mechanisms without evidence
- allowing the debugging story to drift toward whichever subsystem had most recently changed
- treating "async" architecture as relevant to a geometry symptom without measuring geometry
- not instrumenting after repeated identical failure reports
- restoring "known-good" code without checking all surrounding geometry inputs
- focusing on editor internals while a sibling status widget was physically changing the editor's allocation
- optimizing the TextBuffer without first matching the full stock edge-case contract

These are engineering mistakes, not user error.

## Required review checklist for future UI changes

Before merging a top-bar/status change:

- Can the visible label width change during typing?
- If yes, can that change any parent max/min rectangle?
- Is editor viewport width captured independently?
- Does toggling stale/current/save/error state change editor wrap width?
- Is the state available in a tooltip instead of geometry-changing text?

Before merging an editor fast path:

- What exact upstream behavior is being replaced?
- What happens at EOF and one-past EOF?
- Does release mode remain safe without debug assertions?
- Are there regression tests for boundary behavior?

Before claiming a severe editor bug is resolved:

- Was the original reproduction performed?
- Did the measured failing quantity stop changing?
- If not measured, why is the diagnosis considered established?

## Outcome

After `bd76f4b`, the editor stopped jerking.

After `a15c992`, the Delete/Insert EOF crash stopped.

The user confirmed the combined result:

- no jerks
- no crash
- no width jerk
- editor looked good

That confirmation, not CI alone, closes the incident.
