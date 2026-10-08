#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LaunchOptions {
    pub(crate) scratch: bool,
    pub(crate) overlay: bool,
    // Internal marker: Hyprland already spawned this process with floating rules.
    pub(crate) overlay_child: bool,
    pub(crate) harper: bool,
    pub(crate) ruleset: Option<String>,
    pub(crate) tab: Option<String>,
}

impl LaunchOptions {
    pub(crate) fn harper_on_start(&self) -> bool {
        self.scratch || self.harper
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
    let mut arguments = arguments.into_iter().map(Into::into);

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--scratch" => options.scratch = true,
            "--overlay" => options.overlay = true,
            "--overlay-child" => options.overlay_child = true,
            "--harper" => options.harper = true,
            "--ruleset" => {
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
        if latency_probe || options != LaunchOptions::default() {
            return Err("--help cannot be combined with launch options".to_owned());
        }
        return Ok(Command::Help);
    }

    if latency_probe {
        if options != LaunchOptions::default() {
            return Err("--latency-probe cannot be combined with launch options".to_owned());
        }
        return Ok(Command::LatencyProbe);
    }

    if options.overlay && !options.scratch {
        return Err("--overlay requires --scratch".to_owned());
    }
    if options.overlay_child && !options.overlay {
        return Err("--overlay-child requires --scratch --overlay".to_owned());
    }
    if options.scratch && options.tab.is_some() {
        return Err(
            "--scratch and --tab are separate launch modes and cannot be combined".to_owned(),
        );
    }

    Ok(Command::Launch(options))
}

pub(crate) fn help_text() -> &'static str {
    "Lexwright\n\n\
Usage:\n\
  lexwright [--tab NAME] [--harper] [--ruleset NAME]\n\
  lexwright --scratch [--overlay] [--ruleset NAME]\n\
  lexwright --latency-probe\n\n\
Options:\n\
  --tab NAME       Ensure a durable named tab exists and open it.\n\
  --harper         Start with Harper enabled.\n\
  --ruleset NAME   Use an existing expansion ruleset for this process only.\n\
  --overlay        Float a scratch window above existing tiles.\n\
  --scratch        Ephemeral one-document mode. Harper starts on; text is never saved.\n\
                   Uses durable ruleset 'scratch', creating it if needed.\n\
  --latency-probe  Run the editor latency probe and exit.\n\
  -h, --help       Show this help.\n"
}

#[cfg(test)]
mod tests {
    use super::{Command, LaunchOptions, parse_args};

    #[test]
    fn parses_productivity_launch_options() {
        let parsed = parse_args(["--tab", "reply", "--harper", "--ruleset", "aggressive"])
            .expect("arguments failed to parse");

        assert_eq!(
            parsed,
            Command::Launch(LaunchOptions {
                scratch: false,
                overlay: false,
                overlay_child: false,
                harper: true,
                ruleset: Some("aggressive".to_owned()),
                tab: Some("reply".to_owned()),
            })
        );
    }

    #[test]
    fn scratch_forces_harper_on_start() {
        let Command::Launch(options) =
            parse_args(["--scratch"]).expect("scratch arguments failed to parse")
        else {
            panic!("unexpected command");
        };

        assert!(options.harper_on_start());
    }

    #[test]
    fn overlay_is_only_available_in_scratch_mode() {
        let error = parse_args(["--overlay"]).expect_err("workspace overlay unexpectedly parsed");
        assert!(error.contains("--overlay requires --scratch"));

        let parsed = parse_args(["--scratch", "--overlay"])
            .expect("overlay scratch arguments failed to parse");
        let Command::Launch(options) = parsed else {
            panic!("unexpected command");
        };
        assert!(options.scratch && options.overlay);
        assert!(!options.overlay_child);
    }

    #[test]
    fn overlay_child_marker_requires_overlay() {
        let error = parse_args(["--scratch", "--overlay-child"])
            .expect_err("standalone internal marker unexpectedly parsed");
        assert!(error.contains("--overlay-child requires"));
    }

    #[test]
    fn scratch_and_durable_tab_are_mutually_exclusive() {
        let error = parse_args(["--scratch", "--tab", "scratch"])
            .expect_err("conflicting launch modes unexpectedly parsed");
        assert!(error.contains("--scratch and --tab"));
    }
}
