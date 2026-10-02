use std::time::{Duration, Instant};

use eframe::egui;

use crate::storage::{LedgerStore, SaveEvent};

const AUTOSAVE_IDLE: Duration = Duration::from_millis(160);

pub struct LexwrightApp {
    text: String,
    revision: u64,
    queued_revision: u64,
    saved_revision: u64,
    last_edit: Option<Instant>,
    save_error: Option<String>,
    store: LedgerStore,
    focus_editor: bool,
}

impl LexwrightApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let store = LedgerStore::default();
        let (text, save_error) = match store.load() {
            Ok(text) => (text, None),
            Err(error) => (
                String::new(),
                Some(format!("could not load ledger: {error}")),
            ),
        };

        Self {
            text,
            revision: 0,
            queued_revision: 0,
            saved_revision: 0,
            last_edit: None,
            save_error,
            store,
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
        match self.store.queue_save(revision, self.text.clone()) {
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

    fn status_text(&self) -> String {
        if let Some(error) = &self.save_error {
            return format!("save error: {error}");
        }

        if self.saved_revision >= self.revision {
            return "saved".to_owned();
        }

        if self.queued_revision >= self.revision {
            return "saving".to_owned();
        }

        "edited".to_owned()
    }
}

impl eframe::App for LexwrightApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_save_events();
        self.maybe_autosave();

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.strong("Lexwright");
            ui.separator();
            ui.weak(self.status_text());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(self.store.path().display().to_string());
            });
        });
        ui.separator();

        let editor = egui::TextEdit::multiline(&mut self.text)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY)
            .lock_focus(true)
            .frame(false)
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
