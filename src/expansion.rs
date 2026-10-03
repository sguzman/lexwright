use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process,
};

const STARTER_RULES: &[(&str, &str)] = &[
    ("abt", "about"),
    ("bc", "because"),
    ("ppl", "people"),
    ("prob", "probably"),
    ("rly", "really"),
    ("shd", "should"),
    ("smth", "something"),
    ("teh", "the"),
    ("wld", "would"),
    ("woudl", "would"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpansionRule {
    pub trigger: String,
    pub replacement: String,
}

#[derive(Clone, Debug)]
pub struct ExpansionConfig {
    pub starter_enabled: bool,
    pub user_rules: Vec<ExpansionRule>,
}

impl Default for ExpansionConfig {
    fn default() -> Self {
        Self {
            starter_enabled: true,
            user_rules: Vec::new(),
        }
    }
}

#[derive(Default)]
struct Node {
    children: HashMap<char, usize>,
    replacement: Option<Box<str>>,
    trigger_chars: usize,
    replacement_chars: usize,
}

pub struct ExpansionMatch<'a> {
    pub start_byte: usize,
    pub trigger_chars: usize,
    pub replacement: &'a str,
    pub replacement_chars: usize,
}

pub struct ExpansionEngine {
    nodes: Vec<Node>,
    enabled: bool,
    rule_count: usize,
    config_path: PathBuf,
    config_error: Option<String>,
    config: ExpansionConfig,
}

impl ExpansionEngine {
    pub fn load_default() -> Self {
        let config_path = default_config_path();
        let (config, config_error) = match load_config(&config_path) {
            Ok(config) => (config, None),
            Err(error) => (ExpansionConfig::default(), Some(error)),
        };

        let mut engine = Self {
            nodes: vec![Node::default()],
            enabled: true,
            rule_count: 0,
            config_path,
            config_error,
            config,
        };
        engine.rebuild();
        engine
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn rule_count(&self) -> usize {
        self.rule_count
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn config_error(&self) -> Option<&str> {
        self.config_error.as_deref()
    }

    pub fn config(&self) -> &ExpansionConfig {
        &self.config
    }

    pub fn apply_config(&mut self, config: ExpansionConfig) -> Result<(), String> {
        validate_config(&config)?;

        if let Err(error) = save_config(&self.config_path, &config) {
            self.config_error = Some(error.clone());
            return Err(error);
        }

        self.config = config;
        self.config_error = None;
        self.rebuild();
        Ok(())
    }

    pub fn find_suffix<'a>(&'a self, prefix: &str) -> Option<ExpansionMatch<'a>> {
        if !self.enabled {
            return None;
        }

        let mut node_index = 0;
        let mut best = None;

        for (byte_index, ch) in prefix.char_indices().rev() {
            let Some(next) = self.nodes[node_index].children.get(&ch) else {
                break;
            };

            node_index = *next;
            let node = &self.nodes[node_index];

            if let Some(replacement) = node.replacement.as_deref()
                && has_left_boundary(prefix, byte_index)
            {
                best = Some(ExpansionMatch {
                    start_byte: byte_index,
                    trigger_chars: node.trigger_chars,
                    replacement,
                    replacement_chars: node.replacement_chars,
                });
            }
        }

        best
    }

    fn rebuild(&mut self) {
        self.nodes.clear();
        self.nodes.push(Node::default());
        self.rule_count = 0;

        if self.config.starter_enabled {
            for &(trigger, replacement) in STARTER_RULES {
                self.insert_rule(trigger, replacement);
            }
        }

        let user_rules = self.config.user_rules.clone();
        for rule in user_rules {
            self.insert_rule(&rule.trigger, &rule.replacement);
        }
    }

    fn insert_rule(&mut self, trigger: &str, replacement: &str) {
        let trigger_chars = trigger.chars().count();
        let replacement_chars = replacement.chars().count();
        let mut node_index = 0;

        for ch in trigger.chars().rev() {
            let next = if let Some(next) = self.nodes[node_index].children.get(&ch) {
                *next
            } else {
                let next = self.nodes.len();
                self.nodes.push(Node::default());
                self.nodes[node_index].children.insert(ch, next);
                next
            };
            node_index = next;
        }

        if self.nodes[node_index].replacement.is_none() {
            self.rule_count += 1;
        }

        let node = &mut self.nodes[node_index];
        node.replacement = Some(replacement.to_owned().into_boxed_str());
        node.trigger_chars = trigger_chars;
        node.replacement_chars = replacement_chars;
    }
}

pub fn is_activation_char(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '.' | ',' | '!' | '?' | ';' | ':' | ')' | ']' | '}' | '…'
        )
}

fn has_left_boundary(prefix: &str, start_byte: usize) -> bool {
    prefix[..start_byte]
        .chars()
        .next_back()
        .is_none_or(|ch| !is_word_char(ch))
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '\'')
}

fn has_tsv_control(text: &str) -> bool {
    text.chars()
        .any(|ch| matches!(ch, '\t' | '\n' | '\r'))
}

fn validate_config(config: &ExpansionConfig) -> Result<(), String> {
    let mut seen = HashSet::new();

    for (index, rule) in config.user_rules.iter().enumerate() {
        let line = index + 1;

        if rule.trigger.is_empty() {
            return Err(format!("rule {line}: trigger cannot be empty"));
        }

        if has_tsv_control(&rule.trigger) {
            return Err(format!("rule {line}: trigger cannot contain tabs or newlines"));
        }

        if has_tsv_control(&rule.replacement) {
            return Err(format!(
                "rule {line}: replacement cannot contain tabs or newlines yet"
            ));
        }

        if rule.replacement.chars().count() < rule.trigger.chars().count() {
            return Err(format!(
                "rule {line}: expansion cannot be shorter than its trigger"
            ));
        }

        if !seen.insert(rule.trigger.as_str()) {
            return Err(format!("rule {line}: duplicate trigger {:?}", rule.trigger));
        }
    }

    Ok(())
}

fn load_config(path: &Path) -> Result<ExpansionConfig, String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ExpansionConfig::default());
        }
        Err(error) => {
            return Err(format!("could not read {}: {error}", path.display()));
        }
    };

    let mut config = ExpansionConfig::default();

    for (index, raw_line) in contents.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = raw_line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some((trigger, replacement)) = raw_line.split_once('\t') else {
            return Err(format!(
                "{}:{line_number}: expected trigger<TAB>replacement",
                path.display()
            ));
        };

        if trigger == "@starter" {
            config.starter_enabled = match replacement.trim() {
                "true" | "on" | "1" => true,
                "false" | "off" | "0" => false,
                other => {
                    return Err(format!(
                        "{}:{line_number}: @starter must be true or false, got {other:?}",
                        path.display()
                    ));
                }
            };
            continue;
        }

        config.user_rules.push(ExpansionRule {
            trigger: trigger.to_owned(),
            replacement: replacement.to_owned(),
        });
    }

    validate_config(&config).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(config)
}

fn save_config(path: &Path, config: &ExpansionConfig) -> Result<(), String> {
    validate_config(config)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }

    let mut contents = String::from("# Lexwright expansion rules\n");
    contents.push_str("@starter\t");
    contents.push_str(if config.starter_enabled {
        "true\n"
    } else {
        "false\n"
    });

    for rule in &config.user_rules {
        contents.push_str(&rule.trigger);
        contents.push('\t');
        contents.push_str(&rule.replacement);
        contents.push('\n');
    }

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

fn default_config_path() -> PathBuf {
    if let Some(path) = nonempty_env("LEXWRIGHT_EXPANSIONS") {
        return PathBuf::from(path);
    }

    if let Some(config_home) = nonempty_env("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home).join("lexwright/expansions.tsv");
    }

    if let Some(home) = nonempty_env("HOME") {
        return PathBuf::from(home).join(".config/lexwright/expansions.tsv");
    }

    PathBuf::from("lexwright-expansions.tsv")
}

fn nonempty_env(name: &str) -> Option<std::ffi::OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{ExpansionConfig, ExpansionEngine, ExpansionRule, STARTER_RULES};

    fn engine(rules: &[(&str, &str)]) -> ExpansionEngine {
        let mut engine = ExpansionEngine {
            nodes: vec![Default::default()],
            enabled: true,
            rule_count: 0,
            config_path: "test.tsv".into(),
            config_error: None,
            config: ExpansionConfig {
                starter_enabled: false,
                user_rules: rules
                    .iter()
                    .map(|(trigger, replacement)| ExpansionRule {
                        trigger: (*trigger).to_owned(),
                        replacement: (*replacement).to_owned(),
                    })
                    .collect(),
            },
        };
        engine.rebuild();
        engine
    }

    #[test]
    fn finds_exact_suffix_at_word_boundary() {
        let engine = engine(&[("bc", "because")]);
        let hit = engine.find_suffix("say bc").expect("missing expansion");

        assert_eq!(hit.start_byte, 4);
        assert_eq!(hit.trigger_chars, 2);
        assert_eq!(hit.replacement, "because");
    }

    #[test]
    fn does_not_expand_inside_larger_word() {
        let engine = engine(&[("bc", "because")]);
        assert!(engine.find_suffix("abc").is_none());
    }

    #[test]
    fn longest_matching_trigger_wins() {
        let engine = engine(&[("x", "short"), ("xx", "long")]);
        let hit = engine.find_suffix("xx").expect("missing expansion");
        assert_eq!(hit.replacement, "long");
        assert_eq!(hit.trigger_chars, 2);
    }

    #[test]
    fn user_rule_overrides_starter_rule() {
        let mut engine = ExpansionEngine {
            nodes: vec![Default::default()],
            enabled: true,
            rule_count: 0,
            config_path: "test.tsv".into(),
            config_error: None,
            config: ExpansionConfig {
                starter_enabled: true,
                user_rules: vec![ExpansionRule {
                    trigger: "bc".to_owned(),
                    replacement: "big chicken".to_owned(),
                }],
            },
        };
        engine.rebuild();

        let hit = engine.find_suffix("bc").expect("missing expansion");
        assert_eq!(hit.replacement, "big chicken");
        assert_eq!(engine.rule_count(), STARTER_RULES.len());
    }
}
