# Lexwright incidents

Incidents are retained as engineering memory. They are not changelog entries and should not be sanitized into vague "bug fixes."

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

- `bd76f4b` — decouple editor width from dynamic status bar
- `a15c992` — clamp ASCII cursor indices at EOF
