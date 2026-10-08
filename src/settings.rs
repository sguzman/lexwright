use std::{
    env,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process,
};

#[derive(Clone, Debug, PartialEq)]
pub struct EditorSettings {
    pub font_size: f32,
    pub zoom_factor: f32,
    pub cursor_width: f32,
    pub cursor_blink: bool,
    pub cursor_on_seconds: f32,
    pub cursor_off_seconds: f32,
    pub vim_lite: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_size: 18.0,
            zoom_factor: 1.0,
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
        if !(8.0..=48.0).contains(&self.font_size) {
            return Err(format!(
                "font size must be between 8.0 and 48.0, got {}",
                self.font_size
            ));
        }

        if !(0.2..=5.0).contains(&self.zoom_factor) {
            return Err(format!(
                "zoom factor must be between 0.2 and 5.0, got {}",
                self.zoom_factor
            ));
        }

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

pub fn save_font_size(path: &Path, font_size: f32) -> Result<EditorSettings, String> {
    let mut current = load(path)?;
    current.font_size = font_size;
    save(path, &current)?;

    let reloaded = load(path)?;
    if reloaded.font_size != font_size {
        return Err(format!(
            "font-size verification failed for {}: requested {}, reloaded {}",
            path.display(),
            font_size,
            reloaded.font_size
        ));
    }

    Ok(reloaded)
}

pub fn save_zoom_factor(path: &Path, zoom_factor: f32) -> Result<EditorSettings, String> {
    let mut current = load(path)?;
    current.zoom_factor = zoom_factor;
    save(path, &current)?;

    let reloaded = load(path)?;
    if reloaded.zoom_factor != zoom_factor {
        return Err(format!(
            "zoom verification failed for {}: requested {}, reloaded {}",
            path.display(),
            zoom_factor,
            reloaded.zoom_factor
        ));
    }

    Ok(reloaded)
}

pub fn save_preserving_display_settings(
    path: &Path,
    settings: &EditorSettings,
) -> Result<EditorSettings, String> {
    let current = load(path)?;
    let mut merged = settings.clone();
    merged.font_size = current.font_size;
    merged.zoom_factor = current.zoom_factor;
    save(path, &merged)?;
    Ok(merged)
}

pub fn save(path: &Path, settings: &EditorSettings) -> Result<(), String> {
    settings.validate()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }

    let contents = format!(
        "# Lexwright editor settings\n\
         font_size\t{}\n\
         zoom_factor\t{}\n\
         cursor_width\t{}\n\
         cursor_blink\t{}\n\
         cursor_on_seconds\t{}\n\
         cursor_off_seconds\t{}\n\
         vim_lite\t{}\n",
        settings.font_size,
        settings.zoom_factor,
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
            "font_size" => {
                settings.font_size = parse_f32(path, line_number, key, value)?;
            }
            "zoom_factor" => {
                settings.zoom_factor = parse_f32(path, line_number, key, value)?;
            }
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
    use std::{
        fs,
        path::Path,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        EditorSettings, load, parse, save, save_font_size, save_preserving_display_settings,
        save_zoom_factor,
    };

    #[test]
    fn parses_editor_settings() {
        let settings = parse(
            "font_size\t22\nzoom_factor\t1.4\ncursor_width\t4\ncursor_blink\tfalse\ncursor_on_seconds\t0.25\ncursor_off_seconds\t0.75\nvim_lite\ttrue\n",
            Path::new("editor.tsv"),
        )
        .expect("settings parse failed");

        assert_eq!(
            settings,
            EditorSettings {
                font_size: 22.0,
                zoom_factor: 1.4,
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
        assert_eq!(settings.font_size, 18.0);
        assert_eq!(settings.zoom_factor, 1.0);
        assert!(settings.cursor_blink);
        assert!(!settings.vim_lite);
    }

    #[test]
    fn save_then_reload_preserves_font_size() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("lexwright-font-size-{}-{nonce}", process::id()));
        let path = directory.join("editor.tsv");
        let expected = EditorSettings {
            font_size: 27.5,
            zoom_factor: 1.3,
            cursor_width: 3.0,
            vim_lite: true,
            ..EditorSettings::default()
        };

        save(&path, &expected).expect("settings save failed");
        let reloaded = load(&path).expect("settings reload failed");
        assert_eq!(reloaded, expected);

        let raw = fs::read_to_string(&path).expect("settings file unreadable");
        assert!(raw.contains("font_size\t27.5"));
        assert!(raw.contains("zoom_factor\t1.3"));

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn font_write_preserves_other_settings() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("lexwright-font-merge-{}-{nonce}", process::id()));
        let path = directory.join("editor.tsv");

        let initial = EditorSettings {
            font_size: 18.0,
            cursor_width: 4.0,
            cursor_blink: false,
            vim_lite: true,
            ..EditorSettings::default()
        };
        save(&path, &initial).expect("initial settings save failed");

        let merged = save_font_size(&path, 29.0).expect("font-size save failed");
        assert_eq!(merged.font_size, 29.0);
        assert_eq!(merged.cursor_width, 4.0);
        assert!(!merged.cursor_blink);
        assert!(merged.vim_lite);

        let reloaded = load(&path).expect("settings reload failed");
        assert_eq!(reloaded, merged);

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn stale_generic_save_cannot_clobber_newer_font_size() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "lexwright-font-stale-writer-{}-{nonce}",
            process::id()
        ));
        let path = directory.join("editor.tsv");

        let original = EditorSettings::default();
        save(&path, &original).expect("initial settings save failed");

        let stale = original.clone();
        save_font_size(&path, 31.0).expect("font-size save failed");
        save_zoom_factor(&path, 1.7).expect("zoom save failed");

        let mut stale_cursor_edit = stale;
        stale_cursor_edit.cursor_width = 5.0;
        let merged = save_preserving_display_settings(&path, &stale_cursor_edit)
            .expect("merged cursor save failed");

        assert_eq!(merged.font_size, 31.0);
        assert_eq!(merged.zoom_factor, 1.7);
        assert_eq!(merged.cursor_width, 5.0);

        let reloaded = load(&path).expect("settings reload failed");
        assert_eq!(reloaded.font_size, 31.0);
        assert_eq!(reloaded.zoom_factor, 1.7);

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn rejects_out_of_range_font_size() {
        let error = parse("font_size\t99\n", Path::new("editor.tsv"))
            .expect_err("invalid font size unexpectedly parsed");
        assert!(error.contains("font size"));
    }

    #[test]
    fn rejects_out_of_range_zoom_factor() {
        let error = parse("zoom_factor\t9\n", Path::new("editor.tsv"))
            .expect_err("invalid zoom factor unexpectedly parsed");
        assert!(error.contains("zoom factor"));
    }

    #[test]
    fn rejects_out_of_range_cursor_width() {
        let error = parse("cursor_width\t99\n", Path::new("editor.tsv"))
            .expect_err("invalid width unexpectedly parsed");
        assert!(error.contains("cursor width"));
    }
}
