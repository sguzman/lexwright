//! Hyprland-aware scratch overlay launch.
//!
//! Standard Wayland xdg_toplevel cannot request floating. A per-exec window
//! rule lets Hyprland float the scratch window before mapping it, without
//! ever inserting it into the tiling tree.

use std::{env, process::Command};

use crate::cli::LaunchOptions;

/// Return true only when the compositor has accepted a floating child launch.
pub(crate) fn maybe_launch_via_hyprland(launch: &LaunchOptions) -> Result<bool, String> {
    if !launch.overlay
        || launch.overlay_child
        || env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none()
    {
        return Ok(false);
    }

    let executable = env::current_exe()
        .map_err(|error| format!("cannot find Lexwright executable for overlay: {error}"))?;

    // hyprctl's exec dispatcher uses a shell. Quote each argument separately,
    // including the user-provided ruleset, to preserve literal semantics.
    let command = std::iter::once(executable.to_string_lossy().into_owned())
        .chain(env::args().skip(1))
        .chain(std::iter::once("--overlay-child".to_owned()))
        .map(|arg| shell_quote(&arg))
        .collect::<Vec<_>>()
        .join(" ");

    // Modern Lua config and legacy hyprlang config use different dispatch APIs.
    let lua_dispatch = format!(
        "hl.dsp.exec_cmd({command:?}, {{ float = true, center = true }})"
    );
    let legacy_dispatch = format!("[float; center] {command}");
    let variants: [Vec<&str>; 2] = [
        vec!["dispatch", &lua_dispatch],
        vec!["dispatch", "exec", &legacy_dispatch],
    ];
    let mut errors = Vec::new();

    for args in variants {
        match Command::new("hyprctl").args(&args).output() {
            Ok(output)
                if output.status.success()
                    && String::from_utf8_lossy(&output.stdout).trim() == "ok" =>
            {
                return Ok(true);
            }
            Ok(output) => errors.push(format!(
                "hyprctl {}: {} {}",
                args[1],
                String::from_utf8_lossy(&output.stderr).trim(),
                String::from_utf8_lossy(&output.stdout).trim(),
            )),
            Err(error) => errors.push(format!("hyprctl not available: {error}")),
        }
    }

    Err(format!(
        "could not float scratch without disturbing the tiling layout. {}",
        errors.join("; ")
    ))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::shell_quote;

    #[test]
    fn shell_quoting_preserves_apostrophes_spaces_and_empty_strings() {
        assert_eq!(shell_quote("scratch notes"), "'scratch notes'");
        assert_eq!(shell_quote("it's here"), "'it'\\''s here'");
        assert_eq!(shell_quote(""), "''");
    }
}
