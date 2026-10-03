use std::{any::TypeId, ops::Range, time::Instant};

use eframe::egui::{self, TextBuffer};

use crate::{
    expansion::{ExpansionConfig, ExpansionEngine, ExpansionRule, is_activation_char},
    metrics::TimingMetric,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct IndexStats {
    pub ascii_fast: u64,
    pub utf8_fallback: u64,
}

pub struct EditorBuffer {
    text: String,
    ascii_only: bool,
    expansions: ExpansionEngine,
    expansions_applied: u64,
    insert_timing: TimingMetric,
    expansion_lookup_timing: TimingMetric,
    index_stats: IndexStats,
}

impl EditorBuffer {
    pub fn new(text: String) -> Self {
        let ascii_only = text.is_ascii();

        Self {
            text,
            ascii_only,
            expansions: ExpansionEngine::load_default(),
            expansions_applied: 0,
            insert_timing: TimingMetric::default(),
            expansion_lookup_timing: TimingMetric::default(),
            index_stats: IndexStats::default(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn replace_byte_range(
        &mut self,
        range: Range<usize>,
        replacement: &str,
    ) -> Result<usize, String> {
        if range.start > range.end || range.end > self.text.len() {
            return Err("edit range is outside the current ledger".to_owned());
        }

        if !self.text.is_char_boundary(range.start) || !self.text.is_char_boundary(range.end) {
            return Err("edit range does not land on UTF-8 character boundaries".to_owned());
        }

        let start_char = self.text[..range.start].chars().count();
        let replacement_chars = replacement.chars().count();

        self.text.replace_range(range, replacement);

        if self.ascii_only && !replacement.is_ascii() {
            self.ascii_only = false;
        }

        Ok(start_char.saturating_add(replacement_chars))
    }

    pub fn is_ascii_fast_path(&self) -> bool {
        self.ascii_only
    }

    pub fn insert_timing(&self) -> TimingMetric {
        self.insert_timing
    }

    pub fn expansion_lookup_timing(&self) -> TimingMetric {
        self.expansion_lookup_timing
    }

    pub fn index_stats(&self) -> IndexStats {
        self.index_stats
    }

    pub fn expansions_enabled(&self) -> bool {
        self.expansions.enabled()
    }

    pub fn set_expansions_enabled(&mut self, enabled: bool) {
        self.expansions.set_enabled(enabled);
    }

    pub fn expansion_rule_count(&self) -> usize {
        self.expansions.rule_count()
    }

    pub fn expansion_hits(&self) -> u64 {
        self.expansions_applied
    }

    pub fn expansion_config_path(&self) -> &std::path::Path {
        self.expansions.config_path()
    }

    pub fn expansion_config_error(&self) -> Option<&str> {
        self.expansions.config_error()
    }

    pub fn expansion_starter_enabled(&self) -> bool {
        self.expansions.config().starter_enabled
    }

    pub fn expansion_user_rules(&self) -> &[ExpansionRule] {
        &self.expansions.config().user_rules
    }

    pub fn apply_expansion_config(
        &mut self,
        starter_enabled: bool,
        user_rules: Vec<ExpansionRule>,
    ) -> Result<(), String> {
        self.expansions.apply_config(ExpansionConfig {
            starter_enabled,
            user_rules,
        })
    }

    fn byte_index_for_char(&mut self, char_index: egui::text::CharIndex) -> usize {
        if self.ascii_only {
            self.index_stats.ascii_fast = self.index_stats.ascii_fast.saturating_add(1);
            debug_assert!(char_index.0 <= self.text.len());
            char_index.0
        } else {
            self.index_stats.utf8_fallback = self.index_stats.utf8_fallback.saturating_add(1);
            <String as TextBuffer>::byte_index_from_char_index(&self.text, char_index).0
        }
    }

    fn maybe_expand_after_insert(
        &mut self,
        inserted: &str,
        boundary_byte: usize,
        inserted_chars: usize,
    ) -> Option<usize> {
        let mut chars = inserted.chars();
        let activation = chars.next()?;

        if chars.next().is_some() || !is_activation_char(activation) {
            return None;
        }

        let prefix = &self.text[..boundary_byte];
        let lookup_started = Instant::now();
        let hit = self.expansions.find_suffix(prefix);
        let lookup_elapsed = lookup_started.elapsed();

        let Some(hit) = hit else {
            self.expansion_lookup_timing.observe(lookup_elapsed);
            return None;
        };

        debug_assert!(hit.replacement_chars >= hit.trigger_chars);
        let cursor_advance = inserted_chars + hit.replacement_chars - hit.trigger_chars;

        if self.ascii_only && !hit.replacement.is_ascii() {
            self.ascii_only = false;
        }

        self.text
            .replace_range(hit.start_byte..boundary_byte, hit.replacement);
        self.expansions_applied = self.expansions_applied.saturating_add(1);
        self.expansion_lookup_timing.observe(lookup_elapsed);

        Some(cursor_advance)
    }
}

impl TextBuffer for EditorBuffer {
    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        &self.text
    }

    fn insert_text(&mut self, text: &str, char_index: egui::text::CharIndex) -> usize {
        let started = Instant::now();
        let byte_index = self.byte_index_for_char(char_index);
        let text_is_ascii = text.is_ascii();
        let inserted_chars = if text_is_ascii {
            text.len()
        } else {
            text.chars().count()
        };

        self.text.insert_str(byte_index, text);

        if self.ascii_only && !text_is_ascii {
            self.ascii_only = false;
        }

        let cursor_advance = if self.expansions.enabled() {
            self.maybe_expand_after_insert(text, byte_index, inserted_chars)
                .unwrap_or(inserted_chars)
        } else {
            inserted_chars
        };

        self.insert_timing.observe(started.elapsed());
        cursor_advance
    }

    fn delete_char_range(&mut self, char_range: Range<egui::text::CharIndex>) {
        if char_range.is_empty() {
            return;
        }

        let start = self.byte_index_for_char(char_range.start);
        let end = self.byte_index_for_char(char_range.end);
        self.text.replace_range(start..end, "");

        // Once Unicode has entered a document, conservatively keep using the UTF-8
        // fallback even if a deletion happened to remove the final non-ASCII codepoint.
        // This avoids an O(document) is_ascii() rescan on the edit path.
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{TextBuffer, text::CharIndex};

    use super::EditorBuffer;

    #[test]
    fn expands_when_space_is_typed_and_advances_cursor() {
        let mut buffer = EditorBuffer::new(String::new());

        assert_eq!(buffer.insert_text("b", CharIndex(0)), 1);
        assert_eq!(buffer.insert_text("c", CharIndex(1)), 1);
        assert_eq!(buffer.insert_text(" ", CharIndex(2)), 6);

        assert_eq!(buffer.text(), "because ");
        assert_eq!(buffer.expansion_hits(), 1);
    }

    #[test]
    fn expansion_can_be_disabled() {
        let mut buffer = EditorBuffer::new("bc".to_owned());
        buffer.set_expansions_enabled(false);

        assert_eq!(buffer.insert_text(" ", CharIndex(2)), 1);
        assert_eq!(buffer.text(), "bc ");
    }

    #[test]
    fn punctuation_activates_expansion() {
        let mut buffer = EditorBuffer::new("wld".to_owned());

        assert_eq!(buffer.insert_text(",", CharIndex(3)), 3);
        assert_eq!(buffer.text(), "would,");
    }

    #[test]
    fn external_byte_range_edit_returns_new_character_cursor() {
        let mut buffer = EditorBuffer::new("hello world".to_owned());
        let cursor = buffer
            .replace_byte_range(6..11, "Lexwright")
            .expect("external edit failed");

        assert_eq!(buffer.text(), "hello Lexwright");
        assert_eq!(cursor, 15);
    }

    #[test]
    fn external_byte_range_edit_respects_utf8_boundaries() {
        let mut buffer = EditorBuffer::new("café".to_owned());

        assert!(buffer.replace_byte_range(4..5, "x").is_err());

        let cursor = buffer
            .replace_byte_range(3..5, "e")
            .expect("valid utf8 edit failed");
        assert_eq!(buffer.text(), "cafe");
        assert_eq!(cursor, 4);
    }

    #[test]
    fn ascii_documents_use_constant_time_index_mapping() {
        let mut buffer = EditorBuffer::new("hello".to_owned());
        buffer.insert_text("!", CharIndex(5));

        let stats = buffer.index_stats();
        assert_eq!(stats.ascii_fast, 1);
        assert_eq!(stats.utf8_fallback, 0);
        assert!(buffer.is_ascii_fast_path());
    }

    #[test]
    fn unicode_switches_to_utf8_index_mapping() {
        let mut buffer = EditorBuffer::new("café".to_owned());
        buffer.insert_text("!", CharIndex(4));

        let stats = buffer.index_stats();
        assert_eq!(stats.ascii_fast, 0);
        assert_eq!(stats.utf8_fallback, 1);
        assert!(!buffer.is_ascii_fast_path());
        assert_eq!(buffer.text(), "café!");
    }

    #[test]
    fn inserting_unicode_disables_ascii_fast_path_without_rescanning() {
        let mut buffer = EditorBuffer::new("cafe".to_owned());
        buffer.insert_text("é", CharIndex(4));
        buffer.insert_text("!", CharIndex(5));

        let stats = buffer.index_stats();
        assert_eq!(stats.ascii_fast, 1);
        assert_eq!(stats.utf8_fallback, 1);
        assert!(!buffer.is_ascii_fast_path());
    }
}
