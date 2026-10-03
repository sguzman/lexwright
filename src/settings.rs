use std::{
    env,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process,
};

#[derive(Clone, Debug, PartialEq)]
pub struct EditorSettings {
    pub cursor_width: f32,
    pub cursor_blink: bool,
    pub cursor_on_seconds: f32,
    pub cursor_off_seconds: f32,
    pub vim_lite: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            cursor_width: 2.0,
            cursor_blink: true,
            cursor_on_seconds: 0.5,
            cursor_off_seconds: 0.5,
            vim_lite: false,
        }
    }
}

impl EditorSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(0.5..=12.0).contains(&self.cursor_width) {
            return Err(format!(
                "cursor width must be between 0.5 and 12.0, got {}",
                self.cursor_width
            ));
        }

        if !(0.05..=3.0).contains(&self.cursor_on_seconds) {
            return Err(format!(
                "cursor visible duration must be between 0.05s and 3.0s, got {}",
                self.cursor_on_seconds
            ));
        }

        if !(0.05..=3.0).contains(&self.cursor_off_seconds) {
            return Err(format!(
                "cursor hidden duration must be between 0.05s and 3.0s, got {}",
                self.cursor_off_seconds
            ));
        }

        Ok(())
    }
}

pub fn load_default() -> (EditorSettings, PathBuf, Option<String>) {
    let path = default_path();

    match load(&path) {
        Ok(settings) => (settings, path, None),
        Err(error) => (EditorSettings::default(), path, Some(error)),
    }
}

pub fn save(path: &Path, settings: &EditorSettings) -> Result<(), String> {
    settings.validate()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }

    let contents = format!(
        "# Lexwright editor settings\n\
         cursor_width\t{}\n\
         cursor_blink\t{}\n\
         cursor_on_seconds\t{}\n\
         cursor_off_seconds\t{}\n\
         vim_lite\t{}\n",
        settings.cursor_width,
        settings.cursor_blink,
        settings.cursor_on_seconds,
        settings.cursor_off_seconds,
        settings.vim_lite,
    );

    let temp_path = path.with_extension(format!("tmp-{}", process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, path)?;

        #[cfg(unix)]
        if let Some(parent) = path.parent()
            && let Ok(directory) = File::open(parent)
        {
            let _ = directory.sync_all();
        }

        Ok(())
    })();

    if let Err(error) = result {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("could not save {}: {error}", path.display()));
    }

    Ok(())
}

fn load(path: &Path) -> Result<EditorSettings, String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(EditorSettings::default());
        }
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };

    parse(&contents, path)
}

fn parse(contents: &str, path: &Path) -> Result<EditorSettings, String> {
    let mut settings = EditorSettings::default();

    for (index, raw_line) in contents.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = raw_line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some((key, value)) = raw_line.split_once('\t') else {
            return Err(format!(
                "{}:{line_number}: expected key<TAB>value",
                path.display()
            ));
        };

        match key {
            "cursor_width" => {
                settings.cursor_width = parse_f32(path, line_number, key, value)?;
            }
            "cursor_blink" => {
                settings.cursor_blink = parse_bool(path, line_number, key, value)?;
            }
            "cursor_on_seconds" => {
                settings.cursor_on_seconds = parse_f32(path, line_number, key, value)?;
            }
            "cursor_off_seconds" => {
                settings.cursor_off_seconds = parse_f32(path, line_number, key, value)?;
            }
            "vim_lite" => {
                settings.vim_lite = parse_bool(path, line_number, key, value)?;
            }
            other => {
                return Err(format!(
                    "{}:{line_number}: unknown editor setting {other:?}",
                    path.display()
                ));
            }
        }
    }

    settings
        .validate()
        .map_err(|error| format!("{}: {error}", path.display()))?;

    Ok(settings)
}

fn parse_f32(path: &Path, line_number: usize, key: &str, value: &str) -> Result<f32, String> {
    value.trim().parse::<f32>().map_err(|error| {
        format!(
            "{}:{line_number}: invalid {key} value {value:?}: {error}",
            path.display()
        )
    })
}

fn parse_bool(path: &Path, line_number: usize, key: &str, value: &str) -> Result<bool, String> {
    match value.trim() {
        "true" | "on" | "1" => Ok(true),
        "false" | "off" | "0" => Ok(false),
        other => Err(format!(
            "{}:{line_number}: {key} must be true or false, got {other:?}",
            path.display()
        )),
    }
}

fn default_path() -> PathBuf {
    if let Some(path) = nonempty_env("LEXWRIGHT_EDITOR_SETTINGS") {
        return PathBuf::from(path);
    }

    if let Some(config_home) = nonempty_env("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home).join("lexwright/editor.tsv");
    }

    if let Some(home) = nonempty_env("HOME") {
        return PathBuf::from(home).join(".config/lexwright/editor.tsv");
    }

    PathBuf::from("lexwright-editor.tsv")
}

fn nonempty_env(name: &str) -> Option<std::ffi::OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{EditorSettings, parse};

    #[test]
    fn parses_editor_settings() {
        let settings = parse(
            "cursor_width\t4\ncursor_blink\tfalse\ncursor_on_seconds\t0.25\ncursor_off_seconds\t0.75\nvim_lite\ttrue\n",
            Path::new("editor.tsv"),
        )
        .expect("settings parse failed");

        assert_eq!(
            settings,
            EditorSettings {
                cursor_width: 4.0,
                cursor_blink: false,
                cursor_on_seconds: 0.25,
                cursor_off_seconds: 0.75,
                vim_lite: true,
            }
        );
    }

    #[test]
    fn missing_fields_keep_defaults() {
        let settings =
            parse("cursor_width\t3\n", Path::new("editor.tsv")).expect("settings parse failed");

        assert_eq!(settings.cursor_width, 3.0);
        assert!(settings.cursor_blink);
        assert!(!settings.vim_lite);
    }

    #[test]
    fn rejects_out_of_range_cursor_width() {
        let error = parse("cursor_width\t99\n", Path::new("editor.tsv"))
            .expect_err("invalid width unexpectedly parsed");
        assert!(error.contains("cursor width"));
    }
}
