use std::{sync::Arc, time::Instant};

use crate::{
    analysis::{AnalysisWorker, TextAnalysis},
    editor_buffer::EditorBuffer,
    harper::{HarperDiagnostic, HarperResult, HarperWorker},
    storage::LedgerStore,
};

/// State that belongs to one durable Lexwright document.
///
/// Lexwright is still a single-document UI today, but this boundary is deliberately
/// separate from application-global chrome/settings so future tabs can own independent
/// buffers, revision clocks, persistence lanes, analyzer freshness, and suggestion state.
pub struct DocumentState {
    pub(crate) buffer: EditorBuffer,
    pub(crate) revision: u64,
    pub(crate) queued_revision: u64,
    pub(crate) saved_revision: u64,
    pub(crate) last_edit: Option<Instant>,
    pub(crate) save_error: Option<String>,
    pub(crate) store: LedgerStore,
    pub(crate) path_label: String,
    pub(crate) analysis_worker: AnalysisWorker,
    pub(crate) analysis_latest: Option<TextAnalysis>,
    pub(crate) analysis_pending_revision: Option<u64>,
    pub(crate) analysis_error: Option<String>,
    pub(crate) harper_worker: HarperWorker,
    pub(crate) harper_latest: Option<HarperResult>,
    pub(crate) harper_display_revision: Option<u64>,
    pub(crate) harper_display_diagnostics: Arc<[HarperDiagnostic]>,
    pub(crate) harper_pending_revision: Option<u64>,
    pub(crate) harper_error: Option<String>,
    pub(crate) pending_external_edit: Option<PendingExternalEdit>,
    pub(crate) harper_action_status: Option<String>,
    pub(crate) snapshot_revision: Option<u64>,
    pub(crate) snapshot_cache: Option<Arc<str>>,
}

pub(crate) struct PendingExternalEdit {
    pub(crate) expected_revision: u64,
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) replacement: String,
    pub(crate) description: String,
}

impl DocumentState {
    pub(crate) fn load_default() -> Self {
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

        // Harper is intentionally lazy. Merely opening a document must not initialize
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
            analysis_worker,
            analysis_latest: None,
            analysis_pending_revision,
            analysis_error,
            harper_worker,
            harper_latest: None,
            harper_display_revision: None,
            harper_display_diagnostics: Arc::from([]),
            harper_pending_revision,
            harper_error,
            pending_external_edit: None,
            harper_action_status: None,
            snapshot_revision: Some(0),
            snapshot_cache: Some(initial_snapshot),
        }
    }
}
