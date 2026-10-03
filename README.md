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

## Custom expansion rules

User rules are read once at startup from:

```text
$XDG_CONFIG_HOME/lexwright/expansions.tsv
```

or, when `XDG_CONFIG_HOME` is unset:

```text
~/.config/lexwright/expansions.tsv
```

Override the path with `LEXWRIGHT_EXPANSIONS`.

The format is deliberately trivial: one tab-separated rule per line.

```text
# trigger<TAB>replacement
idk	I don't know
fwiw	for what it's worth
```

User rules override starter rules with the same trigger. A malformed rule is shown in the Lexwright top bar rather than crashing the editor.

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

## Roadmap

Near-term work is intentionally ordered by dependency, not spectacle:

- establish startup/edit/save latency instrumentation
- replace the bootstrap `String` storage with a buffer designed for large ledgers
- add in-app rule editing and named expansion rulesets
- add spelling/grammar diagnostics (Harper-class behavior) asynchronously
- add token/POS overlays and counts
- add lexeme/morpheme inspection
- add named experimental English modes and transformation pipelines

See `ARCHITECTURE.md` for the invariants that future features must preserve.
