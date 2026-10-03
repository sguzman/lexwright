use std::{any::TypeId, ops::Range};

use eframe::egui::{self, TextBuffer};

use crate::expansion::{ExpansionEngine, is_activation_char};

pub struct EditorBuffer {
    text: String,
    expansions: ExpansionEngine,
    expansions_applied: u64,
}

impl EditorBuffer {
    pub fn new(text: String) -> Self {
        Self {
            text,
            expansions: ExpansionEngine::load_default(),
            expansions_applied: 0,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
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

    fn maybe_expand_after_insert(
        &mut self,
        inserted: &str,
        char_index: egui::text::CharIndex,
        inserted_chars: usize,
    ) -> Option<usize> {
        let mut chars = inserted.chars();
        let activation = chars.next()?;

        if chars.next().is_some() || !is_activation_char(activation) {
            return None;
        }

        let boundary_byte =
            <String as TextBuffer>::byte_index_from_char_index(&self.text, char_index).0;
        let prefix = &self.text[..boundary_byte];

        let hit = self.expansions.find_suffix(prefix)?;
        debug_assert!(hit.replacement_chars >= hit.trigger_chars);

        let cursor_advance = inserted_chars + hit.replacement_chars - hit.trigger_chars;

        self.text
            .replace_range(hit.start_byte..boundary_byte, hit.replacement);
        self.expansions_applied = self.expansions_applied.saturating_add(1);

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
        let inserted_chars =
            <String as TextBuffer>::insert_text(&mut self.text, text, char_index);

        if !self.expansions.enabled() {
            return inserted_chars;
        }

        self.maybe_expand_after_insert(text, char_index, inserted_chars)
            .unwrap_or(inserted_chars)
    }

    fn delete_char_range(&mut self, char_range: Range<egui::text::CharIndex>) {
        <String as TextBuffer>::delete_char_range(&mut self.text, char_range);
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
}
