# Lexwright

**Lexwright is an English IDE.**

It is a native, latency-first writing instrument for writing, inspecting, transforming, and experimenting with English in real time.

The first product is deliberately small: open Lexwright, type into a durable local ledger, close it whenever you want, and come back to the same text. No save ritual. No browser. No network dependency.

## Principles

1. **Typing is sacred.** Disk I/O, grammar checking, morphology, tagging, indexing, and future language experiments must not block the keystroke-to-paint path.
2. **The ledger is durable by default.** Edits are autosaved locally with atomic replacement; shutdown performs a final flush.
3. **Language features are observers, not gatekeepers.** An analyzer may lag, fail, or be disabled without preventing editing.
4. **Local first.** The core editor needs no service, account, telemetry, or network connection.
5. **Experiments are reversible.** Abbreviation systems, spelling rules, grammar rules, morphology views, and future transformations should be independently switchable.
6. **Measure latency.** Performance claims belong in measurements, not vibes.

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
- one always-present ledger
- automatic local persistence
- atomic background saves
- visible save state
- a latency-bounded abbreviation engine
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

Lexwright now uses one custom TextEdit layouter in every visual state. Wrapping is a permanent editor invariant: text wraps to the current editor viewport width whether structure, morphology, Harper, or none of them are active. Analyzer updates may change color and underline, but they are not allowed to change text geometry or line-break behavior.


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

Harper diagnostics are also underlined directly in the editor whenever the result matches the live revision, so spelling/grammar mistakes remain visible without opening the diagnostics window. Spelling uses a red underline; capitalization and other grammar classes use distinct warm underlines. Harper underlines are painted **after** egui has finished laying out the text; they never participate in shaping or wrapping. The diagnostics window preserves each issue's exact source span, Harper category, message, priority, and structured replacement/insertion/removal suggestions. Suggestions can now be applied explicitly. Each apply operation is revision-checked against the snapshot that produced the diagnostic, validates UTF-8 byte boundaries, seeds egui's undo history with the exact pre-fix text/cursor state, moves the cursor deterministically after the replacement, and becomes a normal new Lexwright revision. **Ctrl+Z restores the pre-fix text.**

Harper results are revision-tagged and queued stale snapshots are collapsed before the next grammar pass. After the initial full pass, Harper diffs the newest snapshot against its last completed snapshot, expands the changed range to local sentence context (capped for pathological long sentences), preserves diagnostics outside that window, shifts unaffected later spans by the edit delta, and lints only the dirty window. The Harper tooltip reports `work: linted / total bytes incremental` so this behavior is directly inspectable. If the ledger changes before a suggestion is applied, that suggestion expires rather than editing the wrong bytes.

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
- turn the starter rules on or off
- override a starter trigger with your own replacement
- revert a draft back to the currently active rules
- apply changes live without restarting Lexwright

Draft editing does not mutate the live matcher. **Apply** validates the draft, atomically saves it, and recompiles the reversed trie. That work happens only on the explicit Apply action, never on the normal typing path.

The durable config lives at:

```text
$XDG_CONFIG_HOME/lexwright/expansions.tsv
```

or, when `XDG_CONFIG_HOME` is unset:

```text
~/.config/lexwright/expansions.tsv
```

Override the path with `LEXWRIGHT_EXPANSIONS`.

The file remains intentionally simple and human-editable:

```text
# Lexwright expansion rules
@starter	true
idk	I don't know
fwiw	for what it's worth
```

`@starter false` disables the built-in starter set. User rules with the same trigger as a starter rule override it.

The synchronous expansion layer still requires replacements to be at least as long as their triggers. Arbitrary shortening belongs in a later transformation layer because egui's current TextBuffer insertion contract only reports forward cursor advance cleanly.

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

For testing or alternate ledgers:

```bash
LEXWRIGHT_LEDGER=/path/to/ledger.txt cargo run --release
```

## Buffer direction

The current editor still uses a contiguous Rust `String` because egui's stock `TextEdit` exposes the document as a contiguous `&str`.

That is excellent for the common case of typing at the end of an ASCII ledger: index mapping is O(1) and appending is amortized O(1). It is not the final answer for enormous documents with frequent edits in the middle, because inserting into the middle of a contiguous string must shift trailing bytes.

Lexwright will not paper over that limitation with a fake "rope abstraction" while still flattening it for every frame. When large-document measurements justify the change, the correct next architecture is a Lexwright-owned editor surface backed by a piece table / rope / gap-oriented buffer.

## Roadmap

Near-term work is intentionally ordered by dependency, not spectacle:

- collect real latency measurements on normal and large ledgers
- decide the custom editor-buffer boundary from those measurements
- add in-app rule editing and named expansion rulesets
- deepen Harper incremental edit provenance beyond snapshot diffing
- deepen token/POS accuracy beyond the current heuristic overlay
- deepen lexeme/morpheme inspection beyond conservative normalization rules
- add named experimental English modes and transformation pipelines

See `ARCHITECTURE.md` for the invariants that future features must preserve.
