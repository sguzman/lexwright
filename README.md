# Lexwright

**An ephemeral, keyboard-first scratchpad with programmable text expansion.**

Lexwright is for the moments between applications: summon a floating text field, write something or expand shorthand with your own rules, press **Enter** to copy it, and move on. There are no scratch documents to manage, and nothing to save.

**Invoke → type / expand → Enter → done.**

Built in Rust with egui, targeting a quick repeated-invocation workflow on Linux and Wayland. On Hyprland, scratch launches as a floating overlay without disturbing the existing tiling layout.

## Use it

With Rust/Cargo installed:

```sh
cargo run --release
```

That opens an empty scratchpad in its default overlay mode. Every invocation starts fresh.

| Key | Scratch behavior |
| --- | --- |
| **Enter** | Copy nonblank scratch text using `wl-copy`, then close; blank text just closes |
| **Shift+Enter** | Insert a newline |
| **Escape** | Close and discard scratch text from anywhere in scratch mode, leaving the clipboard unchanged |

Copying requires the Wayland `wl-copy` command from **wl-clipboard**. If clipboard delivery fails, Lexwright stays open and reports the failure instead of discarding your text. Enter with empty or whitespace-only text skips `wl-copy` entirely and preserves the clipboard. Ctrl+J is no longer a Lexwright shortcut.

On Hyprland, Lexwright arranges for the floating window rule **before** mapping the window, so a tiled workspace is not temporarily rearranged. Scratch remains available on other compositors, though non-Hyprland compositors may need their own window rules to guarantee floating.

## Launch options

```sh
lexwright                             # default: ephemeral scratch overlay
lexwright --no-overlay                # scratch without Hyprland floating dispatch
lexwright --ruleset aggressive        # scratch with a chosen expansion ruleset
lexwright --harper                    # opt into language diagnostics
lexwright --editor                    # older persistent, tabbed editor
lexwright --editor --tab notes        # open a durable tab
lexwright --latency-probe             # CPU-side editor latency measurements
lexwright --help
```

When running from the source tree, insert `--` before Lexwright flags, for example `cargo run --release -- --editor`.

The full editor and its linguistic inspection features are preserved, but **not** the default or the current product-development focus.

## What persists (and what does not)

**Scratch text is disposable.** No autosave, document file, history, recovery file, or session restoration is created for scratch mode. The editor buffer and undo state exist only while that window is open. Closing or crashing discards them.

**Your rules and preferences persist.** Lexwright loads expansion rules from the normal config path (generally `~/.config/lexwright/expansions.tsv`) and appearance/editor preferences from `~/.config/lexwright/editor.tsv`. It maintains a dedicated durable ruleset named `scratch` by default, without changing which ruleset the legacy editor has selected. You can override the scratch ruleset for one process with `--ruleset NAME`.

Expansion remains the main programmable feature: configured abbreviations are replaced as you type a delimiter such as a space or punctuation. The rules editor is available from the **rules** control in the scratch bar; rules are saved deliberately with **Save & activate**, never inferred from discarded scratch text.

## Appearance

Open **appearance** in the top bar to configure two independent sizes:

- **Editor font size** — text you compose.
- **Interface font size** — labels, controls, rules, rule-editing inputs, and settings.

Both settings persist immediately; they do not need the legacy document editor. Native whole-app zoom still works separately.

## Product boundaries

The priority is **fast launch, immediate focus, predictable expansion, reliable clipboard handoff, clean exit, and readable settings**. The UI should stay small enough to get out of the way.

Lexwright retains its existing persistent workspace, tabs, Vim-lite navigation, language-coloring overlays, morphology experiments, Harper diagnostics, and other editor infrastructure under `--editor`. They are maintained for compatibility, not presented as the product's central purpose. Future work on a comprehensive English IDE can live in another application.

## Development

```sh
cargo check --all-targets
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

CI runs these checks on every push to `main`.

Technical details and the preserved secondary editor architecture live in [ARCHITECTURE.md](ARCHITECTURE.md). Historical incident notes are in [docs/incidents](docs/incidents).
