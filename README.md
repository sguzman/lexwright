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
- add spelling/grammar diagnostics (Harper-class behavior) asynchronously
- add token/POS overlays and counts
- add lexeme/morpheme inspection
- add named experimental English modes and transformation pipelines

See `ARCHITECTURE.md` for the invariants that future features must preserve.
