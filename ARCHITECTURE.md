# Lexwright architecture

Lexwright's primary product is now a **disposable scratchpad with persistent expansion rules**. The former durable English editor, NLP instrumentation, and document subsystem remain available through the explicit `--editor` mode. Sections covering ledgers, tabs, diagnostics, and document analysis below describe **secondary editor infrastructure**; they do not describe the default scratch workflow.

## Default scratch contract

- Zero CLI arguments mean scratch **and** overlay; `--no-overlay` opts out of Hyprland floating dispatch.
- Scratch starts with an empty `DocumentState::scratch()`. Its `LedgerStore::ephemeral` does not own a save worker, has no disk-load path, and rejects save requests.
- Expansion rules/configuration are persistent independently of scratch text. The default scratch ruleset is `scratch`; the durable globally active workspace ruleset is not silently switched.
- Harper is opt-in even in scratch; structure/morphology/inspection UI is not part of the default scratch bar.
- **Unmodified Enter**, when the scratch editor has focus, copies nonblank text via `wl-copy` and exits on successful handoff. Empty or whitespace-only text exits without clipboard access. **Shift+Enter** inserts a newline. **Escape** closes scratch from any focused control without copying. **Ctrl+J** is not a Lexwright copy shortcut.
- If clipboard handoff fails, keep the buffer and show an error. A successful handoff permits exit. Never persist scratch text in a recovery artifact.
- The legacy editor is opened explicitly with `--editor`; `--tab NAME` implies editor mode for backward compatibility.
- The global settings record stores **editor font size** and **interface font size** independently. Interface text styles include Body, Button, Small, Monospace, and Heading, so rules and settings inputs scale with labels. Native egui zoom remains a separate dimension.
- Normal scratch chrome stays minimal: ephemeral indicator, copy action, expansion/rules control, appearance, and visible launch/copy errors.

Lexwright is a writing instrument first and a language laboratory second.

This document is intentionally opinionated. Features that violate these invariants should be redesigned rather than allowed to accrete latency into the editor.

## 1. The hot path

The hot path is:

```text
OS input -> egui input -> editor buffer mutation -> paint
```

The hot path must not perform:

- filesystem I/O
- network I/O
- grammar checking
- spelling analysis
- tokenization of the whole document
- POS tagging
- morphology
- syntax parsing
- corpus lookup
- indexing
- serialization
- work proportional to the whole document merely because one key was pressed

The current editor storage is a contiguous Rust `String`, but Lexwright no longer delegates every insertion back through egui's generic String insertion helper.

For an ASCII-only ledger, an egui character index is exactly the same number as the UTF-8 byte index. Lexwright tracks that invariant and uses the index directly in O(1). This is the expected fast path for ordinary English typing.

If any non-ASCII text enters the document, Lexwright switches conservatively to UTF-8-aware index conversion. It does **not** rescan the whole document after deletions merely to see whether the final non-ASCII character disappeared.

The contiguous String remains a bootstrap limitation: middle-of-document insertion still shifts trailing bytes. A real rope / piece table / gap-buffer architecture cannot be honestly obtained while stock egui `TextEdit` requires the whole document as a contiguous `&str`. The eventual large-document solution therefore includes a Lexwright-owned editor surface rather than repeatedly flattening a non-contiguous buffer.

## 2. Persistence (explicit legacy editor mode)

In `--editor` mode, the ledger is durable user data. This section does **not** apply to scratch mode.

Current policy:

- default path: `$XDG_DATA_HOME/lexwright/ledger.txt`
- fallback: `~/.local/share/lexwright/ledger.txt`
- optional override: `LEXWRIGHT_LEDGER`
- saves happen on a dedicated worker thread
- an edit schedules a snapshot after 160 ms of idle time
- each snapshot is written to a temporary file, synced, and atomically renamed
- shutdown queues the newest revision and waits for the save queue to flush

The UI thread never performs the disk write.

The current snapshot operation still copies the document on the UI thread, so its duration is measured explicitly. Snapshots are cached by document revision. With Harper enabled, a snapshot may be created after Harper's shorter idle delay; if no further edit occurs, persistence and the lightweight analyzer later reuse that exact `Arc<str>` instead of cloning the ledger again. A future buffer/storage design must remove even this document-sized snapshot copy once measurements show it matters.

## 3. Programmable input

Text expansion is special because it intentionally changes canonical text while the user is typing, so it is the only linguistic layer currently allowed to run synchronously.

Rules are compiled into a **reversed trie**. On an activation character such as a space or punctuation, matching walks backward from the insertion point only while a trie branch exists. It does not regex-scan or search the document. User rules may opt into one additional compiled alias: first trigger letter uppercase with the remainder lowercase. That alias maps back to the canonical rule for telemetry and capitalizes only the first replacement letter; arbitrary case-insensitive matching is deliberately not enabled.

Current invariants:

- no regex engine in the keystroke path
- no heap allocation for a successful/failed suffix lookup
- no full-document scan by the expansion matcher
- expansion only activates on a single delimiter insertion
- a trigger must start at a word boundary
- the synchronous expansion layer only accepts replacements at least as long as their triggers, allowing the editor buffer to report the correct forward cursor advance immediately
- shortening and arbitrary rewrites belong in a later transformation layer with explicit cursor/state handling

Starter rules are compiled into the binary. User rules are stored in `$XDG_CONFIG_HOME/lexwright/expansions.tsv` (or `~/.config/lexwright/expansions.tsv`) and override starter rules by trigger.

The in-app rule editor works on a detached draft. Editing the draft does not rebuild or mutate the live trie. Each user rule also carries a durable per-rule capitalized-form flag; legacy two-column rule rows default that flag to false, while new saves use a third boolean column. An explicit **Save & activate** action validates the entire active ruleset, writes the complete configuration atomically, then rebuilds the trie. This keeps configuration work completely outside the normal typing hot path.

The config also persists whether the starter set is enabled. Turning starter rules off gives the user a blank expansion language without deleting their custom rules.

Expansion configuration supports multiple named rulesets, but exactly one set is active. The active set alone is compiled into the reversed trie. Set creation, deletion, switching, validation, persistence, and trie rebuilds occur only through explicit rule-window actions; none of that work enters ordinary typing. Legacy flat `expansions.tsv` files are parsed as a single ruleset named `default` and migrate naturally when next saved. Rulesets also carry one-line durable notes. Rename is an explicit metadata operation that preserves the active ruleset's runtime telemetry by moving those counters to the new profile name.

Per-ruleset compression telemetry is runtime-only and intentionally cheap: counters are updated only after a successful trie match. Ordinary keystrokes that do not expand pay no ruleset-statistics work. Per-trigger keys are allocated only on the first successful hit for that trigger; repeated hits update existing counters. The counters measure hits, trigger characters, produced replacement characters, derived avoided characters, and typed/output percentage. The same successful expansion also updates a per-trigger counter for the active ruleset, so individual user rules can be ranked by actual use and character savings. The rules window exposes all configured sets in one comparison grid, including zero-hit baselines. Saving a changed ruleset resets only that set's runtime counters, because measurements from two different rule definitions are not a valid single experiment. The active set can also be reset manually without changing rules. These are experimental feedback, not durable user data. A session report may be exported explicitly as Markdown under the XDG state directory. The report is derived from in-memory telemetry and the active ruleset definition; report generation and filesystem I/O are never part of the editor hot path.

## 4. Measurement

Lexwright now records lightweight timing metrics for the actual code it controls:

- process start -> first UI frame
- frame CPU duration
- editor insertion/mutation duration
- expansion-trie lookup duration
- ASCII O(1) vs UTF-8 fallback index counts
- autosave snapshot-clone duration
- background atomic-save duration

The top bar exposes the latest edit timing and a hover tooltip exposes last / average / maximum values.

These are **CPU-side instrumentation points**, not fabricated end-to-end latency. They do not include keyboard hardware scan time, compositor scheduling, display scanout, or pixel response.

Instrumentation itself must remain cheap. The timing structure is fixed-size and updates without allocation; UI string formatting happens during normal UI construction, outside the buffer mutation itself.

## 5. Analysis model

The analysis lane is now live.

The lightweight structure/morphology analyzer follows the 160 ms persistence idle boundary. Harper has its own shorter 45 ms idle boundary so spelling feedback does not inherit disk-save latency. Both obtain the same revision-cached immutable `Arc<str>` when possible; a new edit invalidates that cache immediately.

If an analyzer falls behind, its queued jobs are collapsed independently to the newest waiting revision before the next pass. Stale work is observationally useless and must never become backpressure on typing.

Expansion-candidate mining uses the lightweight analysis worker but is **request-gated**. Normal revision analysis does not build n-gram frequency tables. Opening the candidates surface queues the current immutable snapshot with candidate mining enabled; while that surface remains open, subsequent lightweight analysis requests retain mining for new revisions. Candidate results are revision-tagged, bounded to repeated 1-4 word patterns, ranked by theoretical savings, and observational only. Promotion crosses into the existing expansion-rule draft boundary and still requires explicit Save & activate.

Harper is deliberately isolated on its own worker and disabled by default. Its dictionary and curated linter are constructed lazily only after the user enables Harper or explicitly requests a pass. The first request performs a full lint. Subsequent requests compute the changed byte range against the last completed snapshot, expand it into bounded sentence context, retain diagnostics in untouched regions, shift later untouched spans by the byte/character delta, and lint only that local window. Separately, `EditorBuffer` records exact mutation deltas as edits happen. The UI uses those deltas to keep unaffected visible diagnostics aligned immediately while invalidating only a local neighborhood until Harper catches up. Dictionary-backed grammar work therefore cannot extend startup latency, block typing, or force a full-document grammar pass after every keystroke.

The analyzer now reports mechanical counts plus revision-tagged lexical spans. Closed-class English words can be classified directly from small explicit lexicons. Open-class guesses are intentionally named `*-like` because the first pass uses conservative lexical/suffix heuristics rather than pretending to be a statistical POS tagger.

The ordinary writing path uses egui's stock multiline `TextEdit` layouter with no Lexwright layout override. Harper is intentionally excluded from the `LayoutJob`: after `TextEdit::show` returns the final galley, Harper underlines are painted as clipped line segments using egui's own character-cursor -> layout-cursor conversion. Lexwright does not infer document character offsets from glyph counts.

Structure and morphology are explicit visual modes and require colored layout sections. Only while one of those modes is enabled does Lexwright install its custom layouter. That layouter mirrors egui 0.36.2's stock editor geometry contract: the callback wrap width is used directly, trailing whitespace is preserved, and each section uses the same computed line height as stock TextEdit. Analyzer freshness can change paint data inside that mode, but it does not switch the layout implementation while the mode remains enabled.

Future systems plug into the same worker boundary:

```text
revision N
  -> tokenizer
  -> morphology
  -> POS
  -> grammar
  -> diagnostics(revision N)
```

The UI discards or visibly marks stale results rather than blocking for a current answer.

No analyzer owns the canonical text. An analyzer never mutates it directly. Analyzer-proposed changes must cross the canonical editor-mutation boundary, carry the revision they were derived from, and fail closed if that revision is stale.

### External editor mutations

Diagnostics and future transformations may propose edits, but they cannot call `String::replace_range` arbitrarily from UI code.

The first canonical external-edit path is used by Harper suggestions. It:

- requires the diagnostic revision to equal the live ledger revision
- validates byte-range bounds and UTF-8 character boundaries
- captures the existing egui cursor and text state as an explicit undo point
- applies the edit through `EditorBuffer::replace_byte_range`
- stores a deterministic post-edit cursor
- increments the normal Lexwright revision and therefore reuses autosave and analyzer invalidation

This makes programmatic fixes participate in Ctrl+Z instead of becoming invisible mutations outside the editor's history.

### Morphology

The same background pass now emits non-overlapping prefix/stem/suffix byte spans for words that match a deliberately conservative surface-affix inventory. Up to two prefix and two suffix layers can be represented.

The colored spans remain orthographic segmentation: the middle span is a **surface stem**, not canonical text. Affix recognition is evidence-gated: matching edge letters is insufficient. A candidate suffix must leave a supported lexical base or participate in a supported stacked derivation, and prefix stripping must likewise expose a supported base after relevant spelling normalization. This prevents accidental segmentations such as `re + ally`, `re + consid + er`, and `de + cision + s`.

A separate lexeme-candidate layer may normalize only when a conservative spelling rule applies. Initial rules handle `i -> y` before suffixes such as `-ness`, undoubling selected final consonants before inflectional suffixes, and restoring final `e` when the restored form is a supported base.

Each lexeme candidate stores both the original word/stem byte ranges and the derived string. This preserves provenance: the UI can show exactly which document bytes produced a candidate without rewriting or pretending those normalized bytes exist in the ledger.

Obvious lexical exceptions are filtered before segmentation when a productive-looking suffix is actually part of the lexical base (for example `something`).

Morphology visualization is mutually exclusive with the lexical-category overlay and remains opt-in, preserving the normal editor path. Lexeme inspection is observational and uses the same revision gate.


## 6. Failure isolation

If a future subsystem fails:

- grammar dies -> typing continues
- POS tagger dies -> typing continues
- morphology dies -> typing continues
- an expansion config is invalid -> starter rules remain available and the error becomes visible
- save fails -> typing continues and the failure becomes visible
- network is unavailable -> core editing is unaffected

The editor is the load-bearing system. Everything else is optional machinery around it.

## 7. Dependency policy

Keep the core dependency graph small.

A dependency belongs on the editor hot path only if its value clearly exceeds its latency, binary-size, startup, and maintenance cost. Heavy NLP libraries should live behind optional modules or worker boundaries. `harper-core` is the first concrete example: the dependency is pinned, its default optional feature set is disabled, and its runtime engine is owned entirely by an opt-in background worker.

The initial renderer is `glow`, selected intentionally to keep the native stack smaller than the default wgpu path. eframe/egui remain replaceable implementation choices; the editor and linguistic model should not become inseparable from GUI widgets.


## 8. Editor geometry isolation

The editor rectangle and wrap width are load-bearing state.

A severe 2026-10-03 incident demonstrated that dynamic top-bar text could expand egui's parent `Ui` and therefore change the editor width even though the application window had not changed size. Geometry tracing captured the editor oscillating from roughly 946 px to 982.5 px and back, forcing whole-document rewraps.

Permanent invariants:

- editor width is captured from stable container geometry before dynamic status surfaces are rendered
- status, telemetry, save state, analyzer freshness, badges, errors, and notifications do not determine editor width
- stale/current analysis state belongs in tooltips or geometry-stable controls rather than labels whose changing width can resize the writing surface
- a stable application window implies a stable editor wrap width unless the user explicitly changes layout
- any unexplained editor-width change during typing is a bug
- Harper is post-paint only in normal writing mode and has no authority over text shaping or wrapping
- structure/morphology custom layout must mirror the stock TextEdit geometry contract and must not switch layout implementation merely because an analysis result becomes stale/current

The detailed postmortem is retained at `docs/incidents/2026-10-03-editor-geometry-jitter.md`.

## 9. Optimization equivalence

A fast path is allowed only when it preserves the complete behavioral contract of the path it replaces.

The 2026-10-03 EOF crash was caused by an ASCII O(1) character-to-byte optimization returning the numeric character index directly. While the arithmetic is valid for ASCII, stock egui/String `TextBuffer` behavior also clamps transient one-past-EOF indices to `text.len()`. The optimized path omitted that semantic behavior and crashed in release mode.

Permanent invariants:

- optimizations must match upstream semantics, not only common-case values
- safety must never depend on `debug_assert!`
- EOF and one-past-EOF behavior must be tested explicitly when replacing editor indexing code
- non-ASCII fallback behavior must remain covered
- insert/delete/replace boundary tests are required for index fast paths
- a performance optimization that weakens correctness is a failed optimization

## 10. Severe-editor-bug debugging protocol

The editor is load-bearing. Repeated speculative fixes are more damaging here than slower evidence gathering.

Escalation policy:

1. One clear reproduction may receive one narrow targeted fix.
2. If that fix fails, reassess the hypothesis rather than layering another patch onto it.
3. If the same externally visible editor failure survives a second targeted fix, instrument before changing architecture again.
4. Record the physical quantity capable of producing the symptom:
   - geometry/reflow -> editor rect, galley rect, wrap width, row count, cursor layout
   - latency -> timings, queue depth, revision transitions
   - corruption -> exact mutation deltas and revisions
   - persistence -> snapshot/save generations
5. CI success is not user-visible resolution.
6. Do not call a severe interaction bug fixed until the original reproduction stops occurring.
7. Keep a successful diagnostic recorder available through subsequent feature work.

The editor geometry recorder writes edit-adjacent frames to `$XDG_STATE_HOME/lexwright/editor-trace.tsv` or `~/.local/state/lexwright/editor-trace.tsv`.


## 11. Editor ergonomics and Vim-lite

Editor ergonomics may customize interaction, but they do not receive permission to destabilize the writing surface.

Cursor configuration is stored separately from document data in `$XDG_CONFIG_HOME/lexwright/editor.tsv` (or `~/.config/lexwright/editor.tsv`). Cursor width and blink timing are applied through egui's native `TextCursorStyle`. Lexwright does not custom-paint the main cursor merely to offer cosmetic settings.

The top-bar cursor control has a geometry-stable label. As established by the 2026-10-03 geometry incident, dynamic editor settings/status text must not determine the editor viewport width. The top bar has a responsive compact mode: save state, navigation mode, expansion controls, cursor settings, and a status entry point remain reachable at narrow widths, while lower-priority diagnostics move into a status window rather than clipping the primary controls.

Vim-lite is an optional navigation adapter over the existing `TextEditState`, not a second editor and not a Vim implementation.

Current mode contract:

- INSERT is the ordinary Lexwright editor
- `Ctrl+i` toggles between INSERT and NAV
- Escape has no mode-switching role
- NAV supports `h/j/k/l`, `w/b`, `0/$`, and `gg/G`
- wrapped-row vertical/start/end movement uses the already-laid-out egui galley
- word movement operates on canonical text but never mutates it
- cursor movement stores a new egui cursor range under the existing stable editor ID
- NAV does not create Lexwright document revisions because cursor movement is not a text mutation

NAV is fail-closed against mutation. When the main editor owns focus, the input adapter removes text, IME, paste, cut, deletion/newline/tab input, and common mutating undo/cut/paste/delete-word shortcuts before stock `TextEdit` receives the frame. Unsupported printable NAV keys therefore do nothing instead of inserting text.

This layer must remain small. Full Vim operators, registers, macros, command mode, configuration language, or modal editing semantics are outside the current contract unless individually justified by Lexwright's writing goals.

## 12. Durable document tabs

Tabs are document ownership, not UI chrome.

The required boundary is `DocumentState`. One document owns its canonical buffer, revision/save clocks, persistence worker, snapshot cache, analysis worker/results, Harper worker/results, live diagnostic revision, and pending document-edit state. Application-global editor settings and chrome remain outside that object.

`Workspace` owns the ordered durable document registry and active index. At process start it reads `$XDG_DATA_HOME/lexwright/workspace.tsv` (or `~/.local/share/lexwright/workspace.tsv`), falling back to the historical default ledger when no registry exists. Only the active document is loaded initially. Inactive entries are path/title records until first activation, after which their `DocumentState` remains resident so its independent save/analyzer generations survive tab switches.

The switching contract is:

1. Queue the outgoing document's current revision for persistence.
2. Take the target's resident `DocumentState`, or lazily load it from its durable path.
3. Swap the active document and retain the previous `DocumentState` in its workspace slot.
4. Persist the new active index in the workspace registry.
5. Reset app-global Vim-lite mode and close document-bound draft/action windows.
6. If Harper is globally enabled, seed the newly active document's Harper lane explicitly.

Each document's durable path is also part of its egui editor ID. Cursor and undo state therefore belong to the document instead of leaking through one process-global TextEdit ID.

The tabs row is rendered only after Lexwright captures the stable editor viewport width. Adding or changing tab labels therefore has no authority to change the document wrap width.

New `+` documents receive durable `documents/untitled-N.txt` paths. Closing, deleting, hiding, reopening, and reordering are intentionally separate lifecycle operations and are not inferred from one ambiguous close control. Until that policy exists, tabs may be created and switched but not destructively closed.

Expansion configuration remains globally persisted, while each loaded `EditorBuffer` owns its runtime expansion engine/session telemetry. Already-resident documents are not silently hot-reloaded after another document edits the global rules file. Cross-document rule synchronization requires an explicit experiment/telemetry policy before it is added.

Tabs must never become multiple labels that secretly share one global ledger, one undo state, or one analyzer generation.

### Launch profiles and scratch isolation

CLI launch options are orchestration around the same ownership boundaries, not alternate hidden editor implementations.

- `--tab NAME` resolves or creates one durable workspace entry and makes it active before the first frame.
- `--harper` enables the normal per-document Harper worker at startup.
- `--ruleset NAME` is a process-local ruleset override. It changes the in-memory expansion engine and recompiles its trie without persisting a new `@active` selection.
- `--scratch` is mutually exclusive with `--tab` and bypasses `Workspace::load_default` entirely. Scratch defaults to a durable expansion ruleset named `scratch`; the ruleset is created on first launch by cloning the currently active ruleset, but that creation and later edits preserve the globally persisted `@active` selection.

Scratch mode owns an ephemeral `DocumentState` whose `LedgerStore` has no persistence worker. Ordinary edits still receive revisions, background lightweight analysis, Harper diagnostics, expansion matching, and editor undo state, but no document save can be queued. The application also skips workspace rendering and the shutdown flush path. Scratch therefore remains ephemeral even if future code accidentally reaches `LedgerStore::queue_save`: the store itself rejects the write.

The scratch top bar must make the disposable lifecycle visually unmistakable without exposing the full editor's language-analysis controls. On Linux, scratch exit delegates clipboard ownership to the external `wl-copy` helper because Wayland clipboard data must continue to be served after the editor window disappears. **Enter** when the scratch editor is focused (or the **Copy + Quit** control) submits the scratch buffer. Empty or whitespace-only buffers skip `wl-copy` and close without overwriting the clipboard. Nonblank buffers are piped to `wl-copy` and Lexwright waits only for the short-lived launcher process before marking the window for close on the following UI frame. **Escape** dismisses scratch without copying, even while another scratch UI control has focus. The background clipboard-owner process intentionally outlives Lexwright. Lexwright must not capture a pipe inherited by that background process and then call `wait_with_output()`, because EOF would be delayed until clipboard ownership ends and the UI would freeze. If spawning, writing to, or waiting for the launcher fails, the close is cancelled and the draft remains visible with an explicit copy error.



### Font-size persistence

Editor font size and independent interface font size are fields of the existing `EditorSettings` record, not separate scratch/workspace preferences. The slider edits the live editor-font setting and persists it through the editor settings writer. Manual saving of the other editor-ergonomics draft explicitly preserves both already-live font-size values, preventing a stale draft from overwriting it. Both plain and decorated editor layout paths consume the same `editor_settings.font_size`.


### Cross-process editor-setting writes

Multiple Lexwright processes may coexist. Therefore an in-memory `EditorSettings` snapshot is not authoritative for fields a different process may have changed. Font-size updates use read/modify/write semantics that replace only `font_size`; generic cursor/navigation saves first reload and preserve the current on-disk font size. Regression tests model a stale process attempting to save cursor state after another process has changed font size and require the newer font size to survive.


### Native zoom persistence

egui owns Cmd/Ctrl + Plus/Equals/Minus/0 and applies those shortcuts to `Context::zoom_factor()` at the framework level. Lexwright deliberately leaves `Options::zoom_with_keyboard` enabled instead of shadowing that behavior.

`EditorSettings.zoom_factor` stores the last observed egui zoom. Startup applies that value to the egui context before normal interaction. During runtime, Lexwright observes changes to the native zoom factor and schedules one persistence write after a short idle period rather than doing synchronous disk I/O for every repeated keypress. Shutdown flushes a pending zoom write, including in scratch mode.

The acceptance contract is therefore the literal user path:

~~~text
Ctrl++ / Ctrl+= / Ctrl+- / Ctrl+0
-> egui native whole-app zoom
-> visible empty-or-nonempty UI scale
-> debounced persisted zoom_factor
-> restart
-> same whole-app scale
~~~

The editor-font slider is separate from whole-app zoom and must not be substituted for this shortcut path.

Detailed incident history: `docs/incidents/2026-10-07-font-size-native-zoom-control-path.md`.

This incident strengthens the severe-editor-bug debugging protocol with one additional hard gate: before
repairing a UI preference or persistence bug, record the literal user gesture and identify the framework
or application state that gesture actually mutates. A semantic label such as "font size" is not accepted
as proof of the event path.
