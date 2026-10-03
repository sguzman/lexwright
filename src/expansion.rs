use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process,
};

const DEFAULT_SET_NAME: &str = "default";

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpansionSet {
    pub name: String,
    pub note: String,
    pub starter_enabled: bool,
    pub user_rules: Vec<ExpansionRule>,
}

impl ExpansionSet {
    fn empty(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            note: String::new(),
            starter_enabled: true,
            user_rules: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExpansionConfig {
    pub active_set: String,
    pub sets: Vec<ExpansionSet>,
}

impl Default for ExpansionConfig {
    fn default() -> Self {
        Self {
            active_set: DEFAULT_SET_NAME.to_owned(),
            sets: vec![ExpansionSet::empty(DEFAULT_SET_NAME)],
        }
    }
}

impl ExpansionConfig {
    fn active(&self) -> &ExpansionSet {
        self.sets
            .iter()
            .find(|set| set.name == self.active_set)
            .expect("validated expansion config must contain active set")
    }

    fn active_mut(&mut self) -> &mut ExpansionSet {
        let active = self.active_set.clone();
        self.sets
            .iter_mut()
            .find(|set| set.name == active)
            .expect("validated expansion config must contain active set")
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

    pub fn active_set(&self) -> &ExpansionSet {
        self.config.active()
    }

    pub fn active_set_name(&self) -> &str {
        &self.config.active_set
    }

    pub fn set_names(&self) -> impl Iterator<Item = &str> {
        self.config.sets.iter().map(|set| set.name.as_str())
    }

    pub fn apply_active_set(
        &mut self,
        note: String,
        starter_enabled: bool,
        user_rules: Vec<ExpansionRule>,
    ) -> Result<(), String> {
        let mut next = self.config.clone();
        let active = next.active_mut();
        active.note = note;
        active.starter_enabled = starter_enabled;
        active.user_rules = user_rules;
        self.commit_config(next)
    }

    pub fn select_set(&mut self, name: &str) -> Result<(), String> {
        if self.config.active_set == name {
            return Ok(());
        }

        if !self.config.sets.iter().any(|set| set.name == name) {
            return Err(format!("unknown expansion ruleset {name:?}"));
        }

        let mut next = self.config.clone();
        next.active_set = name.to_owned();
        self.commit_config(next)
    }

    pub fn create_set_from_active(&mut self, name: &str) -> Result<(), String> {
        validate_set_name(name)?;

        if self.config.sets.iter().any(|set| set.name == name) {
            return Err(format!("ruleset {name:?} already exists"));
        }

        let source = self.config.active().clone();
        let mut next = self.config.clone();
        next.sets.push(ExpansionSet {
            name: name.to_owned(),
            note: source.note,
            starter_enabled: source.starter_enabled,
            user_rules: source.user_rules,
        });
        next.active_set = name.to_owned();
        self.commit_config(next)
    }

    pub fn rename_active_set(&mut self, name: &str) -> Result<String, String> {
        validate_set_name(name)?;

        let previous = self.config.active_set.clone();
        if previous == name {
            return Ok(previous);
        }

        if self.config.sets.iter().any(|set| set.name == name) {
            return Err(format!("ruleset {name:?} already exists"));
        }

        let mut next = self.config.clone();
        next.active_mut().name = name.to_owned();
        next.active_set = name.to_owned();
        self.commit_config(next)?;
        Ok(previous)
    }

    pub fn delete_active_set(&mut self) -> Result<String, String> {
        if self.config.sets.len() <= 1 {
            return Err("cannot delete the only expansion ruleset".to_owned());
        }

        let removed = self.config.active_set.clone();
        let mut next = self.config.clone();
        next.sets.retain(|set| set.name != removed);
        next.active_set = next
            .sets
            .first()
            .map(|set| set.name.clone())
            .ok_or_else(|| "expansion config unexpectedly has no rulesets".to_owned())?;

        self.commit_config(next)?;
        Ok(removed)
    }

    fn commit_config(&mut self, config: ExpansionConfig) -> Result<(), String> {
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

        let active = self.config.active().clone();

        if active.starter_enabled {
            for &(trigger, replacement) in STARTER_RULES {
                self.insert_rule(trigger, replacement);
            }
        }

        for rule in active.user_rules {
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
    text.chars().any(|ch| matches!(ch, '\t' | '\n' | '\r'))
}

fn validate_set_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("ruleset name cannot be empty".to_owned());
    }

    if has_tsv_control(name) {
        return Err("ruleset name cannot contain tabs or newlines".to_owned());
    }

    if name.starts_with('@') {
        return Err("ruleset name cannot start with '@'".to_owned());
    }

    Ok(())
}

fn validate_set(set: &ExpansionSet) -> Result<(), String> {
    validate_set_name(&set.name)?;

    if has_tsv_control(&set.note) {
        return Err(format!(
            "ruleset {:?}: note cannot contain tabs or newlines",
            set.name
        ));
    }

    let mut seen = HashSet::new();

    for (index, rule) in set.user_rules.iter().enumerate() {
        let line = index + 1;

        if rule.trigger.is_empty() {
            return Err(format!(
                "ruleset {:?}, rule {line}: trigger cannot be empty",
                set.name
            ));
        }

        if has_tsv_control(&rule.trigger) {
            return Err(format!(
                "ruleset {:?}, rule {line}: trigger cannot contain tabs or newlines",
                set.name
            ));
        }

        if rule.trigger.starts_with('@') {
            return Err(format!(
                "ruleset {:?}, rule {line}: triggers beginning with '@' are reserved",
                set.name
            ));
        }

        if has_tsv_control(&rule.replacement) {
            return Err(format!(
                "ruleset {:?}, rule {line}: replacement cannot contain tabs or newlines yet",
                set.name
            ));
        }

        if rule.replacement.chars().count() < rule.trigger.chars().count() {
            return Err(format!(
                "ruleset {:?}, rule {line}: expansion cannot be shorter than its trigger",
                set.name
            ));
        }

        if !seen.insert(rule.trigger.as_str()) {
            return Err(format!(
                "ruleset {:?}, rule {line}: duplicate trigger {:?}",
                set.name, rule.trigger
            ));
        }
    }

    Ok(())
}

fn validate_config(config: &ExpansionConfig) -> Result<(), String> {
    if config.sets.is_empty() {
        return Err("expansion config must contain at least one ruleset".to_owned());
    }

    let mut names = HashSet::new();
    for set in &config.sets {
        validate_set(set)?;
        if !names.insert(set.name.as_str()) {
            return Err(format!("duplicate expansion ruleset {:?}", set.name));
        }
    }

    if !config.sets.iter().any(|set| set.name == config.active_set) {
        return Err(format!(
            "active expansion ruleset {:?} does not exist",
            config.active_set
        ));
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

    parse_config(&contents, path)
}

fn parse_config(contents: &str, path: &Path) -> Result<ExpansionConfig, String> {
    let has_named_sets = contents.lines().any(|raw_line| {
        raw_line
            .split_once('\t')
            .is_some_and(|(key, _)| key == "@set")
    });

    if !has_named_sets {
        let mut set = ExpansionSet::empty(DEFAULT_SET_NAME);

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
                set.starter_enabled =
                    parse_bool_directive(path, line_number, "@starter", replacement)?;
                continue;
            }

            if trigger.starts_with('@') {
                return Err(format!(
                    "{}:{line_number}: unknown directive {trigger:?}",
                    path.display()
                ));
            }

            set.user_rules.push(ExpansionRule {
                trigger: trigger.to_owned(),
                replacement: replacement.to_owned(),
            });
        }

        let config = ExpansionConfig {
            active_set: DEFAULT_SET_NAME.to_owned(),
            sets: vec![set],
        };
        validate_config(&config).map_err(|error| format!("{}: {error}", path.display()))?;
        return Ok(config);
    }

    let mut active_set = None;
    let mut sets = Vec::<ExpansionSet>::new();
    let mut current_set = None::<usize>;

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
            "@active" => {
                if active_set.replace(value.to_owned()).is_some() {
                    return Err(format!(
                        "{}:{line_number}: duplicate @active directive",
                        path.display()
                    ));
                }
            }
            "@set" => {
                validate_set_name(value)
                    .map_err(|error| format!("{}:{line_number}: {error}", path.display()))?;
                sets.push(ExpansionSet::empty(value));
                current_set = Some(sets.len() - 1);
            }
            "@note" => {
                let Some(set_index) = current_set else {
                    return Err(format!(
                        "{}:{line_number}: @note must follow @set",
                        path.display()
                    ));
                };
                sets[set_index].note = value.to_owned();
            }
            "@starter" => {
                let Some(set_index) = current_set else {
                    return Err(format!(
                        "{}:{line_number}: @starter must follow @set",
                        path.display()
                    ));
                };
                sets[set_index].starter_enabled =
                    parse_bool_directive(path, line_number, "@starter", value)?;
            }
            directive if directive.starts_with('@') => {
                return Err(format!(
                    "{}:{line_number}: unknown directive {directive:?}",
                    path.display()
                ));
            }
            trigger => {
                let Some(set_index) = current_set else {
                    return Err(format!(
                        "{}:{line_number}: rule must follow @set",
                        path.display()
                    ));
                };
                sets[set_index].user_rules.push(ExpansionRule {
                    trigger: trigger.to_owned(),
                    replacement: value.to_owned(),
                });
            }
        }
    }

    let active_set = active_set.unwrap_or_else(|| {
        sets.first()
            .map(|set| set.name.clone())
            .unwrap_or_else(|| DEFAULT_SET_NAME.to_owned())
    });

    let config = ExpansionConfig { active_set, sets };
    validate_config(&config).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(config)
}

fn parse_bool_directive(
    path: &Path,
    line_number: usize,
    directive: &str,
    value: &str,
) -> Result<bool, String> {
    match value.trim() {
        "true" | "on" | "1" => Ok(true),
        "false" | "off" | "0" => Ok(false),
        other => Err(format!(
            "{}:{line_number}: {directive} must be true or false, got {other:?}",
            path.display()
        )),
    }
}

fn save_config(path: &Path, config: &ExpansionConfig) -> Result<(), String> {
    validate_config(config)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }

    let mut contents = String::from("# Lexwright expansion rulesets\n");
    contents.push_str("@active\t");
    contents.push_str(&config.active_set);
    contents.push('\n');

    for set in &config.sets {
        contents.push('\n');
        contents.push_str("@set\t");
        contents.push_str(&set.name);
        contents.push('\n');
        contents.push_str("@note\t");
        contents.push_str(&set.note);
        contents.push('\n');
        contents.push_str("@starter\t");
        contents.push_str(if set.starter_enabled {
            "true\n"
        } else {
            "false\n"
        });

        for rule in &set.user_rules {
            contents.push_str(&rule.trigger);
            contents.push('\t');
            contents.push_str(&rule.replacement);
            contents.push('\n');
        }
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
    use std::path::Path;

    use super::{
        DEFAULT_SET_NAME, ExpansionConfig, ExpansionEngine, ExpansionRule, ExpansionSet,
        STARTER_RULES, parse_config,
    };

    fn config_with_sets(active: &str, sets: &[(&str, &[(&str, &str)])]) -> ExpansionConfig {
        ExpansionConfig {
            active_set: active.to_owned(),
            sets: sets
                .iter()
                .map(|(name, rules)| ExpansionSet {
                    name: (*name).to_owned(),
                    note: String::new(),
                    starter_enabled: false,
                    user_rules: rules
                        .iter()
                        .map(|(trigger, replacement)| ExpansionRule {
                            trigger: (*trigger).to_owned(),
                            replacement: (*replacement).to_owned(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    fn engine(rules: &[(&str, &str)]) -> ExpansionEngine {
        let mut engine = ExpansionEngine {
            nodes: vec![Default::default()],
            enabled: true,
            rule_count: 0,
            config_path: "test.tsv".into(),
            config_error: None,
            config: config_with_sets(DEFAULT_SET_NAME, &[(DEFAULT_SET_NAME, rules)]),
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
                active_set: DEFAULT_SET_NAME.to_owned(),
                sets: vec![ExpansionSet {
                    name: DEFAULT_SET_NAME.to_owned(),
                    note: String::new(),
                    starter_enabled: true,
                    user_rules: vec![ExpansionRule {
                        trigger: "bc".to_owned(),
                        replacement: "big chicken".to_owned(),
                    }],
                }],
            },
        };
        engine.rebuild();

        let hit = engine.find_suffix("bc").expect("missing expansion");
        assert_eq!(hit.replacement, "big chicken");
        assert_eq!(engine.rule_count(), STARTER_RULES.len());
    }

    #[test]
    fn legacy_flat_config_migrates_to_default_set_in_memory() {
        let config = parse_config(
            "# old file\n@starter\tfalse\nbc\tbecause\n",
            Path::new("legacy.tsv"),
        )
        .expect("legacy config failed");

        assert_eq!(config.active_set, DEFAULT_SET_NAME);
        assert_eq!(config.sets.len(), 1);
        assert!(!config.sets[0].starter_enabled);
        assert_eq!(config.sets[0].user_rules[0].trigger, "bc");
    }

    #[test]
    fn parses_named_rulesets_and_active_selection() {
        let config = parse_config(
            "@active\tcompressed\n\n@set\tdefault\n@note\tbaseline\n@starter\ttrue\n\n@set\tcompressed\n@note\tshort forms\n@starter\tfalse\nbc\tbecause\n",
            Path::new("sets.tsv"),
        )
        .expect("named config failed");

        assert_eq!(config.active_set, "compressed");
        assert_eq!(config.sets.len(), 2);
        assert_eq!(config.active().note, "short forms");
        assert_eq!(config.active().user_rules[0].replacement, "because");
    }

    #[test]
    fn rebuild_uses_only_active_ruleset() {
        let mut engine = ExpansionEngine {
            nodes: vec![Default::default()],
            enabled: true,
            rule_count: 0,
            config_path: "test.tsv".into(),
            config_error: None,
            config: config_with_sets(
                "short",
                &[
                    ("long", &[("idk", "I do not know")]),
                    ("short", &[("idk", "I dunno")]),
                ],
            ),
        };
        engine.rebuild();

        let hit = engine.find_suffix("idk").expect("missing active expansion");
        assert_eq!(hit.replacement, "I dunno");
        assert_eq!(engine.rule_count(), 1);
    }
}
