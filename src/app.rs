use std::time::{Duration, Instant};

use eframe::egui;

use crate::{
    editor_buffer::EditorBuffer,
    metrics::TimingMetric,
    storage::{LedgerStore, SaveEvent},
};

const AUTOSAVE_IDLE: Duration = Duration::from_millis(160);
const SAVE_STATUS_POLL: Duration = Duration::from_millis(40);

struct AppMetrics {
    process_started: Instant,
    first_ui: Option<Duration>,
    frame_cpu: TimingMetric,
    snapshot_clone: TimingMetric,
    background_save: TimingMetric,
}

impl AppMetrics {
    fn new(process_started: Instant) -> Self {
        Self {
            process_started,
            first_ui: None,
            frame_cpu: TimingMetric::default(),
            snapshot_clone: TimingMetric::default(),
            background_save: TimingMetric::default(),
        }
    }
}

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
    metrics: AppMetrics,
}

impl LexwrightApp {
    pub fn new(cc: &eframe::CreationContext<'_>, process_started: Instant) -> Self {
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
            metrics: AppMetrics::new(process_started),
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
                SaveEvent::Saved { revision, elapsed } => {
                    self.metrics.background_save.observe(elapsed);
                    self.saved_revision = self.saved_revision.max(revision);
                    if revision >= self.revision {
                        self.save_error = None;
                    }
                }
                SaveEvent::Failed {
                    revision,
                    elapsed,
                    error,
                } => {
                    self.metrics.background_save.observe(elapsed);
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
        let clone_started = Instant::now();
        let snapshot = self.buffer.text().to_owned();
        self.metrics.snapshot_clone.observe(clone_started.elapsed());

        match self.store.queue_save(revision, snapshot) {
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

    fn show_perf_status(&self, ui: &mut egui::Ui) {
        let insert = self.buffer.insert_timing();
        let label = if insert.count() == 0 {
            "perf ready".to_owned()
        } else {
            format!("perf edit {}", format_ns(insert.last_ns()))
        };

        let first_ui = self
            .metrics
            .first_ui
            .map(|duration| format_ns(duration.as_nanos().min(u64::MAX as u128) as u64))
            .unwrap_or_else(|| "pending".to_owned());

        let expansion = self.buffer.expansion_lookup_timing();
        let indexes = self.buffer.index_stats();
        let total_indexes = indexes.ascii_fast.saturating_add(indexes.utf8_fallback);
        let ascii_percent = if total_indexes == 0 {
            0.0
        } else {
            indexes.ascii_fast as f64 * 100.0 / total_indexes as f64
        };

        let tooltip = format!(
            "first UI: {first_ui}\n\
             frame CPU last/avg/max: {}/{}/{}\n\
             edit CPU last/avg/max: {}/{}/{}\n\
             expansion lookup last/avg/max: {}/{}/{}\n\
             index path: {:.1}% ASCII O(1) ({} fast / {} UTF-8 fallback)\n\
             snapshot clone last/max: {}/{}\n\
             background save last/max: {}/{}\n\
             document: {} bytes · buffer path: {}",
            format_ns(self.metrics.frame_cpu.last_ns()),
            format_ns(self.metrics.frame_cpu.average_ns()),
            format_ns(self.metrics.frame_cpu.max_ns()),
            format_ns(insert.last_ns()),
            format_ns(insert.average_ns()),
            format_ns(insert.max_ns()),
            format_ns(expansion.last_ns()),
            format_ns(expansion.average_ns()),
            format_ns(expansion.max_ns()),
            ascii_percent,
            indexes.ascii_fast,
            indexes.utf8_fallback,
            format_ns(self.metrics.snapshot_clone.last_ns()),
            format_ns(self.metrics.snapshot_clone.max_ns()),
            format_ns(self.metrics.background_save.last_ns()),
            format_ns(self.metrics.background_save.max_ns()),
            self.buffer.text().len(),
            if self.buffer.is_ascii_fast_path() {
                "ASCII fast"
            } else {
                "UTF-8 fallback"
            },
        );

        ui.weak(label).on_hover_text(tooltip);
    }
}

impl eframe::App for LexwrightApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let frame_started = Instant::now();

        if self.metrics.first_ui.is_none() {
            self.metrics.first_ui = Some(self.metrics.process_started.elapsed());
        }

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
            ui.separator();
            self.show_perf_status(ui);

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

        self.metrics.frame_cpu.observe(frame_started.elapsed());
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.queue_current_revision();
        let _ = self.store.flush();
    }
}

fn format_ns(nanos: u64) -> String {
    if nanos == 0 {
        return "—".to_owned();
    }

    if nanos < 1_000 {
        format!("{nanos} ns")
    } else if nanos < 1_000_000 {
        format!("{:.1} µs", nanos as f64 / 1_000.0)
    } else {
        format!("{:.2} ms", nanos as f64 / 1_000_000.0)
    }
}
