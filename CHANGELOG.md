# Changelog

This log records user-facing releases. Engineering notes and historical
investigations live in the architecture and incident documents.

## [0.1.0] - 2026-10-08

**First accepted MVP.** Lexwright is an ephemeral, keyboard-first text
scratchpad with programmable expansion rules for Linux and Wayland.

- Scratch overlay is the default launch mode, with pre-map floating on Hyprland.
- New invocations start blank; scratch text has no persistence or recovery path.
- Enter copies nonblank text and exits; blank or whitespace-only Enter exits
  without changing the clipboard.
- Escape dismisses scratch without copying, and Shift+Enter inserts a newline.
- Ctrl+J is not a Lexwright copy shortcut.
- Persistent, named expansion rulesets and configurable typing behavior.
- Independent persistent editor and interface font sizes.
- The existing durable English editor is still accessible with `--editor`.
- CI checks compilation, tests, Clippy, and rustfmt.

This version is a source release; no prebuilt packages or installers are
promised. Future changes follow semantic versioning: fixes in patch versions,
new compatible capabilities in minor versions, and breaking behavior changes
in major versions.
