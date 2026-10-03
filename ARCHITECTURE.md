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

The current editor storage is a contiguous Rust `String`, but Lexwright no longer delegates every insertion back through egui's generic String insertion helper.

For an ASCII-only ledger, an egui character index is exactly the same number as the UTF-8 byte index. Lexwright tracks that invariant and uses the index directly in O(1). This is the expected fast path for ordinary English typing.

If any non-ASCII text enters the document, Lexwright switches conservatively to UTF-8-aware index conversion. It does **not** rescan the whole document after deletions merely to see whether the final non-ASCII character disappeared.

The contiguous String remains a bootstrap limitation: middle-of-document insertion still shifts trailing bytes. A real rope / piece table / gap-buffer architecture cannot be honestly obtained while stock egui `TextEdit` requires the whole document as a contiguous `&str`. The eventual large-document solution therefore includes a Lexwright-owned editor surface rather than repeatedly flattening a non-contiguous buffer.

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

The current snapshot operation still copies the document once on the UI thread after the idle delay. Its duration is measured explicitly. The snapshot is an immutable `Arc<str>` shared by persistence and analysis, so adding observers does not multiply full-document copies. A future buffer/storage design must remove even this document-sized snapshot copy from routine persistence once measurements show it matters.

## 3. Programmable input

Text expansion is special because it intentionally changes canonical text while the user is typing, so it is the only linguistic layer currently allowed to run synchronously.

Rules are compiled into a **reversed trie**. On an activation character such as a space or punctuation, matching walks backward from the insertion point only while a trie branch exists. It does not regex-scan or search the document.

Current invariants:

- no regex engine in the keystroke path
- no heap allocation for a successful/failed suffix lookup
- no full-document scan by the expansion matcher
- expansion only activates on a single delimiter insertion
- a trigger must start at a word boundary
- the synchronous expansion layer only accepts replacements at least as long as their triggers, allowing the editor buffer to report the correct forward cursor advance immediately
- shortening and arbitrary rewrites belong in a later transformation layer with explicit cursor/state handling

Starter rules are compiled into the binary. User rules are stored in `$XDG_CONFIG_HOME/lexwright/expansions.tsv` (or `~/.config/lexwright/expansions.tsv`) and override starter rules by trigger.

The in-app rule editor works on a detached draft. Editing the draft does not rebuild or mutate the live trie. An explicit **Apply** action validates the entire configuration, writes it atomically, then rebuilds the trie. This keeps configuration work completely outside the normal typing hot path.

The config also persists whether the starter set is enabled. Turning starter rules off gives the user a blank expansion language without deleting their custom rules.

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

After the same 160 ms idle boundary used for persistence, Lexwright creates one immutable `Arc<str>` snapshot. The save worker and analysis worker receive shared references to that snapshot. The analyzer runs on its own named thread and returns a revision-tagged result.

If analysis falls behind, queued jobs are collapsed to the newest waiting revision before the next pass. Stale work is observationally useless and must never become backpressure on typing.

The analyzer now reports mechanical counts plus revision-tagged lexical spans. Closed-class English words can be classified directly from small explicit lexicons. Open-class guesses are intentionally named `*-like` because the first pass uses conservative lexical/suffix heuristics rather than pretending to be a statistical POS tagger.

An optional editor layouter consumes those already-computed spans and colors them only when the analysis revision exactly matches the live document revision. Classification never happens in the layouter. The overlay is off by default because egui calls custom TextEdit layouters at least once per frame, and visualization work is not allowed to tax the default writing path.

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

No analyzer owns the canonical text. No analyzer may synchronously mutate it.

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

A dependency belongs on the editor hot path only if its value clearly exceeds its latency, binary-size, startup, and maintenance cost. Heavy NLP libraries should live behind optional modules or worker boundaries.

The initial renderer is `glow`, selected intentionally to keep the native stack smaller than the default wgpu path. eframe/egui remain replaceable implementation choices; the editor and linguistic model should not become inseparable from GUI widgets.
