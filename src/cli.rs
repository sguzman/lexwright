#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LaunchOptions {
    pub(crate) scratch: bool,
    pub(crate) overlay: bool,
    // Internal marker: Hyprland already spawned this process with floating rules.
    pub(crate) overlay_child: bool,
    pub(crate) harper: bool,
    pub(crate) ruleset: Option<String>,
    pub(crate) tab: Option<String>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            scratch: true,
            overlay: true,
            overlay_child: false,
            harper: false,
            ruleset: None,
            tab: None,
        }
    }
}

impl LaunchOptions {
    pub(crate) fn harper_on_start(&self) -> bool {
        self.harper
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Launch(LaunchOptions),
    LatencyProbe,
    Help,
}

pub(crate) fn parse_args<I, S>(arguments: I) -> Result<Command, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut options = LaunchOptions::default();
    let mut latency_probe = false;
    let mut help = false;
    let mut explicit_scratch = false;
    let mut explicit_editor = false;
    let mut explicit_overlay = false;
    let mut no_overlay = false;
    let mut launch_option_supplied = false;
    let mut arguments = arguments.into_iter().map(Into::into);

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--scratch" => {
                explicit_scratch = true;
                launch_option_supplied = true;
            }
            "--editor" => {
                explicit_editor = true;
                launch_option_supplied = true;
            }
            "--overlay" => {
                explicit_overlay = true;
                launch_option_supplied = true;
            }
            "--no-overlay" => {
                no_overlay = true;
                launch_option_supplied = true;
            }
            "--overlay-child" => {
                options.overlay_child = true;
                launch_option_supplied = true;
            }
            "--harper" => {
                options.harper = true;
                launch_option_supplied = true;
            }
            "--ruleset" => {
                launch_option_supplied = true;
                if options.ruleset.is_some() {
                    return Err("--ruleset may be supplied only once".to_owned());
                }
                let value = arguments
                    .next()
                    .ok_or_else(|| "--ruleset requires a name".to_owned())?;
                if value.trim().is_empty() {
                    return Err("--ruleset requires a non-empty name".to_owned());
                }
                options.ruleset = Some(value);
            }
            "--tab" => {
                launch_option_supplied = true;
                if options.tab.is_some() {
                    return Err("--tab may be supplied only once".to_owned());
                }
                let value = arguments
                    .next()
                    .ok_or_else(|| "--tab requires a name".to_owned())?;
                if value.trim().is_empty() {
                    return Err("--tab requires a non-empty name".to_owned());
                }
                options.tab = Some(value);
            }
            "--latency-probe" => latency_probe = true,
            "--help" | "-h" => help = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }

    if help {
        if latency_probe || launch_option_supplied {
            return Err("--help cannot be combined with launch options".to_owned());
        }
        return Ok(Command::Help);
    }
    if latency_probe {
        if launch_option_supplied {
            return Err("--latency-probe cannot be combined with launch options".to_owned());
        }
        return Ok(Command::LatencyProbe);
    }

    if explicit_scratch && explicit_editor {
        return Err("--scratch and --editor cannot be combined".to_owned());
    }
    if explicit_scratch && options.tab.is_some() {
        return Err("--scratch and --tab are separate launch modes".to_owned());
    }
    if explicit_overlay && no_overlay {
        return Err("--overlay and --no-overlay cannot be combined".to_owned());
    }
    if explicit_editor || options.tab.is_some() {
        if explicit_overlay || no_overlay || options.overlay_child {
            return Err("overlay options require scratch mode, not --editor/--tab".to_owned());
        }
        options.scratch = false;
        options.overlay = false;
    } else {
        options.scratch = true;
        options.overlay = !no_overlay;
    }
    if options.overlay_child && !explicit_overlay {
        return Err("--overlay-child requires an explicit --overlay".to_owned());
    }

    Ok(Command::Launch(options))
}

pub(crate) fn help_text() -> &'static str {
    "Lexwright — ephemeral scratchpad with programmable text expansion\n\n\
Usage:\n\
  lexwright [--ruleset NAME] [--harper]\n\
  lexwright --no-overlay [--ruleset NAME]\n\
  lexwright --editor [--tab NAME] [--harper] [--ruleset NAME]\n\
  lexwright --latency-probe\n\n\
Options:\n\
  --scratch        Scratch mode (already the default).\n\
  --overlay        Floating scratch overlay (already the default).\n\
  --no-overlay     Open scratch without the Hyprland floating overlay.\n\
  --editor         Open the legacy persistent editor instead of scratch.\n\
  --tab NAME       Open a durable named tab in editor mode (implies --editor).\n\
  --harper         Opt into Harper diagnostics for this session.\n\
  --ruleset NAME   Use an existing expansion ruleset for this process only.\n\
  --latency-probe  Run the editor latency probe and exit.\n\
  -h, --help       Show this help.\n\n\
Scratch shortcuts: Enter copies and exits; Shift+Enter inserts a newline;\n\
Escape discards and exits. Ctrl+J remains an alternate copy-and-exit shortcut.\n\
Scratch text is never saved. Rules and preferences persist.\n"
}

#[cfg(test)]
mod tests {
    use super::{Command, LaunchOptions, parse_args};

    fn launch(args: &[&str]) -> LaunchOptions {
        let Command::Launch(options) = parse_args(args.iter().copied()).unwrap() else {
            panic!("unexpected command");
        };
        options
    }

    #[test]
    fn zero_arguments_launch_ephemeral_overlay_without_harper() {
        assert_eq!(launch(&[]), LaunchOptions::default());
        assert!(launch(&[]).scratch);
        assert!(launch(&[]).overlay);
        assert!(!launch(&[]).harper_on_start());
    }

    #[test]
    fn editor_is_explicit_and_tab_implies_editor() {
        let expected = LaunchOptions {
            scratch: false,
            overlay: false,
            overlay_child: false,
            harper: true,
            ruleset: Some("aggressive".to_owned()),
            tab: Some("reply".to_owned()),
        };
        assert_eq!(
            launch(&[
                "--editor",
                "--tab",
                "reply",
                "--harper",
                "--ruleset",
                "aggressive"
            ]),
            expected
        );
        assert_eq!(
            launch(&["--tab", "reply", "--harper", "--ruleset", "aggressive"]),
            expected
        );
    }

    #[test]
    fn scratch_harper_is_opt_in() {
        assert!(!launch(&["--scratch"]).harper_on_start());
        assert!(launch(&["--harper"]).harper_on_start());
    }

    #[test]
    fn scratch_overlay_can_be_disabled() {
        let options = launch(&["--no-overlay"]);
        assert!(options.scratch);
        assert!(!options.overlay);
        assert!(!options.overlay_child);
        assert!(launch(&["--overlay"]).overlay);
        assert!(parse_args(["--editor", "--overlay"]).is_err());
        assert!(parse_args(["--overlay", "--no-overlay"]).is_err());
    }

    #[test]
    fn internal_overlay_child_requires_explicit_overlay() {
        assert!(parse_args(["--overlay-child"]).is_err());
        assert!(launch(&["--overlay", "--overlay-child"]).overlay_child);
    }

    #[test]
    fn scratch_and_durable_tab_are_mutually_exclusive() {
        assert!(parse_args(["--scratch", "--tab", "scratch"]).is_err());
        assert!(parse_args(["--scratch", "--editor"]).is_err());
    }

    #[test]
    fn help_and_probe_cannot_hide_launch_flags() {
        assert!(parse_args(["--scratch", "--help"]).is_err());
        assert!(parse_args(["--latency-probe", "--no-overlay"]).is_err());
        assert_eq!(parse_args(["--help"]).unwrap(), Command::Help);
        assert_eq!(
            parse_args(["--latency-probe"]).unwrap(),
            Command::LatencyProbe
        );
    }
}
