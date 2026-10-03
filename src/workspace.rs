use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{
    document::DocumentState,
    storage::{atomic_write, default_ledger_path},
};

pub(crate) struct Workspace {
    tabs: Vec<TabEntry>,
    active_index: usize,
    registry_path: PathBuf,
    error: Option<String>,
}

struct TabEntry {
    path: PathBuf,
    title: String,
    loaded: Option<DocumentState>,
}

impl Workspace {
    pub(crate) fn load_default() -> (Self, DocumentState) {
        let registry_path = default_workspace_path();
        let fallback = default_ledger_path();

        let (paths, active_index, error) = match load_registry(&registry_path, &fallback) {
            Ok((paths, active_index)) => (paths, active_index, None),
            Err(error) => (vec![fallback], 0, Some(error)),
        };

        let tabs = paths
            .into_iter()
            .map(|path| TabEntry {
                title: title_for_path(&path),
                path,
                loaded: None,
            })
            .collect::<Vec<_>>();

        let active_index = active_index.min(tabs.len().saturating_sub(1));
        let document = DocumentState::load_path(tabs[active_index].path.clone());

        (
            Self {
                tabs,
                active_index,
                registry_path,
                error,
            },
            document,
        )
    }

    pub(crate) fn len(&self) -> usize {
        self.tabs.len()
    }

    pub(crate) fn active_index(&self) -> usize {
        self.active_index
    }

    pub(crate) fn title(&self, index: usize) -> Option<&str> {
        self.tabs.get(index).map(|tab| tab.title.as_str())
    }

    pub(crate) fn path(&self, index: usize) -> Option<&Path> {
        self.tabs.get(index).map(|tab| tab.path.as_path())
    }

    pub(crate) fn registry_path(&self) -> &Path {
        &self.registry_path
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn switch_to(
        &mut self,
        active_document: &mut DocumentState,
        target_index: usize,
    ) -> Result<(), String> {
        if target_index >= self.tabs.len() {
            return Err(format!(
                "tab index {target_index} is outside workspace with {} tabs",
                self.tabs.len()
            ));
        }

        if target_index == self.active_index {
            return Ok(());
        }

        let target_path = self.tabs[target_index].path.clone();
        let next_document = match self.tabs[target_index].loaded.take() {
            Some(document) => document,
            None => DocumentState::load_path(target_path),
        };

        let previous_index = self.active_index;
        let previous_document = std::mem::replace(active_document, next_document);
        self.tabs[previous_index].loaded = Some(previous_document);
        self.active_index = target_index;
        self.persist();

        Ok(())
    }

    pub(crate) fn create_and_activate(
        &mut self,
        active_document: &mut DocumentState,
    ) -> Result<usize, String> {
        let path = self.next_untitled_path()?;
        let title = title_for_path(&path);
        let next_document = DocumentState::load_path(path.clone());

        let previous_index = self.active_index;
        let previous_document = std::mem::replace(active_document, next_document);
        self.tabs[previous_index].loaded = Some(previous_document);

        self.tabs.push(TabEntry {
            path,
            title,
            loaded: None,
        });
        self.active_index = self.tabs.len() - 1;
        self.persist();

        Ok(self.active_index)
    }

    pub(crate) fn flush_inactive(&self) -> Vec<String> {
        let mut errors = Vec::new();

        for tab in &self.tabs {
            let Some(document) = tab.loaded.as_ref() else {
                continue;
            };

            if let Err(error) = document.store.flush() {
                errors.push(format!("{}: {error}", tab.path.display()));
            }
        }

        errors
    }

    fn next_untitled_path(&self) -> Result<PathBuf, String> {
        let directory = default_documents_dir();

        for index in 1_u64.. {
            let candidate = directory.join(format!("untitled-{index}.txt"));
            let already_registered = self.tabs.iter().any(|tab| tab.path == candidate);

            if !already_registered && !candidate.exists() {
                return Ok(candidate);
            }
        }

        Err("could not allocate an untitled document path".to_owned())
    }

    fn persist(&mut self) {
        self.error = save_registry(&self.registry_path, &self.tabs, self.active_index).err();
    }
}

fn load_registry(path: &Path, fallback: &Path) -> Result<(Vec<PathBuf>, usize), String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((vec![fallback.to_path_buf()], 0));
        }
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };

    parse_registry(&contents, path, fallback)
}

fn parse_registry(
    contents: &str,
    path: &Path,
    fallback: &Path,
) -> Result<(Vec<PathBuf>, usize), String> {
    let mut tabs = Vec::<PathBuf>::new();
    let mut active_index = 0_usize;

    for (offset, raw_line) in contents.lines().enumerate() {
        let line_number = offset + 1;
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((kind, value)) = raw_line.split_once('\t') else {
            return Err(format!(
                "{}:{line_number}: expected kind<TAB>value",
                path.display()
            ));
        };

        match kind {
            "active" => {
                active_index = value.trim().parse::<usize>().map_err(|error| {
                    format!(
                        "{}:{line_number}: invalid active tab index {value:?}: {error}",
                        path.display()
                    )
                })?;
            }
            "tab" => {
                if value.is_empty() {
                    return Err(format!(
                        "{}:{line_number}: tab path cannot be empty",
                        path.display()
                    ));
                }

                let candidate = PathBuf::from(value);
                if !tabs.iter().any(|existing| existing == &candidate) {
                    tabs.push(candidate);
                }
            }
            other => {
                return Err(format!(
                    "{}:{line_number}: unknown workspace record {other:?}",
                    path.display()
                ));
            }
        }
    }

    if tabs.is_empty() {
        tabs.push(fallback.to_path_buf());
        active_index = 0;
    }

    if active_index >= tabs.len() {
        return Err(format!(
            "{}: active tab index {active_index} is outside {} registered tabs",
            path.display(),
            tabs.len()
        ));
    }

    Ok((tabs, active_index))
}

fn save_registry(path: &Path, tabs: &[TabEntry], active_index: usize) -> Result<(), String> {
    let mut contents = format!("# Lexwright workspace\nactive\t{active_index}\n");

    for tab in tabs {
        let Some(path_text) = tab.path.to_str() else {
            return Err(format!(
                "cannot persist non-UTF-8 document path {}",
                tab.path.display()
            ));
        };

        if path_text.contains('\t') || path_text.contains('\n') || path_text.contains('\r') {
            return Err(format!(
                "cannot persist document path containing tab/newline: {}",
                tab.path.display()
            ));
        }

        contents.push_str("tab\t");
        contents.push_str(path_text);
        contents.push('\n');
    }

    atomic_write(path, contents.as_bytes())
        .map_err(|error| format!("could not save {}: {error}", path.display()))
}

fn default_workspace_path() -> PathBuf {
    if let Some(path) = nonempty_env("LEXWRIGHT_WORKSPACE") {
        return PathBuf::from(path);
    }

    default_data_root().join("workspace.tsv")
}

fn default_documents_dir() -> PathBuf {
    default_data_root().join("documents")
}

fn default_data_root() -> PathBuf {
    if let Some(data_home) = nonempty_env("XDG_DATA_HOME") {
        return PathBuf::from(data_home).join("lexwright");
    }

    if let Some(home) = nonempty_env("HOME") {
        return PathBuf::from(home).join(".local/share/lexwright");
    }

    PathBuf::from(".")
}

fn nonempty_env(name: &str) -> Option<std::ffi::OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

fn title_for_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("document")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::parse_registry;

    #[test]
    fn missing_tabs_fall_back_to_default_document() {
        let (tabs, active) = parse_registry(
            "# Lexwright workspace\nactive\t0\n",
            Path::new("workspace.tsv"),
            Path::new("/tmp/ledger.txt"),
        )
        .expect("workspace parse failed");

        assert_eq!(tabs, vec![Path::new("/tmp/ledger.txt").to_path_buf()]);
        assert_eq!(active, 0);
    }

    #[test]
    fn parses_active_tab_and_deduplicates_paths() {
        let (tabs, active) = parse_registry(
            "active\t1\ntab\t/tmp/a.txt\ntab\t/tmp/b.txt\ntab\t/tmp/a.txt\n",
            Path::new("workspace.tsv"),
            Path::new("/tmp/ledger.txt"),
        )
        .expect("workspace parse failed");

        assert_eq!(
            tabs,
            vec![
                Path::new("/tmp/a.txt").to_path_buf(),
                Path::new("/tmp/b.txt").to_path_buf()
            ]
        );
        assert_eq!(active, 1);
    }

    #[test]
    fn rejects_out_of_range_active_tab() {
        let error = parse_registry(
            "active\t2\ntab\t/tmp/a.txt\n",
            Path::new("workspace.tsv"),
            Path::new("/tmp/ledger.txt"),
        )
        .expect_err("invalid active tab unexpectedly parsed");

        assert!(error.contains("outside 1 registered tabs"));
    }
}
