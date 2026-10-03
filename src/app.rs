use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use eframe::egui;

use crate::{
    analysis::{AnalysisWorker, TextAnalysis},
    editor_buffer::EditorBuffer,
    expansion::ExpansionRule,
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

#[derive(Default)]
struct RuleEditor {
    open: bool,
    starter_enabled: bool,
    rules: Vec<ExpansionRule>,
    new_trigger: String,
    new_replacement: String,
    status: Option<String>,
}

impl RuleEditor {
    fn load_from(&mut self, buffer: &EditorBuffer) {
        self.open = true;
        self.starter_enabled = buffer.expansion_starter_enabled();
        self.rules = buffer.expansion_user_rules().to_vec();
        self.new_trigger.clear();
        self.new_replacement.clear();
        self.status = None;
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
    rule_editor: RuleEditor,
    analysis_worker: AnalysisWorker,
    analysis_latest: Option<TextAnalysis>,
    analysis_pending_revision: Option<u64>,
    analysis_error: Option<String>,
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

        let analysis_worker = AnalysisWorker::new();
        let initial_snapshot = Arc::<str>::from(text.as_str());
        let (analysis_pending_revision, analysis_error) =
            match analysis_worker.queue(0, initial_snapshot) {
                Ok(()) => (Some(0), None),
                Err(error) => (None, Some(error)),
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
            rule_editor: RuleEditor::default(),
            analysis_worker,
            analysis_latest: None,
            analysis_pending_revision,
            analysis_error,
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
        let snapshot = Arc::<str>::from(self.buffer.text());
        self.metrics.snapshot_clone.observe(clone_started.elapsed());

        match self.store.queue_save(revision, Arc::clone(&snapshot)) {
            Ok(()) => {
                self.queued_revision = revision;

                match self.analysis_worker.queue(revision, snapshot) {
                    Ok(()) => {
                        self.analysis_pending_revision = Some(revision);
                        self.analysis_error = None;
                    }
                    Err(error) => {
                        self.analysis_pending_revision = None;
                        self.analysis_error = Some(error);
                    }
                }
            }
            Err(error) => {
                self.save_error = Some(error);
            }
        }
    }

    fn poll_analysis(&mut self) {
        while let Some(result) = self.analysis_worker.poll() {
            let should_store = self
                .analysis_latest
                .is_none_or(|previous| result.revision >= previous.revision);

            if should_store {
                if self
                    .analysis_pending_revision
                    .is_some_and(|pending| result.revision >= pending)
                {
                    self.analysis_pending_revision = None;
                }
                self.analysis_latest = Some(result);
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

        if ui.small_button("rules").clicked() {
            self.rule_editor.load_from(&self.buffer);
        }

        if let Some(error) = self.buffer.expansion_config_error() {
            ui.separator();
            ui.weak("expansion config error").on_hover_text(error);
        }
    }

    fn show_rule_editor(&mut self, ctx: &egui::Context) {
        if !self.rule_editor.open {
            return;
        }

        let config_path = self.buffer.expansion_config_path().display().to_string();
        let mut open = self.rule_editor.open;
        let mut apply = false;
        let mut revert = false;

        egui::Window::new("Expansion rules")
            .open(&mut open)
            .default_width(640.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.weak(&config_path);
                ui.add_space(4.0);

                ui.checkbox(
                    &mut self.rule_editor.starter_enabled,
                    "Enable the 10 starter rules",
                )
                .on_hover_text(
                    "User rules override starter rules with the same trigger. Turn this off to build your expansion language from scratch.",
                );

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.strong("Add rule");
                    ui.label("trigger");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.rule_editor.new_trigger)
                            .desired_width(100.0),
                    );
                    ui.label("replacement");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.rule_editor.new_replacement)
                            .desired_width(240.0),
                    );

                    if ui.button("Add").clicked() {
                        if self.rule_editor.new_trigger.is_empty() {
                            self.rule_editor.status =
                                Some("trigger cannot be empty".to_owned());
                        } else {
                            self.rule_editor.rules.push(ExpansionRule {
                                trigger: std::mem::take(&mut self.rule_editor.new_trigger),
                                replacement: std::mem::take(
                                    &mut self.rule_editor.new_replacement,
                                ),
                            });
                            self.rule_editor.status = Some(
                                "draft rule added; Apply to compile and save".to_owned(),
                            );
                        }
                    }
                });

                ui.add_space(8.0);
                ui.separator();

                let mut remove_index = None;
                egui::ScrollArea::vertical()
                    .max_height(340.0)
                    .show(ui, |ui| {
                        egui::Grid::new("lexwright_expansion_rule_grid")
                            .num_columns(3)
                            .striped(true)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                ui.strong("trigger");
                                ui.strong("replacement");
                                ui.strong("");
                                ui.end_row();

                                for (index, rule) in
                                    self.rule_editor.rules.iter_mut().enumerate()
                                {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut rule.trigger)
                                            .desired_width(120.0),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(&mut rule.replacement)
                                            .desired_width(360.0),
                                    );
                                    if ui.small_button("remove").clicked() {
                                        remove_index = Some(index);
                                    }
                                    ui.end_row();
                                }
                            });
                    });

                if let Some(index) = remove_index {
                    self.rule_editor.rules.remove(index);
                    self.rule_editor.status =
                        Some("draft rule removed; Apply to save".to_owned());
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }

                    if ui.button("Revert").clicked() {
                        revert = true;
                    }

                    ui.weak(format!(
                        "{} draft user rules · starter rules {}",
                        self.rule_editor.rules.len(),
                        if self.rule_editor.starter_enabled {
                            "on"
                        } else {
                            "off"
                        }
                    ));
                });

                if let Some(status) = &self.rule_editor.status {
                    ui.add_space(4.0);
                    ui.weak(status);
                }

                ui.add_space(4.0);
                ui.weak(
                    "Rules are compiled only when you press Apply. Editing this window never enters the typing hot path.",
                );
            });

        self.rule_editor.open = open;

        if revert {
            self.rule_editor.load_from(&self.buffer);
            self.rule_editor.status = Some("reverted to the active rules".to_owned());
        }

        if apply {
            let starter_enabled = self.rule_editor.starter_enabled;
            let rules = self.rule_editor.rules.clone();

            match self
                .buffer
                .apply_expansion_config(starter_enabled, rules)
            {
                Ok(()) => {
                    self.rule_editor.status = Some(format!(
                        "saved and compiled {} active rules",
                        self.buffer.expansion_rule_count()
                    ));
                }
                Err(error) => {
                    self.rule_editor.status = Some(format!("cannot apply: {error}"));
                }
            }
        }
    }


    fn show_analysis_status(&self, ui: &mut egui::Ui) {
        if let Some(error) = &self.analysis_error {
            ui.weak("analysis error").on_hover_text(error);
            return;
        }

        let Some(analysis) = self.analysis_latest else {
            ui.weak("words …");
            return;
        };

        let stale = analysis.revision < self.revision;
        let label = if stale {
            format!("words {} · analyzing", analysis.words)
        } else {
            format!("words {}", analysis.words)
        };

        ui.weak(label).on_hover_text(format!(
            "revision: {}{}\nwords: {}\ncharacters: {}\nbytes: {}\nlines: {}\nparagraphs: {}\nanalysis CPU: {}",
            analysis.revision,
            if stale { " (stale)" } else { "" },
            analysis.words,
            analysis.chars,
            analysis.bytes,
            analysis.lines,
            analysis.paragraphs,
            format_ns(analysis.elapsed.as_nanos().min(u64::MAX as u128) as u64),
        ));
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
        self.poll_analysis();
        self.maybe_autosave();

        if self.queued_revision > self.saved_revision
            || self.analysis_pending_revision.is_some()
        {
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
            self.show_analysis_status(ui);
            ui.separator();
            self.show_perf_status(ui);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(&self.path_label);
            });
        });
        ui.separator();

        self.show_rule_editor(ui.ctx());

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
