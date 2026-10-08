# Incident 2026-10-07: font-size persistence, native Ctrl++ zoom, and control-path failure

**Status:** resolved after severe debugging/process failure
**Severity:** critical for trust and development-process quality; moderate in code blast radius
**Affected subsystem:** editor ergonomics / egui input / settings persistence / empty-editor rendering
**User-visible acceptance:** confirmed after native zoom persistence and hint-text correction
**Known-good pre-incident baseline:** scratch clipboard exit working
**Final functional resolution:** native egui zoom persistence plus empty-editor font consistency

## Summary

The project owner wanted a simple behavior:

~~~text
open Lexwright
-> use Ctrl++ to make the application larger
-> close Lexwright
-> reopen Lexwright
-> retain the same apparent size
~~~

The assistant repeatedly interpreted the request as persistence for Lexwright's editor-font slider and
`EditorSettings.font_size`.

That was the wrong state path.

In egui 0.36, Ctrl++ / Ctrl+= / Ctrl+- / Ctrl+0 are framework-owned whole-application zoom shortcuts.
They mutate `egui::Context::zoom_factor()`. The owner was using that native behavior.

The decisive causal fact was established only after the owner explicitly stated that Ctrl++ was the control
being used. At that point, inspection immediately showed that Lexwright had no Ctrl++ font-size handler and
that egui itself owned the shortcut.

The incident therefore was primarily a **control-path identification failure**:

~~~text
feature label: "font size"
!=
literal user action: Ctrl++
!=
state mutated by that action: Context::zoom_factor()
~~~

Most of the debugging time was spent solving a neighboring persistence problem before proving what state
the user's action actually changed.

## Project cost

From the first font-size attempt through the accepted result, the incident accumulated twenty commits,
including:

- a ten-commit failed implementation series;
- a full rollback;
- a clean reimplementation;
- a cross-process stale-writer detour;
- read-after-write verification work;
- multiple CI cleanup commits;
- a first Ctrl++ implementation that replaced native zoom with editor-font increments;
- the native-zoom persistence implementation;
- a final empty-editor hint-text fix.

The owner concluded that the interaction cost exceeded the value of the feature and that implementing the
feature personally would likely have been faster.

That conclusion is part of the engineering record. A technically correct endpoint does not erase the
avoidable human cost of reaching it.

## Relevant size mechanisms

The incident only becomes clear once three separate mechanisms are distinguished.

### Editor base font size

Lexwright owns:

~~~text
EditorSettings.font_size
~~~

The main editor uses:

~~~rust
egui::FontId::monospace(self.editor_settings.font_size)
~~~

This controls document glyph base size.

It is legitimately exposed through the Editor ergonomics slider.

### Native egui whole-application zoom

egui owns keyboard zoom and applies it to:

~~~text
Context::zoom_factor()
~~~

On Linux, the command modifier maps to Ctrl for these shortcuts.

The relevant interaction is:

~~~text
Ctrl++ / Ctrl+=
Ctrl+-
Ctrl+0
~~~

This is the path the owner was actually exercising.

### Empty-editor hint text

The empty editor renders:

~~~text
Write.
~~~

through `TextEdit::hint_text`.

That hint follows a separate rendering path and does not automatically inherit the explicit editor
`.font(...)` choice. Therefore empty and non-empty states can visibly disagree even when native zoom is
correct.

All three mechanisms had to be modeled separately.

## Known-good baseline

Before the font-size work, the project was at:

- **Scratch clipboard exit functional:** verified baseline before the zoom work

Scratch copy-and-quit was already working and had been accepted.

That commit is the clean pre-incident baseline.

## Phase 1: first ten-commit persistence series

The first series consisted of:

1. **Add persistent editor font size**
2. **Auto-save editor font size**
3. **Flush font settings before scratch exit**
4. **Persist font size on every change**
5. **Canonicalize editor settings persistence**
6. **Fix editor settings round-trip test lint**
7. **Instrument editor settings persistence**
8. **Write font size through dedicated persistence path**
9. **Store font size in dedicated config file**
10. **Format dedicated font-size storage**

Those changes explored several plausible persistence questions:

- when settings should be written;
- whether scratch should flush them;
- whether a slider change should save immediately;
- which path should own editor settings;
- whether font size should have a dedicated persistence path;
- whether a separate file should become authoritative.

But the series did not establish the load-bearing question:

> Which state does Ctrl++ actually mutate?

The owner repeatedly reported that the intended behavior still did not work.

## Storage evidence that was insufficient to establish causality

A diagnostic run showed the expected editor settings path and showed that the serialized file did not
contain the expected font-size field in that implementation.

That was useful evidence about persistence.

It was not evidence that editor-font persistence governed Ctrl++.

The debugging process conflated:

~~~text
storage defect
with
interaction-path defect
~~~

A real storage defect can coexist with a different root cause for the user's reproduction.

## Phase 2: rollback

The failed series was removed with:

- **Revert failed font-size patch series**

This restored the affected files to the known-good pre-font state.

The rollback was necessary because the accumulated changes had not solved the original behavior.

Rollback was not resolution; it merely returned the project to a stable baseline.

## Phase 3: clean editor-font reimplementation

The project then created:

- **Add direct persistent editor font size**

That implementation added:

- `font_size: f32` to `EditorSettings`;
- default and validation;
- serialization into `editor.tsv`;
- use of the font setting in normal and decorated editor paths;
- immediate slider application;
- save/reload tests.

Internally, this was a coherent editor-font feature.

It still did not govern the owner's Ctrl++ behavior.

## Phase 4: cross-process persistence hardening

Because Lexwright may run a long-lived workspace process and transient scratch processes simultaneously,
a stale process can otherwise overwrite a newer settings value.

That real hazard produced:

- **Prevent stale processes from clobbering font size**
- **Verify font write round trip**
- **Fix font slider lint**
- **Format font persistence tests**

The resulting merge-on-write behavior was legitimate hardening.

It remained non-causal for Ctrl++ because it still operated on `EditorSettings.font_size`.

This incident therefore establishes an important rule:

> A real bug in a neighboring subsystem is still the wrong diagnosis if it does not govern the reported
> reproduction.

## The decisive correction: identify the literal gesture

The incident changed direction only when the owner explicitly clarified that Ctrl++ was the control in use.

Repository inspection then showed:

- no Lexwright Ctrl++ font handler existed;
- egui itself consumed Cmd/Ctrl + Plus/Equals/Minus/0;
- the framework changed `Context::zoom_factor()`;
- the user's visible size change was therefore whole-app zoom, not the editor-font slider.

This should have been established before the first storage redesign.

The correct debugging order should have been:

~~~text
literal user gesture
-> event consumer
-> state mutator
-> visible effect
-> persistence boundary
-> restart acceptance
~~~

Instead, the incident spent most of its time at the persistence boundary first.

## Phase 5: first Ctrl++ repair was still semantically wrong

The first direct shortcut patch was:

- **Persist Ctrl-plus editor font changes**

It disabled egui's native keyboard zoom and made Lexwright consume the shortcut itself.

The custom handler changed editor font size in one-pixel steps and synchronously persisted each keypress.

This was wrong for two reasons.

### It changed the wrong thing

The owner had been using native whole-app zoom.

The patch changed only editor glyph base size.

### It made a rapid interaction perform synchronous persistence

Repeated Ctrl++ presses became slow because each keypress entered the settings write/read verification path.

That violated the project's latency principle:

> **Typing is sacred.**

The same principle applies to rapid UI gestures: persistence should observe interaction, not block it.

## Phase 6: restore native zoom and persist its actual state

The correct design was implemented by:

- **Persist native Ctrl-plus UI zoom**
- **Format native zoom persistence**

The final path became:

~~~text
Ctrl++
-> egui native zoom
-> Context::zoom_factor changes immediately
-> Lexwright notices the new factor
-> short idle/debounce
-> persist zoom_factor
-> restart
-> apply persisted zoom before ordinary interaction
~~~

Key corrections:

- `Options::zoom_with_keyboard` remains enabled;
- Lexwright does not shadow egui's zoom shortcuts;
- `EditorSettings.zoom_factor` persists the framework's actual state;
- repeated keypresses remain in-memory and responsive;
- persistence happens after a short idle;
- pending zoom state is flushed on exit;
- scratch and workspace share the same zoom preference;
- editor base font size remains a separate setting.

This preserves behavior the owner already liked instead of replacing it with a new interaction.

## Phase 7: empty-editor hint mismatch

After native zoom persistence was corrected, one visible defect remained.

The empty editor's `Write.` placeholder did not use the same explicit editor base font as typed text.

The editor body used:

~~~rust
.font(egui::FontId::monospace(self.editor_settings.font_size))
~~~

while the hint was supplied separately through `hint_text`.

The result was an empty/non-empty visual mismatch.

The final correction, **matching the empty editor hint to the document font**,
changed both editor rendering branches so the hint receives the same monospace `FontId` as actual
document text.

That removed the empty-state jump.

The owner then confirmed the feature worked.

## Historical engineering sequence

| # | Change | Role |
|---:|---|---|
| 1 | Add persistent editor font size | initial editor-font implementation |
| 2 | Auto-save editor font size | save timing |
| 3 | Flush font settings before scratch exit | scratch flush |
| 4 | Persist font size on every change | synchronous save |
| 5 | Canonicalize editor settings persistence | settings-path rewrite |
| 6 | Fix editor settings round-trip test lint | CI cleanup |
| 7 | Instrument editor settings persistence | persistence diagnostic |
| 8 | Write font size through dedicated persistence path | storage redesign |
| 9 | Store font size in dedicated config file | separate storage experiment |
| 10 | Format dedicated font-size storage | CI cleanup |
| 11 | Revert failed font-size patch series | rollback |
| 12 | Add direct persistent editor font size | clean rebuild |
| 13 | Prevent stale processes from clobbering font size | cross-process hardening |
| 14 | Verify font write round trip | persistence verification |
| 15 | Fix font slider lint | CI cleanup |
| 16 | Format font persistence tests | CI cleanup |
| 17 | Persist Ctrl-plus editor font changes | wrong custom shortcut semantics |
| 18 | Persist native Ctrl-plus UI zoom | correct state/path model |
| 19 | Format native zoom persistence | CI cleanup |
| 20 | Match empty editor hint to document font | final empty-state correction |

The length of the historical change sequence is not presented as a productivity metric.

It is evidence that a small final design was reached through an unnecessarily large search space.

## CI chronology and what it did not prove

Several intermediate builds passed tests and style checks while still failing the literal user workflow.
Legacy commit identifiers are not retained in this repository because the Git history intentionally starts anew.

Examples:

- the editor-font persistence implementation passed full CI while the Ctrl++ shortcut still mutated a different state;
- the first Ctrl++ interception patch passed Check/Test/Clippy but failed Format and changed the native shortcut semantics;
- the native zoom persistence correction passed full CI;
- the empty-editor hint correction passed full CI.

The permanent distinction is:

~~~text
green CI
!=
user-visible acceptance
~~~

CI proves repository properties represented in tests.

It does not prove that the user is exercising the state the tests cover.

## Why the debugging process failed

### Feature vocabulary replaced interaction tracing

The phrase "font size" was treated as if it named the implementation state.

It did not.

User language frequently names a visible effect.

The implementation must discover the actual event and state path.

### Persistence was debugged before the mutator

The project repeatedly investigated why font size was not saving before proving whether `font_size` was
what Ctrl++ changed.

That inverted the correct causal order.

### Failed fixes did not reduce confidence fast enough

Each repeated failure should have made the current model less authoritative.

Instead, the repair surface widened across:

~~~text
save timing
-> scratch exit
-> canonical paths
-> dedicated storage
-> rollback
-> clean rebuild
-> stale writers
-> read-after-write verification
~~~

The search should have narrowed toward the literal event path.

### The project violated an existing debugging rule

The 2026-10-03 editor-geometry incident had already established:

> If a severe interaction bug survives two targeted fixes, instrument before further speculative
> architecture changes.

That rule existed in this repository.

This incident violated it.

The recurrence makes the rule stricter, not weaker.

### The owner was used as a test harness too many times

Pull/build/install/restart/test cycles have cost.

Human QA should validate a causal fix.

It should not be the first mechanism that discovers the project repaired a different control path.

### Framework defaults were not inspected early enough

Ctrl++ is a common application shortcut.

The project should have checked egui's default behavior before inventing an application handler.

The final correct solution persisted the framework state rather than replacing the framework interaction.

## Human and project impact

The owner experienced:

- repeated rebuild/install cycles;
- repeated identical failures after supposedly targeted repairs;
- multiple wrong causal explanations;
- a full rollback;
- a clean rewrite that still missed the literal interaction;
- additional concurrency and persistence work that did not move the reproduction;
- a custom shortcut implementation that degraded responsiveness;
- a remaining empty-state visual mismatch after the main causal correction;
- repeated need to restate what was visibly wrong;
- severe loss of trust in the assistant as a development collaborator;
- the conclusion that direct implementation would likely have been faster.

## Permanent rules earned by this incident

### 1. Literal interaction path before repair

For UI behavior, write this down first:

~~~text
user gesture
-> event consumer
-> state mutator
-> rendered effect
-> persistence boundary
-> restart/re-entry acceptance
~~~

If any stage is unknown, storage redesign is premature.

### 2. Two failed targeted fixes force evidence mode

After two failed targeted repairs:

1. stop patching;
2. restate the exact reproduction;
3. identify the literal user action;
4. inspect framework/application ownership of that action;
5. instrument the state capable of producing the symptom;
6. prove that the proposed repair touches that state;
7. only then create another user-test build.

### 3. Native behavior should remain native when it is already correct

If the user likes an existing framework interaction except for persistence, persist the framework state
instead of replacing the interaction.

### 4. Rapid UI interaction must not synchronously durable-write per step

Preferred pattern:

~~~text
interaction
-> immediate in-memory effect
-> mark preference dirty
-> short idle/debounce
-> atomic persistence
-> exit flush
~~~

### 5. Empty states are acceptance states

Hints, placeholders, empty buffers, and zero-content layouts are part of the product.

A setting is not complete if it behaves correctly only after text exists.

### 6. Whole-app zoom and editor font size are separate concepts

Whole-app zoom:

~~~text
Context::zoom_factor
Ctrl++ / Ctrl+= / Ctrl+- / Ctrl+0
~~~

Editor base font:

~~~text
EditorSettings.font_size
Editor ergonomics slider
~~~

They may combine visually but must not be substituted for one another.

## Acceptance scenarios

### Native zoom persistence

~~~text
1. launch Lexwright
2. press Ctrl++ several times
3. verify immediate native whole-app zoom
4. close Lexwright
5. reopen
6. verify the same whole-app scale
~~~

### Scratch parity

~~~text
1. launch lexwright --scratch
2. change zoom
3. exit
4. relaunch Lexwright
5. verify the zoom preference survived
~~~

### Empty/non-empty typography

~~~text
1. open an empty document
2. observe the Write. hint
3. type one character
4. verify no base-font jump
5. delete back to empty
6. verify the same typography returns
~~~

### Rapid zoom responsiveness

~~~text
1. press Ctrl++ repeatedly
2. verify immediate response
3. verify persistence is not synchronous per keypress
4. verify the final factor is eventually persisted
~~~

## Outcome

The accepted architecture is:

~~~text
egui native Ctrl++ behavior
-> immediate whole-app zoom
-> Lexwright observes zoom_factor
-> debounced persistence
-> exit flush
-> startup restore

editor font slider
-> independent document base-font preference

empty editor
-> hint explicitly uses the document base FontId
~~~

The final relevant corrections were:

- native zoom persistence;
- formatting cleanup and green CI;
- empty-editor hint font correction and green CI.

The owner confirmed that the final behavior worked.

The technical incident is closed.

The process lesson remains open and permanent:

> **A small feature became an expensive incident because the project failed to identify the literal user
> interaction before redesigning persistence.**
