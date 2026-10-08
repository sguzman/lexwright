use eframe::egui;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VimMode {
    #[default]
    Insert,
    Nav,
}

impl VimMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Insert => "INS",
            Self::Nav => "NAV",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavCommand {
    Left,
    Down,
    Up,
    Right,
    WordNext,
    WordPrevious,
    RowStart,
    RowEnd,
    DocumentStart,
    DocumentEnd,
}

#[derive(Default)]
pub struct VimLite {
    mode: VimMode,
    pending_g: bool,
    vertical_x: Option<f32>,
}

impl VimLite {
    pub fn mode(&self) -> VimMode {
        self.mode
    }

    pub fn reset(&mut self) {
        self.mode = VimMode::Insert;
        self.pending_g = false;
        self.vertical_x = None;
    }

    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            VimMode::Insert => VimMode::Nav,
            VimMode::Nav => VimMode::Insert,
        };
        self.pending_g = false;
        self.vertical_x = None;
    }

    pub fn capture(
        &mut self,
        ui: &mut egui::Ui,
        editor_id: egui::Id,
        enabled: bool,
    ) -> Option<NavCommand> {
        if !enabled {
            self.reset();
            return None;
        }

        if !ui.memory(|memory| memory.has_focus(editor_id)) {
            self.pending_g = false;
            return None;
        }

        if ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::I)) {
            self.toggle_mode();
            self.pending_g = false;
            self.vertical_x = None;
            ui.ctx().request_repaint();
            return None;
        }

        if self.mode == VimMode::Insert {
            return None;
        }

        // NAV is intentionally non-mutating. Remove text/IME/paste/cut input and
        // mutation shortcuts before TextEdit sees this frame.
        ui.input_mut(|input| {
            let command = if input.consume_key(egui::Modifiers::SHIFT, egui::Key::G) {
                self.pending_g = false;
                Some(NavCommand::DocumentEnd)
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::G) {
                if self.pending_g {
                    self.pending_g = false;
                    Some(NavCommand::DocumentStart)
                } else {
                    self.pending_g = true;
                    None
                }
            } else {
                let command = if input.consume_key(egui::Modifiers::NONE, egui::Key::H) {
                    Some(NavCommand::Left)
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::J) {
                    Some(NavCommand::Down)
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::K) {
                    Some(NavCommand::Up)
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::L) {
                    Some(NavCommand::Right)
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::W) {
                    Some(NavCommand::WordNext)
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::B) {
                    Some(NavCommand::WordPrevious)
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::Num0) {
                    Some(NavCommand::RowStart)
                } else if input.consume_key(egui::Modifiers::SHIFT, egui::Key::Num4) {
                    Some(NavCommand::RowEnd)
                } else {
                    None
                };

                if command.is_some() {
                    self.pending_g = false;
                }

                command
            };

            // Escape while already in NAV only clears a pending prefix.
            if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                self.pending_g = false;
            }

            for key in [
                egui::Key::Backspace,
                egui::Key::Delete,
                egui::Key::Enter,
                egui::Key::Tab,
            ] {
                let _ = input.consume_key(egui::Modifiers::NONE, key);
            }

            for key in [
                egui::Key::Z,
                egui::Key::Y,
                egui::Key::X,
                egui::Key::V,
                egui::Key::K,
                egui::Key::U,
                egui::Key::W,
            ] {
                let _ = input.consume_key(egui::Modifiers::COMMAND, key);
                let _ = input.consume_key(egui::Modifiers::CTRL, key);
            }

            input.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::Text(_)
                        | egui::Event::Paste(_)
                        | egui::Event::Cut
                        | egui::Event::Ime(_)
                )
            });

            command
        })
    }

    pub fn apply(
        &mut self,
        ctx: &egui::Context,
        editor_id: egui::Id,
        galley: &egui::Galley,
        text: &str,
        cursor_range: Option<egui::text::CCursorRange>,
        command: Option<NavCommand>,
    ) -> bool {
        let Some(command) = command else {
            return false;
        };

        let primary = cursor_range
            .map(|range| range.primary)
            .unwrap_or_else(|| egui::text::CCursor::new(0));

        let next = match command {
            NavCommand::Left => {
                self.vertical_x = None;
                galley.cursor_left_one_character(&primary)
            }
            NavCommand::Right => {
                self.vertical_x = None;
                galley.cursor_right_one_character(&primary)
            }
            NavCommand::Up => {
                let (cursor, x) = galley.cursor_up_one_row(&primary, self.vertical_x);
                self.vertical_x = x;
                cursor
            }
            NavCommand::Down => {
                let (cursor, x) = galley.cursor_down_one_row(&primary, self.vertical_x);
                self.vertical_x = x;
                cursor
            }
            NavCommand::WordNext => {
                self.vertical_x = None;
                egui::text::CCursor::new(next_word_start(text, primary.index.0))
            }
            NavCommand::WordPrevious => {
                self.vertical_x = None;
                egui::text::CCursor::new(previous_word_start(text, primary.index.0))
            }
            NavCommand::RowStart => {
                self.vertical_x = None;
                galley.cursor_begin_of_row(&primary)
            }
            NavCommand::RowEnd => {
                self.vertical_x = None;
                galley.cursor_end_of_row(&primary)
            }
            NavCommand::DocumentStart => {
                self.vertical_x = None;
                egui::text::CCursor::new(0)
            }
            NavCommand::DocumentEnd => {
                self.vertical_x = None;
                egui::text::CCursor::new(text.chars().count())
            }
        };

        let mut state = egui::TextEdit::load_state(ctx, editor_id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(next)));
        egui::TextEdit::store_state(ctx, editor_id, state);
        ctx.memory_mut(|memory| memory.request_focus(editor_id));
        ctx.request_repaint();
        true
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '\'')
}

fn next_word_start(text: &str, index: usize) -> usize {
    let char_count = text.chars().count();
    let mut position = index.min(char_count);
    let mut chars = text.chars().skip(position).peekable();

    if chars.peek().is_some_and(|ch| is_word_char(*ch)) {
        while chars.peek().is_some_and(|ch| is_word_char(*ch)) {
            chars.next();
            position += 1;
        }
    }

    while chars.peek().is_some_and(|ch| !is_word_char(*ch)) {
        chars.next();
        position += 1;
    }

    position.min(char_count)
}

fn previous_word_start(text: &str, index: usize) -> usize {
    let target = index.min(text.chars().count());
    let mut last_start = 0;
    let mut in_word = false;

    for (position, ch) in text.chars().enumerate().take(target) {
        if is_word_char(ch) {
            if !in_word {
                last_start = position;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }

    if in_word && last_start < target {
        return last_start;
    }

    let mut candidate = 0;
    let mut saw_word = false;
    let mut in_candidate = false;

    for (position, ch) in text.chars().enumerate().take(target) {
        if is_word_char(ch) {
            if !in_candidate {
                candidate = position;
                in_candidate = true;
            }
            saw_word = true;
        } else {
            in_candidate = false;
        }
    }

    if saw_word { candidate } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::{next_word_start, previous_word_start};

    #[test]
    fn next_word_moves_to_next_word_start() {
        let text = "alpha  beta gamma";
        assert_eq!(next_word_start(text, 0), 7);
        assert_eq!(next_word_start(text, 2), 7);
        assert_eq!(next_word_start(text, 5), 7);
        assert_eq!(next_word_start(text, 7), 12);
        assert_eq!(
            next_word_start(text, text.chars().count()),
            text.chars().count()
        );
    }

    #[test]
    fn previous_word_moves_to_current_or_previous_word_start() {
        let text = "alpha  beta gamma";
        assert_eq!(previous_word_start(text, 0), 0);
        assert_eq!(previous_word_start(text, 3), 0);
        assert_eq!(previous_word_start(text, 7), 0);
        assert_eq!(previous_word_start(text, 9), 7);
        assert_eq!(previous_word_start(text, 12), 7);
        assert_eq!(previous_word_start(text, 15), 12);
    }
}
