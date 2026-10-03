use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
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

#[derive(Default)]
struct Node {
    children: HashMap<char, usize>,
    replacement: Option<Box<str>>,
    trigger_chars: usize,
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
}

impl ExpansionEngine {
    pub fn load_default() -> Self {
        let config_path = default_config_path();
        let mut rules: HashMap<String, String> = STARTER_RULES
            .iter()
            .map(|(trigger, replacement)| ((*trigger).to_owned(), (*replacement).to_owned()))
            .collect();

        let config_error = match load_user_rules(&config_path) {
            Ok(user_rules) => {
                for (trigger, replacement) in user_rules {
                    rules.insert(trigger, replacement);
                }
                None
            }
            Err(error) => Some(error),
        };

        Self::compile(rules, config_path, config_error)
    }

    fn compile(
        rules: HashMap<String, String>,
        config_path: PathBuf,
        config_error: Option<String>,
    ) -> Self {
        let mut nodes = vec![Node::default()];
        let mut rule_count = 0;

        for (trigger, replacement) in rules {
            if trigger.is_empty() {
                continue;
            }

            let trigger_chars = trigger.chars().count();
            let replacement_chars = replacement.chars().count();

            // The TextBuffer insertion contract can advance the cursor but cannot move it
            // backwards. Shortening transformations belong in a later transformation layer,
            // not the synchronous abbreviation engine.
            if replacement_chars < trigger_chars {
                continue;
            }

            let mut node_index = 0;

            for ch in trigger.chars().rev() {
                let next = if let Some(next) = nodes[node_index].children.get(&ch) {
                    *next
                } else {
                    let next = nodes.len();
                    nodes.push(Node::default());
                    nodes[node_index].children.insert(ch, next);
                    next
                };
                node_index = next;
            }

            if nodes[node_index].replacement.is_none() {
                rule_count += 1;
            }

            nodes[node_index].replacement = Some(replacement.into_boxed_str());
            nodes[node_index].trigger_chars = trigger_chars;
        }

        Self {
            nodes,
            enabled: true,
            rule_count,
            config_path,
            config_error,
        }
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
                    replacement_chars: replacement.chars().count(),
                });
            }
        }

        best
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

fn load_user_rules(path: &Path) -> Result<Vec<(String, String)>, String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!("could not read {}: {error}", path.display()));
        }
    };

    let mut rules = Vec::new();

    for (index, raw_line) in contents.lines().enumerate() {
        let line_number = index + 1;

        if raw_line.trim().is_empty() || raw_line.trim_start().starts_with('#') {
            continue;
        }

        let Some((trigger, replacement)) = raw_line.split_once('\t') else {
            return Err(format!(
                "{}:{line_number}: expected trigger<TAB>replacement",
                path.display()
            ));
        };

        if trigger.is_empty() {
            return Err(format!(
                "{}:{line_number}: trigger cannot be empty",
                path.display()
            ));
        }

        if replacement.chars().count() < trigger.chars().count() {
            return Err(format!(
                "{}:{line_number}: expansion cannot be shorter than its trigger",
                path.display()
            ));
        }

        rules.push((trigger.to_owned(), replacement.to_owned()));
    }

    Ok(rules)
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
    use super::ExpansionEngine;

    fn engine(rules: &[(&str, &str)]) -> ExpansionEngine {
        let rules = rules
            .iter()
            .map(|(trigger, replacement)| ((*trigger).to_owned(), (*replacement).to_owned()))
            .collect();

        ExpansionEngine::compile(rules, "test.tsv".into(), None)
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
    fn shortening_rules_are_not_compiled() {
        let engine = engine(&[("long", "x")]);
        assert_eq!(engine.rule_count(), 0);
        assert!(engine.find_suffix("long").is_none());
    }
}
