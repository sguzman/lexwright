use std::time::{Duration, Instant};

use eframe::egui;

use crate::{
    editor_buffer::EditorBuffer,
    storage::{LedgerStore, SaveEvent},
};

const AUTOSAVE_IDLE: Duration = Duration::from_millis(160);
const SAVE_STATUS_POLL: Duration = Duration::from_millis(40);

pub struct LexwrightApp {
    buffer: EditorBuffer,
    revision: u64,
    queued_revision: u64,
    saved_revision: u64,
    last_edit: Option<Instant>,
    save_error: Option<String>,
    store: LedgerStore,
    path_label: String,
    focus_editor: bool,
}

impl LexwrightApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let store = LedgerStore::default();
        let path_label = store.path().display().to_string();
        let (text, save_error) = match store.load() {
            Ok(text) => (text, None),
            Err(error) => (
                String::new(),
                Some(format!("could not load ledger: {error}")),
            ),
        };

        Self {
            buffer: EditorBuffer::new(text),
            revision: 0,
            queued_revision: 0,
            saved_revision: 0,
            last_edit: None,
            save_error,
            store,
            path_label,
            focus_editor: true,
        }
    }

    fn mark_edited(&mut self, ctx: &egui::Context) {
        self.revision = self.revision.wrapping_add(1);
        self.last_edit = Some(Instant::now());
        self.save_error = None;
        ctx.request_repaint_after(AUTOSAVE_IDLE);
    }

    fn poll_save_events(&mut self) {
        while let Some(event) = self.store.poll() {
            match event {
                SaveEvent::Saved(revision) => {
                    self.saved_revision = self.saved_revision.max(revision);
                    if revision >= self.revision {
                        self.save_error = None;
                    }
                }
                SaveEvent::Failed { revision, error } => {
                    if revision >= self.saved_revision {
                        self.save_error = Some(error);
                    }
                }
            }
        }
    }

    fn queue_current_revision(&mut self) {
        if self.queued_revision >= self.revision {
            return;
        }

        let revision = self.revision;
        match self.store.queue_save(revision, self.buffer.text().to_owned()) {
            Ok(()) => {
                self.queued_revision = revision;
            }
            Err(error) => {
                self.save_error = Some(error);
            }
        }
    }

    fn maybe_autosave(&mut self) {
        if self.queued_revision >= self.revision {
            return;
        }

        let Some(last_edit) = self.last_edit else {
            return;
        };

        if last_edit.elapsed() >= AUTOSAVE_IDLE {
            self.queue_current_revision();
        }
    }

    fn show_save_status(&self, ui: &mut egui::Ui) {
        if let Some(error) = &self.save_error {
            ui.weak(format!("save error: {error}"));
        } else if self.saved_revision >= self.revision {
            ui.weak("saved");
        } else if self.queued_revision >= self.revision {
            ui.weak("saving");
        } else {
            ui.weak("edited");
        }
    }

    fn show_expansion_status(&mut self, ui: &mut egui::Ui) {
        let enabled = self.buffer.expansions_enabled();
        let label = if enabled {
            format!(
                "expand on · {} rules · {} hits",
                self.buffer.expansion_rule_count(),
                self.buffer.expansion_hits()
            )
        } else {
            format!("expand off · {} rules", self.buffer.expansion_rule_count())
        };

        let response = ui.selectable_label(enabled, label).on_hover_text(format!(
            "Click to toggle. User rules: {}",
            self.buffer.expansion_config_path().display()
        ));

        if response.clicked() {
            self.buffer.set_expansions_enabled(!enabled);
            self.focus_editor = true;
        }

        if let Some(error) = self.buffer.expansion_config_error() {
            ui.separator();
            ui.weak("expansion config error").on_hover_text(error);
        }
    }
}

impl eframe::App for LexwrightApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_save_events();
        self.maybe_autosave();

        if self.queued_revision > self.saved_revision {
            ui.ctx().request_repaint_after(SAVE_STATUS_POLL);
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.strong("Lexwright");
            ui.separator();
            self.show_save_status(ui);
            ui.separator();
            self.show_expansion_status(ui);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(&self.path_label);
            });
        });
        ui.separator();

        let editor = egui::TextEdit::multiline(&mut self.buffer)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY)
            .lock_focus(true)
            .hint_text("Write.");

        let response = ui.add_sized(ui.available_size(), editor);

        if self.focus_editor {
            response.request_focus();
            self.focus_editor = false;
        }

        if response.changed() {
            self.mark_edited(ui.ctx());
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.queue_current_revision();
        let _ = self.store.flush();
    }
}
