# Lexwright architecture

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

The current egui-backed string storage is a bootstrap implementation, not the final large-document buffer architecture. Before large-ledger performance becomes a problem, the editor layer should move behind a buffer designed for incremental edits.

## 2. Persistence

The ledger is user data, not disposable application state.

Current policy:

- default path: `$XDG_DATA_HOME/lexwright/ledger.txt`
- fallback: `~/.local/share/lexwright/ledger.txt`
- optional override: `LEXWRIGHT_LEDGER`
- saves happen on a dedicated worker thread
- an edit schedules a snapshot after 160 ms of idle time
- each snapshot is written to a temporary file, synced, and atomically renamed
- shutdown queues the newest revision and waits for the save queue to flush

The UI thread never performs the disk write.

The current snapshot operation still clones the document string on the UI thread. That is acceptable only for the bootstrap. A future buffer/storage design must remove document-sized copying from routine editing.

## 3. Programmable input

Text expansion is special because it intentionally changes canonical text while the user is typing, so it is the only linguistic layer currently allowed to run synchronously.

Rules are compiled once at startup into a **reversed trie**. On an activation character such as a space or punctuation, matching walks backward from the insertion point only while a trie branch exists. It does not regex-scan or search the document.

Current invariants:

- no regex engine in the keystroke path
- no heap allocation for a successful/failed suffix lookup
- no full-document scan by the expansion matcher
- expansion only activates on a single delimiter insertion
- a trigger must start at a word boundary
- the synchronous expansion layer only accepts replacements at least as long as their triggers, allowing the editor buffer to report the correct forward cursor advance immediately
- shortening and arbitrary rewrites belong in a later transformation layer with explicit cursor/state handling

Starter rules are compiled into the binary. User rules are loaded once at startup from `$XDG_CONFIG_HOME/lexwright/expansions.tsv` (or `~/.config/lexwright/expansions.tsv`) and override starter rules by trigger.

The current egui `TextBuffer` contract still uses character indices over a UTF-8 `String`; converting an insertion position to a byte position inherits egui/String's existing indexing cost. The trie itself adds only bounded local work. Replacing the bootstrap string buffer is therefore still a planned latency milestone.

## 4. Analysis model

Future language systems consume versioned document snapshots or edit deltas.

They return results tagged with the revision they analyzed:

```text
revision N
  -> tokenizer
  -> morphology
  -> POS
  -> grammar
  -> diagnostics(revision N)
```

The UI discards or visually marks stale results rather than blocking for a current answer.

No analyzer owns the canonical text.

## 5. Failure isolation

If a future subsystem fails:

- grammar dies -> typing continues
- POS tagger dies -> typing continues
- morphology dies -> typing continues
- an expansion config is invalid -> starter rules remain available and the error becomes visible
- save fails -> typing continues and the failure becomes visible
- network is unavailable -> core editing is unaffected

The editor is the load-bearing system. Everything else is optional machinery around it.

## 6. Latency policy

Lexwright does not claim a latency number without measuring it.

We should instrument at least:

- process start -> first interactive frame
- input event -> completed editor frame
- editor-frame CPU time
- expansion lookup duration / hit count
- save snapshot enqueue cost
- background save duration
- analyzer turnaround by revision

Performance regressions should eventually be testable with a repeatable benchmark harness.

## 7. Dependency policy

Keep the core dependency graph small.

A dependency belongs on the editor hot path only if its value clearly exceeds its latency, binary-size, startup, and maintenance cost. Heavy NLP libraries should live behind optional modules or worker boundaries.

The initial renderer is `glow`, selected intentionally to keep the native stack smaller than the default wgpu path. eframe/egui remain replaceable implementation choices; the editor and linguistic model should not become inseparable from GUI widgets.
