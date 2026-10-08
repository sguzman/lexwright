# Lexwright incidents

Incidents are retained as engineering memory. They are not changelog entries and should not be reduced to vague bug-fix summaries.

Historical commit identifiers have been removed from these reports because this repository uses a clean
Git history. The technical failure sequences and resolved root causes remain documented.

A severe incident should document:

- what the user experienced
- what Lexwright did internally
- evidence that established the root cause
- failed or misleading hypotheses
- why diagnosis took as long as it did
- the exact fix
- regression protection
- permanent architectural/process rules

## 2026-10-03 — editor geometry jitter and EOF crash

[Full postmortem](2026-10-03-editor-geometry-jitter.md)

A dynamic top-bar analysis label expanded the parent egui UI and changed editor wrap width during ordinary typing, producing repeated whole-paragraph reflow. Geometry tracing showed the editor oscillating between roughly 946 px and 982.5 px despite no window resize. A separate ASCII index fast-path bug violated stock TextBuffer EOF clamping and caused an out-of-bounds crash.

Primary fixes:

- decouple editor width from dynamic status bar
- clamp ASCII cursor indices at EOF

## 2026-10-07 — font-size persistence, native Ctrl++ zoom, and control-path failure

[Full postmortem](2026-10-07-font-size-native-zoom-control-path.md)

A request that Ctrl++ size changes survive restart was repeatedly misimplemented as persistence for a
settings-window editor-font slider. The project accumulated a ten-commit failed patch series, a rollback,
a clean rebuild, a concurrency detour, and a wrong custom Ctrl++ implementation before establishing that
egui's native shortcut changes `Context::zoom_factor()`.

The accepted repair preserves egui's native fast whole-app zoom, persists the actual zoom factor after a
short idle, restores it at startup, and explicitly matches the empty `Write.` hint font to document text.

Primary final fixes:

- persist native Ctrl++ UI zoom
- formatting / full CI green
- match empty editor hint to document font / full CI green

The incident also records a major process failure: the project violated its own two-failed-fixes
instrumentation rule and repeatedly used the project owner as the detector that the wrong interaction path
had been repaired.
