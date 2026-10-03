use std::{any::TypeId, ops::Range, time::Instant};

use eframe::egui::{self, TextBuffer};

use crate::{
    expansion::{ExpansionEngine, ExpansionRule, is_activation_char},
    metrics::TimingMetric,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditDelta {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_char: usize,
    pub old_end_char: usize,
    pub new_end_char: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct IndexStats {
    pub ascii_fast: u64,
    pub utf8_fallback: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExpansionSessionStats {
    pub hits: u64,
    pub trigger_chars: u64,
    pub output_chars: u64,
}

impl ExpansionSessionStats {
    pub fn avoided_chars(self) -> u64 {
        self.output_chars.saturating_sub(self.trigger_chars)
    }

    pub fn typed_percent(self) -> f64 {
        if self.output_chars == 0 {
            0.0
        } else {
            self.trigger_chars as f64 * 100.0 / self.output_chars as f64
        }
    }
}

pub struct EditorBuffer {
    text: String,
    ascii_only: bool,
    expansions: ExpansionEngine,
    expansions_applied: u64,
    expansion_stats: Vec<(String, ExpansionSessionStats)>,
    expansion_rule_stats: Vec<(String, String, ExpansionSessionStats)>,
    insert_timing: TimingMetric,
    expansion_lookup_timing: TimingMetric,
    index_stats: IndexStats,
    pending_edits: Vec<EditDelta>,
}

impl EditorBuffer {
    pub fn new(text: String) -> Self {
        let ascii_only = text.is_ascii();

        Self {
            text,
            ascii_only,
            expansions: ExpansionEngine::load_default(),
            expansions_applied: 0,
            expansion_stats: Vec::new(),
            expansion_rule_stats: Vec::new(),
            insert_timing: TimingMetric::default(),
            expansion_lookup_timing: TimingMetric::default(),
            index_stats: IndexStats::default(),
            pending_edits: Vec::new(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn take_edit_deltas(&mut self) -> Vec<EditDelta> {
        std::mem::take(&mut self.pending_edits)
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
        let old_end_char = self.text[..range.end].chars().count();
        let replacement_chars = replacement.chars().count();
        let new_end_byte = range.start.saturating_add(replacement.len());
        let new_end_char = start_char.saturating_add(replacement_chars);

        self.text.replace_range(range.clone(), replacement);
        self.pending_edits.push(EditDelta {
            start_byte: range.start,
            old_end_byte: range.end,
            new_end_byte,
            start_char,
            old_end_char,
            new_end_char,
        });

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

    pub fn active_expansion_stats(&self) -> ExpansionSessionStats {
        let active = self.expansions.active_set_name();
        self.expansion_stats
            .iter()
            .find(|(name, _)| name == active)
            .map_or_else(ExpansionSessionStats::default, |(_, stats)| *stats)
    }

    pub fn expansion_stats_by_set(&self) -> Vec<(String, ExpansionSessionStats)> {
        self.expansions
            .set_names()
            .map(|name| {
                let stats = self
                    .expansion_stats
                    .iter()
                    .find(|(candidate, _)| candidate == name)
                    .map_or_else(ExpansionSessionStats::default, |(_, stats)| *stats);
                (name.to_owned(), stats)
            })
            .collect()
    }

    pub fn active_expansion_rule_stats(&self, trigger: &str) -> ExpansionSessionStats {
        let active = self.expansions.active_set_name();
        self.expansion_rule_stats
            .iter()
            .find(|(set, candidate, _)| set == active && candidate == trigger)
            .map_or_else(ExpansionSessionStats::default, |(_, _, stats)| *stats)
    }

    pub fn expansion_config_path(&self) -> &std::path::Path {
        self.expansions.config_path()
    }

    pub fn expansion_config_error(&self) -> Option<&str> {
        self.expansions.config_error()
    }

    pub fn expansion_active_set_name(&self) -> &str {
        self.expansions.active_set_name()
    }

    pub fn expansion_set_names(&self) -> Vec<String> {
        self.expansions.set_names().map(ToOwned::to_owned).collect()
    }

    pub fn expansion_starter_enabled(&self) -> bool {
        self.expansions.active_set().starter_enabled
    }

    pub fn expansion_user_rules(&self) -> &[ExpansionRule] {
        &self.expansions.active_set().user_rules
    }

    pub fn apply_expansion_config(
        &mut self,
        starter_enabled: bool,
        user_rules: Vec<ExpansionRule>,
    ) -> Result<(), String> {
        let active = self.expansions.active_set_name().to_owned();
        self.expansions
            .apply_active_set(starter_enabled, user_rules)?;
        self.expansion_stats.retain(|(name, _)| name != &active);
        self.expansion_rule_stats
            .retain(|(set, _, _)| set != &active);
        Ok(())
    }

    pub fn select_expansion_set(&mut self, name: &str) -> Result<(), String> {
        self.expansions.select_set(name)
    }

    pub fn create_expansion_set_from_active(&mut self, name: &str) -> Result<(), String> {
        self.expansions.create_set_from_active(name)?;
        self.expansion_stats
            .retain(|(candidate, _)| candidate != name);
        self.expansion_rule_stats.retain(|(set, _, _)| set != name);
        Ok(())
    }

    pub fn delete_active_expansion_set(&mut self) -> Result<String, String> {
        let removed = self.expansions.delete_active_set()?;
        self.expansion_stats.retain(|(name, _)| name != &removed);
        self.expansion_rule_stats
            .retain(|(set, _, _)| set != &removed);
        Ok(removed)
    }

    pub fn reset_active_expansion_stats(&mut self) {
        let active = self.expansions.active_set_name().to_owned();
        self.expansion_stats.retain(|(name, _)| name != &active);
        self.expansion_rule_stats
            .retain(|(set, _, _)| set != &active);
    }

    fn byte_index_for_char(&mut self, char_index: egui::text::CharIndex) -> usize {
        if self.ascii_only {
            self.index_stats.ascii_fast = self.index_stats.ascii_fast.saturating_add(1);

            // Match egui/String TextBuffer semantics exactly: cursor/delete machinery may
            // transiently ask for a character index past EOF, and the stock implementation
            // clamps that to text.len(). The ASCII fast path must do the same.
            char_index.0.min(self.text.len())
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
    ) -> Option<(usize, usize, usize, usize, usize)> {
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

        let expansion_start = hit.start_byte;
        let trigger_chars = hit.trigger_chars;
        let replacement_bytes = hit.replacement.len();
        let replacement_chars = hit.replacement_chars;

        // Observe only successful expansions. Repeated hits allocate nothing for
        // telemetry; set/trigger keys are owned only on their first hit.
        let active_set = self.expansions.active_set_name();
        let trigger = &self.text[expansion_start..boundary_byte];

        if let Some((_, stats)) = self
            .expansion_stats
            .iter_mut()
            .find(|(name, _)| name == active_set)
        {
            stats.hits = stats.hits.saturating_add(1);
            stats.trigger_chars = stats.trigger_chars.saturating_add(trigger_chars as u64);
            stats.output_chars = stats.output_chars.saturating_add(replacement_chars as u64);
        } else {
            self.expansion_stats.push((
                active_set.to_owned(),
                ExpansionSessionStats {
                    hits: 1,
                    trigger_chars: trigger_chars as u64,
                    output_chars: replacement_chars as u64,
                },
            ));
        }

        if let Some((_, _, stats)) = self
            .expansion_rule_stats
            .iter_mut()
            .find(|(set, candidate, _)| set == active_set && candidate == trigger)
        {
            stats.hits = stats.hits.saturating_add(1);
            stats.trigger_chars = stats.trigger_chars.saturating_add(trigger_chars as u64);
            stats.output_chars = stats.output_chars.saturating_add(replacement_chars as u64);
        } else {
            self.expansion_rule_stats.push((
                active_set.to_owned(),
                trigger.to_owned(),
                ExpansionSessionStats {
                    hits: 1,
                    trigger_chars: trigger_chars as u64,
                    output_chars: replacement_chars as u64,
                },
            ));
        }

        self.text
            .replace_range(expansion_start..boundary_byte, hit.replacement);
        self.expansions_applied = self.expansions_applied.saturating_add(1);
        self.expansion_lookup_timing.observe(lookup_elapsed);

        Some((
            cursor_advance,
            expansion_start,
            trigger_chars,
            replacement_bytes,
            replacement_chars,
        ))
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
            if let Some((
                cursor_advance,
                expansion_start,
                trigger_chars,
                replacement_bytes,
                replacement_chars,
            )) = self.maybe_expand_after_insert(text, byte_index, inserted_chars)
            {
                let start_char = char_index.0.saturating_sub(trigger_chars);
                self.pending_edits.push(EditDelta {
                    start_byte: expansion_start,
                    old_end_byte: byte_index,
                    new_end_byte: expansion_start
                        .saturating_add(replacement_bytes)
                        .saturating_add(text.len()),
                    start_char,
                    old_end_char: char_index.0,
                    new_end_char: start_char
                        .saturating_add(replacement_chars)
                        .saturating_add(inserted_chars),
                });
                cursor_advance
            } else {
                self.pending_edits.push(EditDelta {
                    start_byte: byte_index,
                    old_end_byte: byte_index,
                    new_end_byte: byte_index.saturating_add(text.len()),
                    start_char: char_index.0,
                    old_end_char: char_index.0,
                    new_end_char: char_index.0.saturating_add(inserted_chars),
                });
                inserted_chars
            }
        } else {
            self.pending_edits.push(EditDelta {
                start_byte: byte_index,
                old_end_byte: byte_index,
                new_end_byte: byte_index.saturating_add(text.len()),
                start_char: char_index.0,
                old_end_char: char_index.0,
                new_end_char: char_index.0.saturating_add(inserted_chars),
            });
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
        self.pending_edits.push(EditDelta {
            start_byte: start,
            old_end_byte: end,
            new_end_byte: start,
            start_char: char_range.start.0,
            old_end_char: char_range.end.0,
            new_end_char: char_range.start.0,
        });

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

    use super::{EditorBuffer, ExpansionSessionStats};

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
    fn saving_active_rules_resets_that_rulesets_session_stats() {
        let mut buffer = EditorBuffer::new(String::new());

        buffer.insert_text("b", CharIndex(0));
        buffer.insert_text("c", CharIndex(1));
        buffer.insert_text(" ", CharIndex(2));
        assert_eq!(buffer.active_expansion_stats().hits, 1);

        buffer
            .apply_expansion_config(true, Vec::new())
            .expect("save failed");
        assert_eq!(buffer.active_expansion_stats().hits, 0);
        assert_eq!(buffer.active_expansion_rule_stats("bc").hits, 0);
    }

    #[test]
    fn expansion_tracks_current_ruleset_compression_stats() {
        let mut buffer = EditorBuffer::new(String::new());

        buffer.insert_text("b", CharIndex(0));
        buffer.insert_text("c", CharIndex(1));
        buffer.insert_text(" ", CharIndex(2));

        let stats = buffer.active_expansion_stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.trigger_chars, 2);
        assert_eq!(stats.output_chars, 7);
        assert_eq!(stats.avoided_chars(), 5);
        assert!((stats.typed_percent() - 28.571).abs() < 0.01);

        let rule_stats = buffer.active_expansion_rule_stats("bc");
        assert_eq!(rule_stats.hits, 1);
        assert_eq!(rule_stats.avoided_chars(), 5);
        assert_eq!(
            buffer.active_expansion_rule_stats("unused"),
            ExpansionSessionStats::default()
        );
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
    fn records_exact_insert_and_delete_deltas() {
        let mut buffer = EditorBuffer::new("abc".to_owned());
        buffer.insert_text("X", CharIndex(1));
        let edits = buffer.take_edit_deltas();
        assert_eq!(
            edits,
            vec![super::EditDelta {
                start_byte: 1,
                old_end_byte: 1,
                new_end_byte: 2,
                start_char: 1,
                old_end_char: 1,
                new_end_char: 2,
            }]
        );

        buffer.delete_char_range(CharIndex(1)..CharIndex(2));
        let edits = buffer.take_edit_deltas();
        assert_eq!(
            edits,
            vec![super::EditDelta {
                start_byte: 1,
                old_end_byte: 2,
                new_end_byte: 1,
                start_char: 1,
                old_end_char: 2,
                new_end_char: 1,
            }]
        );
    }

    #[test]
    fn ascii_fast_path_clamps_one_past_end_like_stock_text_buffer() {
        let mut buffer = EditorBuffer::new("abc".to_owned());

        buffer.insert_text("X", CharIndex(4));
        assert_eq!(buffer.text(), "abcX");

        buffer.delete_char_range(CharIndex(3)..CharIndex(5));
        assert_eq!(buffer.text(), "abc");
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
