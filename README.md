# Lexwright

**Lexwright is an English IDE.**

It is a native, latency-first writing instrument for writing, inspecting, transforming, and experimenting with English in real time.

The first product is deliberately small: open Lexwright, type into durable local documents, switch between them without a save ritual, close the app whenever you want, and come back to the same workspace. No browser. No network dependency.

## Principles

1. **Typing is sacred.** Disk I/O, grammar checking, morphology, tagging, indexing, and future language experiments must not block the keystroke-to-paint path.
2. **The ledger is durable by default.** Edits are autosaved locally with atomic replacement; shutdown performs a final flush.
3. **Language features are observers, not gatekeepers.** An analyzer may lag, fail, or be disabled without preventing editing.
4. **Local first.** The core editor needs no service, account, telemetry, or network connection.
5. **Experiments are reversible.** Abbreviation systems, spelling rules, grammar rules, morphology views, and future transformations should be independently switchable.
6. **Measure latency.** Performance claims belong in measurements, not vibes.
7. **Editor geometry is sacred too.** Status text, telemetry, analyzer freshness, save state, and other ancillary UI must never resize the writing surface.

## Intended layers

```text
keystrokes
  -> text buffer
  -> programmable input / expansion
  -> tokens
  -> morphemes + lexemes
  -> parts of speech
  -> syntax
  -> spelling + grammar diagnostics
  -> user-defined transformations
```

Only the first two layers are allowed to participate synchronously in normal editing, and even programmable input must remain bounded and cheap. Everything heavier belongs off the UI thread.

## Current milestone

The instrument now has:

- native Rust + egui/eframe application
- durable local document tabs with one always-present fallback ledger
- automatic local persistence
- atomic background saves
- visible save state
- a latency-bounded abbreviation engine
- named, switchable expansion rulesets with notes and rename
- per-ruleset and per-rule session compression telemetry
- exportable Markdown expansion-session reports
- durable cursor ergonomics controls
- opt-in, non-mutating Vim-lite navigation
- lazy-loaded per-document state with independent persistence/analyzer generations
- hot-path latency telemetry
- an O(1) character-to-byte index fast path for ordinary ASCII English
- a revision-tagged background analysis worker
- live word/character/line/paragraph statistics
- an optional lexical-structure color overlay
- grammatical-category counts with explicit heuristic labeling
- an optional prefix/stem/suffix morphology overlay
- conservative lexeme candidates with spelling-alternation rules
- a lexeme-inspection window
- opt-in Harper spelling/grammar diagnostics with structured suggestions
- one immutable snapshot shared by persistence and active analyzers
- no linguistic analyzer on the input path

The expansion matcher is compiled into a reversed trie. Typing an activation character such as a space or punctuation only walks backward through a possible trigger; it does not regex-scan or rescan the document.

Starter rules include:

```text
abt   -> about
bc    -> because
ppl   -> people
prob  -> probably
rly   -> really
shd   -> should
smth  -> something
teh   -> the
wld   -> would
woudl -> would
```

Typing `bc `, for example, becomes `because ` immediately.

Click the `expand on/off` indicator in the top bar to toggle expansion.

## Latency telemetry

The top bar now includes a compact `perf edit ...` readout. Hover it for:

- process start -> first UI frame
- frame CPU last / average / max
- edit mutation CPU last / average / max
- expansion trie lookup last / average / max
- ASCII O(1) index-path hit rate
- autosave snapshot-clone time
- background atomic-save time
- document byte size and active index path

The telemetry intentionally distinguishes **CPU work inside Lexwright** from display/compositor latency. We do not claim end-to-end key-to-photon latency from numbers we cannot actually observe.

For normal ASCII English, Lexwright maps egui character indices directly to byte indices in O(1). Once non-ASCII text enters the ledger, it conservatively uses UTF-8 character-index conversion rather than rescanning the entire document merely to decide whether the fast path can be re-enabled.


## Background analysis lane

Lexwright now has the first real analysis worker. The top bar shows a live `words N` readout; hover it for characters, bytes, lines, paragraphs, analyzed revision, and analyzer CPU time.

The important part is architectural rather than the simple counts:

```text
typing
  -> 45 ms idle (when Harper is enabled)
       |-> one cached Arc<str> snapshot
       |-> incremental Harper worker
  -> 160 ms idle
       |-> reuse the same snapshot if the revision is unchanged
       |-> save worker
       |-> structure/morphology worker
```

The UI thread caches at most one immutable snapshot for the current revision. If Harper requests it first at 45 ms, the later 160 ms persistence/analysis pass reuses that same allocation rather than cloning the ledger again. Any new edit invalidates the cache.

Analysis results carry the revision they observed. If the user edits again while analysis is running, the UI can identify the result as stale instead of blocking for a fresh answer. The worker also collapses queued stale jobs to the newest waiting snapshot before beginning its next pass.

The current analyzer now produces mechanical counts, lexical-category spans, morphology spans, and conservative lexeme candidates. Harper-class diagnostics and richer tagging can plug into the same worker boundary later; none of them get permission to enter the keystroke path.


## Structure overlay

Click **structure off** in the top bar to enable Lexwright's first language-structure view.

The background analyzer now records byte spans and category counts. When the analyzed revision exactly matches the text on screen, Lexwright can color the corresponding words inside the editable text itself.

The initial classifier deliberately distinguishes between two confidence levels:

- closed lexical classes such as **pronoun**, **determiner**, **preposition**, **conjunction**, and **auxiliary** use explicit English word sets
- open lexical classes are labeled **verb-like**, **adjective-like**, **adverb-like**, and **noun-like** because they currently use conservative word-list and suffix heuristics

Unclassified words remain the normal text color. Hover **structure on/off** for the current category counts and the methodological warning.

This is scaffolding for the real system, not a claim that suffixes solve part-of-speech tagging. The useful achievement is that revision-tagged linguistic spans can now flow from the background analyzer into the live editor without the classifier entering the typing path.

Normal writing now uses egui's stock multiline TextEdit layouter exactly. Harper is a post-paint overlay and never replaces that layout path. Only explicit structure/morphology visualization opts into Lexwright's colored custom layouter; that custom path mirrors egui's editor geometry settings, including viewport wrapping, line height, and preserved trailing whitespace.


## Morphology overlay

Click **morph off** to switch the editor into orthographic morphology mode. Structure and morphology are mutually exclusive visual modes so their colors never fight each other.

The first morphology pass recognizes a conservative set of productive English prefixes and suffixes and can peel up to two layers from either side. For example:

```text
unhelpfulness
un | help | ful | ness
```

The three visual roles are **prefix**, **stem**, and **suffix**. Hover the morph control for counts of decomposed words, prefixes, and suffixes.

The colored morphology remains explicitly **surface/orthographic morphology**. A separate observational lexeme layer now derives conservative candidates from that surface segmentation. Affixes are no longer accepted merely because their letters match the edge of a word: stripping must leave a positively supported base or participate in a supported stacked-affix pattern. Initial normalization rules include `happi + ness -> happy`, `runn + ing -> run`, `probab + ly -> probable`, and `believ + able -> believe`. The surface bytes remain canonical and are never rewritten by this analysis.

Click **lexemes N** in the top bar to inspect the current candidates, their surface stems, and the rule used. Obvious false suffix words such as `something`, `nothing`, `everything`, and `anything` are excluded from the morphology pass. Prefixes are also evidence-gated, which prevents accidental analyses such as `really -> re + ally` or `decisions -> de + cision + s`.

Like the structure overlay, morphology and lexeme derivation are computed on the background analysis worker and exposed only when their revision exactly matches the current ledger.





## Harper diagnostics

Lexwright pins **harper-core 2.11.0** behind a dedicated `lexwright-harper` worker.

Harper is **off by default**. Merely opening Lexwright does not initialize its dictionary and normal ledger typing does not send Harper snapshots. Click **harper off** to open its panel and explicitly enable background diagnostics.

Once enabled, the same immutable snapshot fans out independently:

```text
Arc<str>
  |-> atomic save worker
  |-> fast structure/morphology worker
  |-> Harper grammar/spelling worker
```

The Harper dictionary and curated American-English linter are initialized lazily on the worker after the first request. They never run on the process-start -> first-frame path.

Harper diagnostics are also underlined directly in the editor, so spelling/grammar mistakes remain visible without opening the diagnostics window. Unaffected underlines persist across live edits instead of the whole document blinking off until the next Harper result. Spelling uses a red underline; capitalization and other grammar classes use distinct warm underlines. Harper underlines are painted **after** egui has finished laying out the text; they never participate in shaping or wrapping. The diagnostics window preserves each issue's exact source span, Harper category, message, priority, and structured replacement/insertion/removal suggestions. Suggestions can now be applied explicitly. Each apply operation is revision-checked against the snapshot that produced the diagnostic, validates UTF-8 byte boundaries, seeds egui's undo history with the exact pre-fix text/cursor state, moves the cursor deterministically after the replacement, and becomes a normal new Lexwright revision. **Ctrl+Z restores the pre-fix text.**

Harper results are revision-tagged and queued stale snapshots are collapsed before the next grammar pass. After the initial full pass, Harper diffs the newest snapshot against its last completed snapshot, expands the changed range to local sentence context (capped for pathological long sentences), preserves diagnostics outside that window, shifts unaffected later spans by the edit delta, and lints only the dirty window. The editor itself also records exact insert/delete/replace deltas. While a fresh Harper pass is pending, diagnostics far from the edit are rebased immediately and remain visible; only a small neighborhood around the edit is invalidated. The Harper tooltip reports `work: linted / total bytes incremental` so this behavior is directly inspectable. If the ledger changes before a suggestion is applied, that suggestion expires rather than editing the wrong bytes.

## Repeatable latency probe

For a deterministic scaling check that runs without opening the GUI:

```bash
cargo run --release -- --latency-probe
```

The probe measures append edits, middle-of-document edits, and snapshot cloning at 4 KiB, 64 KiB, 1 MiB, and 8 MiB ledger sizes. It prints CPU-side timings only; it does not pretend to measure keyboard hardware, compositor, scanout, or display response.

This exists to answer a concrete engineering question: **at what document size does the current contiguous String stop being good enough on the actual machine?**


## Expansion rules

Lexwright now has an in-app rule editor. Click **rules** beside the expansion status in the top bar.

The panel lets you:

- add, edit, and remove user rules
- keep multiple named expansion rulesets
- clone the active ruleset into a new experiment
- rename the active ruleset without losing its session telemetry
- attach a durable one-line note/hypothesis to each ruleset
- switch active rulesets explicitly
- delete old experiments while always retaining at least one set
- turn the starter rules on or off independently per ruleset
- override a starter trigger with your own replacement
- discard a draft back to the currently saved active rules
- save and activate changes live without restarting Lexwright

Draft editing does not mutate the live matcher. **Save & activate** validates the draft, atomically saves it, and recompiles the reversed trie. That work happens only on the explicit Save & activate action, never on the normal typing path.

The durable config lives at:

```text
$XDG_CONFIG_HOME/lexwright/expansions.tsv
```

or, when `XDG_CONFIG_HOME` is unset:

```text
~/.config/lexwright/expansions.tsv
```

Override the path with `LEXWRIGHT_EXPANSIONS`.

The file remains intentionally simple and human-editable. Existing flat files still load as a ruleset named `default`; saving with the new system writes the named-set format:

```text
# Lexwright expansion rulesets
@active	default

@set	default
@note	baseline everyday shorthand
@starter	true
idk	I don't know
fwiw	for what it's worth

@set	aggressive
@note	test shorter triggers
@starter	false
bc	because
wld	would
```

`@active` chooses the one ruleset compiled into the hot-path trie. Each `@set` owns its own starter-rule setting and user rules. User rules with the same trigger as a starter rule override it. Switching sets rebuilds only on the explicit UI action; normal typing still sees one precompiled matcher.

Lexwright also keeps **session-only compression telemetry per ruleset**. Hover the expansion control for the active set, or inspect the rules window's comparison table to see every set side by side: expansion hits, trigger characters typed, replacement characters produced, percentage of expanded-word characters actually typed, and characters avoided. Zero-hit sets remain visible so experiments have an explicit baseline. Saving a changed ruleset automatically clears that set's prior session measurements so two different rule definitions are never silently blended; the rules window also has an explicit **Reset active stats** action. User-rule rows show their own hit and avoided-character counts, making dead or high-value abbreviations visible directly beside the rule definition. These counters update only when an expansion succeeds; they do not add work to ordinary non-expanding keystrokes.

Each ruleset also carries a one-line **note** so an experiment can record its hypothesis instead of relying on the name alone. **Rename** changes the profile name while keeping its runtime counters attached. **Export report** writes a Markdown snapshot of the current ruleset comparison and active per-rule measurements to:

```text
$XDG_STATE_HOME/lexwright/expansion-session.md
```

or `~/.local/state/lexwright/expansion-session.md` when `XDG_STATE_HOME` is unset. Export is an explicit UI action and does no work during normal typing.

The synchronous expansion layer still requires replacements to be at least as long as their triggers. Arbitrary shortening belongs in a later transformation layer because egui's current TextBuffer insertion contract only reports forward cursor advance cleanly.

## Editor ergonomics

Lexwright has a deliberately small editor-ergonomics layer that does not replace the stock text-layout path.

Click the fixed **cursor** control in the top bar to configure:

- cursor stroke width from 0.5 px to 12 px
- cursor blinking on/off
- visible and hidden blink durations
- opt-in Vim-lite navigation

Settings are durable at:

```text
$XDG_CONFIG_HOME/lexwright/editor.tsv
```

or:

```text
~/.config/lexwright/editor.tsv
```

The cursor controls use egui's built-in `TextCursorStyle`; Lexwright does not custom-paint or reshape the cursor. This intentionally gives us safe width/blink customization without touching text geometry. Arbitrary cursor-height customization is deferred unless it can be done without replacing the stable editor/cursor paint path.

### Vim-lite

Vim-lite is intentionally **not Vim emulation**. It is a small navigation layer for moving around English text while retaining Lexwright's normal editor.

When enabled in **cursor → Editor ergonomics**:

```text
Esc      INSERT -> NAV
i        NAV -> INSERT

h / j / k / l   character / wrapped-row movement
w / b           next / previous word start
0 / $           wrapped-row start / end
gg / G           document start / end
```

The top bar shows **INS** or **NAV** while Vim-lite is enabled. The mode button can also be clicked to toggle modes.

NAV is deliberately non-mutating. While the main editor has focus, NAV consumes text input, IME composition, paste, cut, Backspace, Delete, Enter, Tab, and common mutating Ctrl/Cmd shortcuts before `TextEdit` can process them. There are no `x`, `dd`, operators, registers, macros, command line, or hidden Vim behaviors.

Vim-lite moves egui's existing `TextEditState` cursor. It does not replace the editor widget, buffer, layout, wrapping, persistence, or analyzer paths.

## Document tabs

Lexwright now has durable document tabs below the main status bar. Tabs are real documents, not alternate labels over one shared buffer.

The existing default ledger remains the fallback first document. Click **+** to create and activate a new durable document. New untitled documents are allocated under:

```text
$XDG_DATA_HOME/lexwright/documents/untitled-N.txt
```

or `~/.local/share/lexwright/documents/untitled-N.txt` when `XDG_DATA_HOME` is unset.

The durable workspace registry lives at:

```text
$XDG_DATA_HOME/lexwright/workspace.tsv
```

or `~/.local/share/lexwright/workspace.tsv`. Override that registry path with `LEXWRIGHT_WORKSPACE`.

Only the active document is loaded at startup. An inactive tab is loaded lazily the first time it is activated; once loaded, it retains its own buffer, revision/save clocks, snapshot cache, analysis state, Harper state, and background persistence worker. Switching tabs queues the outgoing document's current revision before the swap so unsaved text is not stranded.

Each document also gets its own stable egui editor ID derived from its durable path, which isolates cursor and undo state across tabs. Vim-lite resets to INSERT on a document switch rather than carrying modal state into another document.

Close/delete/reorder semantics are deliberately not implemented yet. A tab currently represents durable user data, so Lexwright will not make a close button ambiguously mean hide, unload, or delete until that lifecycle is specified explicitly.

Expansion rules are still stored in the global expansion configuration, while runtime expansion state/telemetry lives inside each loaded document buffer. Already-loaded tabs are therefore not forcibly hot-reloaded when another tab saves rule changes; this avoids silently mixing per-document experiment sessions until cross-document rule synchronization has an explicit policy.

## Incident memory

Severe editor failures are documented under [`docs/incidents/`](docs/incidents/README.md).

The first major incident is intentionally preserved in detail: [2026-10-03 editor geometry jitter and EOF crash](docs/incidents/2026-10-03-editor-geometry-jitter.md).

That incident established two non-negotiable engineering rules:

- dynamic status UI must be geometrically isolated from the editor; a stable window must not acquire a different editor wrap width merely because a label changes
- optimized editor fast paths must preserve the complete behavior of the stock implementation they replace, including EOF edge cases

It also established a debugging rule: when the same load-bearing editor failure survives two targeted fixes, **instrument the failing quantity before making further speculative architecture changes**.

The geometry recorder that resolved the incident remains available at:

```text
$XDG_STATE_HOME/lexwright/editor-trace.tsv
```

or:

```text
~/.local/state/lexwright/editor-trace.tsv
```

## Run

```bash
cargo run --release
```

Lexwright stores the default ledger at:

```text
$XDG_DATA_HOME/lexwright/ledger.txt
```

or, when `XDG_DATA_HOME` is unset:

```text
~/.local/share/lexwright/ledger.txt
```

For testing or alternate initial ledgers:

```bash
LEXWRIGHT_LEDGER=/path/to/ledger.txt cargo run --release
```

Once a workspace registry exists, its registered tabs are authoritative for that workspace. To test with a separate tab registry as well, set `LEXWRIGHT_WORKSPACE` to a different path.

## Buffer direction

The current editor still uses a contiguous Rust `String` because egui's stock `TextEdit` exposes the document as a contiguous `&str`.

That is excellent for the common case of typing at the end of an ASCII ledger: index mapping is O(1) and appending is amortized O(1). It is not the final answer for enormous documents with frequent edits in the middle, because inserting into the middle of a contiguous string must shift trailing bytes.

Lexwright will not paper over that limitation with a fake "rope abstraction" while still flattening it for every frame. When large-document measurements justify the change, the correct next architecture is a Lexwright-owned editor surface backed by a piece table / rope / gap-oriented buffer.

## Roadmap

Near-term work is intentionally ordered by dependency, not spectacle:

- collect real latency measurements on normal and large ledgers
- decide the custom editor-buffer boundary from those measurements
- define explicit close/reopen/reorder lifecycle semantics for durable document tabs
- deepen expansion experimentation beyond named rulesets
- deepen Harper incremental edit provenance beyond snapshot diffing
- deepen token/POS accuracy beyond the current heuristic overlay
- deepen lexeme/morpheme inspection beyond conservative normalization rules
- add named experimental English modes and transformation pipelines

See `ARCHITECTURE.md` for the invariants that future features must preserve.
