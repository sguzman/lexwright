use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use eframe::egui;

use crate::{
    analysis::{AnalysisWorker, LexicalClass, LexicalSpan, MorphClass, MorphSpan, TextAnalysis},
    editor_buffer::EditorBuffer,
    expansion::ExpansionRule,
    harper::{HarperDiagnostic, HarperResult, HarperSuggestion, HarperWorker},
    metrics::TimingMetric,
    storage::{LedgerStore, SaveEvent},
};

const HARPER_IDLE: Duration = Duration::from_millis(45);
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

struct PendingExternalEdit {
    expected_revision: u64,
    start_byte: usize,
    end_byte: usize,
    replacement: String,
    description: String,
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
    structure_overlay: bool,
    morphology_overlay: bool,
    lexeme_window: bool,
    harper_worker: HarperWorker,
    harper_enabled: bool,
    harper_latest: Option<HarperResult>,
    harper_pending_revision: Option<u64>,
    harper_error: Option<String>,
    harper_window: bool,
    pending_external_edit: Option<PendingExternalEdit>,
    harper_action_status: Option<String>,
    snapshot_revision: Option<u64>,
    snapshot_cache: Option<Arc<str>>,
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
        let harper_worker = HarperWorker::new();
        let initial_snapshot = Arc::<str>::from(text.as_str());

        let (analysis_pending_revision, analysis_error) =
            match analysis_worker.queue(0, Arc::clone(&initial_snapshot)) {
                Ok(()) => (Some(0), None),
                Err(error) => (None, Some(error)),
            };

        // Harper is intentionally lazy. Merely opening Lexwright must not initialize
        // its dictionary or compete with the first interactive frame.
        let harper_pending_revision = None;
        let harper_error = None;

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
            structure_overlay: false,
            morphology_overlay: false,
            lexeme_window: false,
            harper_worker,
            harper_enabled: false,
            harper_latest: None,
            harper_pending_revision,
            harper_error,
            harper_window: false,
            pending_external_edit: None,
            harper_action_status: None,
            snapshot_revision: Some(0),
            snapshot_cache: Some(initial_snapshot),
        }
    }

    fn mark_edited(&mut self, ctx: &egui::Context) {
        self.revision = self.revision.wrapping_add(1);
        self.last_edit = Some(Instant::now());
        self.save_error = None;
        self.snapshot_revision = None;
        self.snapshot_cache = None;

        ctx.request_repaint_after(if self.harper_enabled {
            HARPER_IDLE
        } else {
            AUTOSAVE_IDLE
        });
    }

    fn current_snapshot(&mut self) -> Arc<str> {
        if self.snapshot_revision == Some(self.revision)
            && let Some(snapshot) = &self.snapshot_cache
        {
            return Arc::clone(snapshot);
        }

        let started = Instant::now();
        let snapshot = Arc::<str>::from(self.buffer.text());
        self.metrics.snapshot_clone.observe(started.elapsed());
        self.snapshot_revision = Some(self.revision);
        self.snapshot_cache = Some(Arc::clone(&snapshot));
        snapshot
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
        let snapshot = self.current_snapshot();

        match self.store.queue_save(revision, Arc::clone(&snapshot)) {
            Ok(()) => {
                self.queued_revision = revision;
            }
            Err(error) => {
                self.save_error = Some(error);
            }
        }

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

    fn poll_analysis(&mut self) {
        while let Some(result) = self.analysis_worker.poll() {
            let should_store = self
                .analysis_latest
                .as_ref()
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

    fn queue_harper_current(&mut self) {
        if !self.harper_enabled
            || self
                .harper_pending_revision
                .is_some_and(|pending| pending >= self.revision)
            || self
                .harper_latest
                .as_ref()
                .is_some_and(|result| result.revision >= self.revision)
        {
            return;
        }

        let revision = self.revision;
        let snapshot = self.current_snapshot();
        match self.harper_worker.queue(revision, snapshot) {
            Ok(()) => {
                self.harper_pending_revision = Some(revision);
                self.harper_error = None;
            }
            Err(error) => {
                self.harper_pending_revision = None;
                self.harper_error = Some(error);
            }
        }
    }

    fn maybe_queue_harper(&mut self) {
        if !self.harper_enabled {
            return;
        }

        let Some(last_edit) = self.last_edit else {
            return;
        };

        if last_edit.elapsed() >= HARPER_IDLE {
            self.queue_harper_current();
        } else {
            let remaining = HARPER_IDLE.saturating_sub(last_edit.elapsed());
            // The caller always has a Context available, so the UI loop requests this
            // repaint separately after invoking maybe_queue_harper.
            let _ = remaining;
        }
    }

    fn poll_harper(&mut self) {
        while let Some(result) = self.harper_worker.poll() {
            if !self.harper_enabled {
                continue;
            }

            let should_store = self
                .harper_latest
                .as_ref()
                .is_none_or(|previous| result.revision >= previous.revision);

            if should_store {
                if self
                    .harper_pending_revision
                    .is_some_and(|pending| result.revision >= pending)
                {
                    self.harper_pending_revision = None;
                }
                self.harper_latest = Some(result);
            }
        }
    }

    fn apply_pending_external_edit(&mut self, ctx: &egui::Context, editor_id: egui::Id) {
        let Some(edit) = self.pending_external_edit.take() else {
            return;
        };

        if edit.expected_revision != self.revision {
            self.harper_action_status = Some(
                "suggestion expired because the ledger changed before it was applied".to_owned(),
            );
            return;
        }

        let mut state = egui::TextEdit::load_state(ctx, editor_id).unwrap_or_default();
        let old_cursor = state.cursor.char_range().unwrap_or_else(|| {
            egui::text::CCursorRange::one(egui::text::CCursor::new(
                self.buffer.text().chars().count(),
            ))
        });

        // Programmatic edits explicitly seed egui's undoer with the pre-edit state.
        // The TextEdit sees the mutated state immediately afterwards, so Ctrl+Z can
        // return to the exact text/cursor state that existed before the suggestion.
        let old_text = self.buffer.text().to_owned();
        let mut undoer = state.undoer();
        undoer.add_undo(&(old_cursor, old_text));

        match self
            .buffer
            .replace_byte_range(edit.start_byte..edit.end_byte, &edit.replacement)
        {
            Ok(cursor_char) => {
                let cursor = egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_char));
                state.cursor.set_char_range(Some(cursor));
                state.set_undoer(undoer);
                state.store(ctx, editor_id);

                self.mark_edited(ctx);
                self.focus_editor = true;
                self.harper_action_status = Some(format!(
                    "applied {}; Ctrl+Z restores the previous text",
                    edit.description
                ));
            }
            Err(error) => {
                self.harper_action_status = Some(format!("could not apply suggestion: {error}"));
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

            match self.buffer.apply_expansion_config(starter_enabled, rules) {
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

        let Some(analysis) = self.analysis_latest.as_ref() else {
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

    fn show_structure_status(&mut self, ui: &mut egui::Ui) {
        let current = self
            .analysis_latest
            .as_ref()
            .filter(|analysis| analysis.revision == self.revision);

        let label = match (self.structure_overlay, current.is_some()) {
            (false, _) => "structure off",
            (true, true) => "structure on",
            (true, false) => "structure waiting",
        };

        let mut tooltip = String::from(
            "Heuristic English structure overlay. Closed-class categories are lexical matches; open-class *-like categories use conservative suffix/lexicon heuristics. This is not yet a statistical POS tagger.\n",
        );

        if let Some(analysis) = current {
            for class in LexicalClass::ALL {
                tooltip.push_str(class.label());
                tooltip.push_str(": ");
                tooltip.push_str(&analysis.lexical_counts.get(class).to_string());
                tooltip.push('\n');
            }
            tooltip.push_str("unclassified: ");
            tooltip.push_str(&analysis.lexical_counts.unclassified.to_string());
            tooltip.push_str("\nclassified total: ");
            tooltip.push_str(&analysis.lexical_counts.classified_total().to_string());
        } else {
            tooltip.push_str("Waiting for analysis of the current revision.");
        }

        let response = ui
            .selectable_label(self.structure_overlay, label)
            .on_hover_text(tooltip);

        if response.clicked() {
            self.structure_overlay = !self.structure_overlay;
            if self.structure_overlay {
                self.morphology_overlay = false;
            }
            self.focus_editor = true;
        }
    }

    fn show_morphology_status(&mut self, ui: &mut egui::Ui) {
        let current = self
            .analysis_latest
            .as_ref()
            .filter(|analysis| analysis.revision == self.revision);

        let label = match (self.morphology_overlay, current.is_some()) {
            (false, _) => "morph off",
            (true, true) => "morph on",
            (true, false) => "morph waiting",
        };

        let tooltip = if let Some(analysis) = current {
            format!(
                "Heuristic orthographic morphology, not lemmatization.\ndecomposed words: {}\nprefixes: {}\nsuffixes: {}\n\nPrefix/stem/suffix colors show surface segmentation. Spelling alternations such as happiness → happy and running → run are not normalized yet.",
                analysis.morph_counts.decomposed_words,
                analysis.morph_counts.prefixes,
                analysis.morph_counts.suffixes,
            )
        } else {
            "Heuristic orthographic morphology. Waiting for analysis of the current revision."
                .to_owned()
        };

        let response = ui
            .selectable_label(self.morphology_overlay, label)
            .on_hover_text(tooltip);

        if response.clicked() {
            self.morphology_overlay = !self.morphology_overlay;
            if self.morphology_overlay {
                self.structure_overlay = false;
            }
            self.focus_editor = true;
        }
    }

    fn show_lexeme_status(&mut self, ui: &mut egui::Ui) {
        let current = self
            .analysis_latest
            .as_ref()
            .filter(|analysis| analysis.revision == self.revision);

        let count = current.map_or(0, |analysis| analysis.lexeme_candidates.len());
        let label = if current.is_some() {
            format!("lexemes {count}")
        } else {
            "lexemes …".to_owned()
        };

        let response = ui
            .small_button(label)
            .on_hover_text(
                "Open conservative lexeme candidates derived from the current morphology pass. Candidates never rewrite the ledger.",
            );

        if response.clicked() {
            self.lexeme_window = !self.lexeme_window;
        }
    }

    fn show_lexeme_window(&mut self, ctx: &egui::Context) {
        if !self.lexeme_window {
            return;
        }

        let mut open = self.lexeme_window;

        egui::Window::new("Lexeme candidates")
            .open(&mut open)
            .default_width(540.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.weak(
                    "Derived candidates only. The surface bytes remain canonical; normalization is observational.",
                );
                ui.add_space(6.0);

                let Some(analysis) = self
                    .analysis_latest
                    .as_ref()
                    .filter(|analysis| analysis.revision == self.revision)
                else {
                    ui.weak("Waiting for analysis of the current revision.");
                    return;
                };

                if analysis.lexeme_candidates.is_empty() {
                    ui.weak("No morphology-derived lexeme candidates in this revision.");
                    return;
                }

                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        egui::Grid::new("lexwright_lexeme_grid")
                            .num_columns(4)
                            .striped(true)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                ui.strong("word");
                                ui.strong("surface stem");
                                ui.strong("lexeme");
                                ui.strong("rule");
                                ui.end_row();

                                for candidate in analysis.lexeme_candidates.iter() {
                                    let text = self.buffer.text();

                                    let word = text
                                        .get(candidate.word_start..candidate.word_end)
                                        .unwrap_or("?");
                                    let stem = text
                                        .get(candidate.stem_start..candidate.stem_end)
                                        .unwrap_or("?");

                                    ui.monospace(word);
                                    ui.monospace(stem);
                                    ui.monospace(candidate.lexeme.as_ref());
                                    ui.weak(candidate.rule.label());
                                    ui.end_row();
                                }
                            });
                    });
            });

        self.lexeme_window = open;
    }

    fn show_harper_status(&mut self, ui: &mut egui::Ui) {
        let label = if !self.harper_enabled {
            "harper off".to_owned()
        } else if let Some(result) = self
            .harper_latest
            .as_ref()
            .filter(|result| result.revision == self.revision)
        {
            format!("harper {}", result.diagnostics.len())
        } else if self.harper_pending_revision.is_some() {
            "harper …".to_owned()
        } else {
            "harper on".to_owned()
        };

        let tooltip = if let Some(error) = &self.harper_error {
            format!("Harper error: {error}\nClick to open diagnostics/settings.")
        } else if !self.harper_enabled {
            "Harper is disabled and consumes no analysis work. Click to open diagnostics/settings."
                .to_owned()
        } else if let Some(result) = self
            .harper_latest
            .as_ref()
            .filter(|result| result.revision == self.revision)
        {
            format!(
                "Harper 2.11.0 · American English\nrevision: {}\ndiagnostics: {}{}\nHarper CPU: {}\nwork: {} / {} bytes{}\n\nRuns on a separate background worker. Click to inspect suggestions.",
                result.revision,
                result.diagnostics.len(),
                if result.truncated { " (truncated)" } else { "" },
                format_ns(result.elapsed.as_nanos().min(u64::MAX as u128) as u64),
                result.linted_bytes,
                result.total_bytes,
                if result.incremental {
                    " incremental"
                } else {
                    " full"
                },
            )
        } else {
            "Harper is waiting for the current revision. Click to open diagnostics/settings."
                .to_owned()
        };

        if ui.small_button(label).on_hover_text(tooltip).clicked() {
            self.harper_window = !self.harper_window;
        }
    }

    fn show_harper_window(&mut self, ctx: &egui::Context) {
        if !self.harper_window {
            return;
        }

        let mut open = self.harper_window;
        let mut requested_edit = None;

        egui::Window::new("Harper diagnostics")
            .open(&mut open)
            .default_width(680.0)
            .resizable(true)
            .show(ctx, |ui| {
                let was_enabled = self.harper_enabled;
                ui.checkbox(
                    &mut self.harper_enabled,
                    "Enable background Harper diagnostics",
                )
                .on_hover_text(
                    "When disabled, Lexwright does not initialize Harper or send it document snapshots.",
                );

                if self.harper_enabled && !was_enabled {
                    self.queue_harper_current();
                } else if !self.harper_enabled && was_enabled {
                    self.harper_pending_revision = None;
                    self.harper_error = None;
                }

                ui.weak(
                    "Suggestions apply as revision-checked editor mutations and participate in Ctrl+Z.",
                );

                if let Some(status) = &self.harper_action_status {
                    ui.add_space(4.0);
                    ui.weak(status);
                }

                ui.add_space(6.0);

                if !self.harper_enabled {
                    ui.weak("Harper is off.");
                    return;
                }

                let Some(result) = self
                    .harper_latest
                    .as_ref()
                    .filter(|result| result.revision == self.revision)
                else {
                    ui.weak("Waiting for Harper to finish the current revision.");
                    return;
                };

                if result.diagnostics.is_empty() {
                    ui.label("No Harper diagnostics for this revision.");
                    return;
                }

                if result.truncated {
                    ui.weak("Showing the first 256 diagnostics.");
                    ui.separator();
                }

                egui::ScrollArea::vertical()
                    .max_height(480.0)
                    .show(ui, |ui| {
                        for diagnostic in result.diagnostics.iter() {
                            let source = self
                                .buffer
                                .text()
                                .get(diagnostic.start_byte..diagnostic.end_byte)
                                .unwrap_or("?");

                            ui.horizontal_wrapped(|ui| {
                                ui.strong(diagnostic.kind.as_ref());
                                ui.monospace(source);
                                ui.weak(format!("priority {}", diagnostic.priority));
                            });
                            ui.label(diagnostic.message.as_ref());

                            if !diagnostic.suggestions.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    ui.weak("suggestions:");

                                    for suggestion in diagnostic.suggestions.iter() {
                                        let button_label = format!("apply {}", suggestion.label());
                                        if ui.small_button(button_label).clicked() {
                                            let (start_byte, end_byte, replacement) =
                                                match suggestion {
                                                    HarperSuggestion::ReplaceWith(text) => (
                                                        diagnostic.start_byte,
                                                        diagnostic.end_byte,
                                                        text.to_string(),
                                                    ),
                                                    HarperSuggestion::InsertAfter(text) => (
                                                        diagnostic.end_byte,
                                                        diagnostic.end_byte,
                                                        text.to_string(),
                                                    ),
                                                    HarperSuggestion::Remove => (
                                                        diagnostic.start_byte,
                                                        diagnostic.end_byte,
                                                        String::new(),
                                                    ),
                                                };

                                            requested_edit = Some(PendingExternalEdit {
                                                expected_revision: result.revision,
                                                start_byte,
                                                end_byte,
                                                replacement,
                                                description: suggestion.label(),
                                            });
                                        }
                                    }
                                });
                            }

                            ui.separator();
                        }
                    });
            });

        self.harper_window = open;

        if let Some(edit) = requested_edit {
            self.pending_external_edit = Some(edit);
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
        self.poll_analysis();
        self.poll_harper();
        self.maybe_queue_harper();
        self.maybe_autosave();

        if self.queued_revision > self.saved_revision
            || self.analysis_pending_revision.is_some()
            || self.harper_pending_revision.is_some()
        {
            ui.ctx().request_repaint_after(SAVE_STATUS_POLL);
        }

        if self.harper_enabled
            && let Some(last_edit) = self.last_edit
            && last_edit.elapsed() < HARPER_IDLE
        {
            ui.ctx()
                .request_repaint_after(HARPER_IDLE.saturating_sub(last_edit.elapsed()));
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
            self.show_structure_status(ui);
            ui.separator();
            self.show_morphology_status(ui);
            ui.separator();
            self.show_lexeme_status(ui);
            ui.separator();
            self.show_harper_status(ui);
            ui.separator();
            self.show_perf_status(ui);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak("ledger").on_hover_text(&self.path_label);
            });
        });
        ui.separator();

        self.show_rule_editor(ui.ctx());
        self.show_lexeme_window(ui.ctx());
        self.show_harper_window(ui.ctx());

        let editor_id = egui::Id::new("lexwright-ledger-editor");
        self.apply_pending_external_edit(ui.ctx(), editor_id);

        let lexical_spans = if self.structure_overlay {
            self.analysis_latest
                .as_ref()
                .filter(|analysis| analysis.revision == self.revision)
                .map(|analysis| Arc::clone(&analysis.lexical_spans))
        } else {
            None
        };

        let morph_spans = if self.morphology_overlay {
            self.analysis_latest
                .as_ref()
                .filter(|analysis| analysis.revision == self.revision)
                .map(|analysis| Arc::clone(&analysis.morph_spans))
        } else {
            None
        };

        let harper_diagnostics = if self.harper_enabled {
            self.harper_latest
                .as_ref()
                .filter(|result| result.revision == self.revision)
                .map(|result| Arc::clone(&result.diagnostics))
        } else {
            None
        };

        let editor_size = ui.available_size();
        let editor_width = editor_size.x.max(1.0);

        // Harper is deliberately NOT part of this LayoutJob. Structure/morphology may
        // color glyphs, but Harper only paints after text geometry is already final.
        let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, wrap_width: f32| {
            decorated_galley(
                ui,
                buffer.as_str(),
                wrap_width,
                lexical_spans.as_deref(),
                morph_spans.as_deref(),
            )
        };

        let editor = egui::TextEdit::multiline(&mut self.buffer)
            .font(egui::TextStyle::Monospace)
            .desired_width(editor_width)
            .min_size(editor_size)
            .lock_focus(true)
            .hint_text("Write.")
            .id(editor_id)
            .layouter(&mut layouter);

        let output = editor.show(ui);

        if let Some(diagnostics) = harper_diagnostics.as_deref() {
            paint_harper_underlines(
                ui,
                &output.galley,
                output.galley_pos,
                output.text_clip_rect,
                diagnostics,
            );
        }

        let response = &output.response;

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

fn decorated_galley(
    ui: &egui::Ui,
    text: &str,
    wrap_width: f32,
    lexical_spans: Option<&[LexicalSpan]>,
    morph_spans: Option<&[MorphSpan]>,
) -> Arc<egui::Galley> {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let default_color = ui.visuals().text_color();
    let mut job = egui::text::LayoutJob::default();

    // Wrapping is a permanent editor invariant. The callback's wrap_width is derived
    // from the TextEdit's current viewport width, and every visual state uses this same
    // geometry path. Colors/underlines may change; line breaks may not.
    job.wrap.max_width = wrap_width.max(1.0);

    if text.is_empty() {
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id,
                color: default_color,
                ..Default::default()
            },
        );
        return ui.fonts_mut(|fonts| fonts.layout_job(job));
    }

    let mut boundaries = Vec::with_capacity(
        2 + lexical_spans.map_or(0, |spans| spans.len() * 2)
            + morph_spans.map_or(0, |spans| spans.len() * 2),
    );
    boundaries.push(0);
    boundaries.push(text.len());

    if let Some(spans) = lexical_spans {
        for span in spans {
            push_valid_boundaries(text, &mut boundaries, span.start, span.end);
        }
    }

    if let Some(spans) = morph_spans {
        for span in spans {
            push_valid_boundaries(text, &mut boundaries, span.start, span.end);
        }
    }

    boundaries.sort_unstable();
    boundaries.dedup();

    for pair in boundaries.windows(2) {
        let start = pair[0];
        let end = pair[1];

        if start >= end {
            continue;
        }

        let color = morph_spans
            .and_then(|spans| {
                spans
                    .iter()
                    .find(|span| span.start <= start && end <= span.end)
                    .map(|span| morphology_color(span.class))
            })
            .or_else(|| {
                lexical_spans.and_then(|spans| {
                    spans
                        .iter()
                        .find(|span| span.start <= start && end <= span.end)
                        .map(|span| lexical_color(span.class))
                })
            })
            .unwrap_or(default_color);

        job.append(
            &text[start..end],
            0.0,
            egui::TextFormat {
                font_id: font_id.clone(),
                color,
                ..Default::default()
            },
        );
    }

    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

fn paint_harper_underlines(
    ui: &egui::Ui,
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    clip_rect: egui::Rect,
    diagnostics: &[HarperDiagnostic],
) {
    let painter = ui.painter_at(clip_rect);

    for diagnostic in diagnostics {
        if diagnostic.start_char >= diagnostic.end_char {
            continue;
        }

        let stroke = egui::Stroke::new(1.0, harper_underline_color(diagnostic.kind.as_ref()));
        let mut row_start = 0usize;

        for row in &galley.rows {
            let row_chars = row.glyphs.len();
            let row_end = row_start.saturating_add(row_chars);

            let start = diagnostic.start_char.max(row_start);
            let end = diagnostic.end_char.min(row_end);

            if start < end {
                let local_start = start - row_start;
                let local_end = end - row_start;
                let x1 =
                    galley_pos.x + row.pos.x + row.x_offset(egui::text::CharIndex(local_start));
                let x2 = galley_pos.x + row.pos.x + row.x_offset(egui::text::CharIndex(local_end));
                let y = galley_pos.y + row.pos.y + row.max_y() - 1.0;

                if x2 > x1 {
                    painter.line_segment([egui::pos2(x1, y), egui::pos2(x2, y)], stroke);
                }
            }

            row_start = row_end + usize::from(row.ends_with_newline);

            if row_start >= diagnostic.end_char {
                break;
            }
        }
    }
}

fn push_valid_boundaries(text: &str, boundaries: &mut Vec<usize>, start: usize, end: usize) {
    if start > end
        || end > text.len()
        || !text.is_char_boundary(start)
        || !text.is_char_boundary(end)
    {
        return;
    }

    boundaries.push(start);
    boundaries.push(end);
}

fn harper_underline_color(kind: &str) -> egui::Color32 {
    let kind = kind.to_ascii_lowercase();

    if kind.contains("spelling") {
        egui::Color32::from_rgb(255, 92, 92)
    } else if kind.contains("capital") {
        egui::Color32::from_rgb(255, 190, 92)
    } else {
        egui::Color32::from_rgb(255, 138, 92)
    }
}

fn morphology_color(class: MorphClass) -> egui::Color32 {
    match class {
        MorphClass::Prefix => egui::Color32::from_rgb(222, 151, 255),
        MorphClass::Stem => egui::Color32::from_rgb(255, 218, 120),
        MorphClass::Suffix => egui::Color32::from_rgb(105, 210, 180),
    }
}

fn lexical_color(class: LexicalClass) -> egui::Color32 {
    match class {
        LexicalClass::Pronoun => egui::Color32::from_rgb(222, 151, 255),
        LexicalClass::Determiner => egui::Color32::from_rgb(145, 190, 255),
        LexicalClass::Preposition => egui::Color32::from_rgb(105, 210, 220),
        LexicalClass::Conjunction => egui::Color32::from_rgb(255, 181, 103),
        LexicalClass::Auxiliary => egui::Color32::from_rgb(255, 129, 162),
        LexicalClass::VerbLike => egui::Color32::from_rgb(255, 112, 112),
        LexicalClass::AdjectiveLike => egui::Color32::from_rgb(248, 211, 106),
        LexicalClass::AdverbLike => egui::Color32::from_rgb(137, 222, 138),
        LexicalClass::NounLike => egui::Color32::from_rgb(120, 181, 255),
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
