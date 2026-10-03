use std::{
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use harper_core::{
    Dialect, Document,
    linting::{LintGroup, Linter, Suggestion},
    parsers::PlainEnglish,
    spell::FstDictionary,
};

const MAX_DIAGNOSTICS: usize = 256;

struct HarperJob {
    revision: u64,
    text: Arc<str>,
}

#[derive(Clone, Debug)]
pub enum HarperSuggestion {
    ReplaceWith(Box<str>),
    InsertAfter(Box<str>),
    Remove,
}

impl HarperSuggestion {
    pub fn label(&self) -> String {
        match self {
            Self::ReplaceWith(text) => format!("replace with {text:?}"),
            Self::InsertAfter(text) => format!("insert {text:?} after"),
            Self::Remove => "remove".to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct HarperDiagnostic {
    pub start_byte: usize,
    pub end_byte: usize,
    pub kind: Box<str>,
    pub message: Box<str>,
    pub priority: u8,
    pub suggestions: Box<[HarperSuggestion]>,
}

#[derive(Clone, Debug)]
pub struct HarperResult {
    pub revision: u64,
    pub elapsed: Duration,
    pub diagnostics: Arc<[HarperDiagnostic]>,
    pub truncated: bool,
}

pub struct HarperWorker {
    job_tx: Sender<HarperJob>,
    result_rx: Receiver<HarperResult>,
}

impl HarperWorker {
    pub fn new() -> Self {
        let (job_tx, job_rx) = mpsc::channel::<HarperJob>();
        let (result_tx, result_rx) = mpsc::channel::<HarperResult>();

        thread::Builder::new()
            .name("lexwright-harper".to_owned())
            .spawn(move || harper_worker(job_rx, result_tx))
            .expect("failed to start Harper worker");

        Self { job_tx, result_rx }
    }

    pub fn queue(&self, revision: u64, text: Arc<str>) -> Result<(), String> {
        self.job_tx
            .send(HarperJob { revision, text })
            .map_err(|_| "Harper worker stopped".to_owned())
    }

    pub fn poll(&self) -> Option<HarperResult> {
        self.result_rx.try_recv().ok()
    }
}

fn harper_worker(job_rx: Receiver<HarperJob>, result_tx: Sender<HarperResult>) {
    // Harper is intentionally initialized inside its own worker thread. Dictionary
    // construction must never extend Lexwright's process-start -> first-frame path.
    let parser = PlainEnglish;
    let mut linter = LintGroup::new_curated(FstDictionary::curated(), Dialect::American);

    while let Ok(mut job) = job_rx.recv() {
        // Grammar work is observational. Drop queued stale snapshots before beginning
        // another potentially expensive lint pass.
        while let Ok(newer) = job_rx.try_recv() {
            job = newer;
        }

        let started = Instant::now();
        let document = Document::new_curated(job.text.as_ref(), &parser);
        let lints = linter.lint(&document);
        let total = lints.len();
        let byte_offsets = char_to_byte_offsets(&job.text);

        let diagnostics = lints
            .into_iter()
            .take(MAX_DIAGNOSTICS)
            .filter_map(|lint| {
                let start_byte = *byte_offsets.get(lint.span.start)?;
                let end_byte = *byte_offsets.get(lint.span.end)?;

                let suggestions = lint
                    .suggestions
                    .into_iter()
                    .map(|suggestion| match suggestion {
                        Suggestion::ReplaceWith(chars) => HarperSuggestion::ReplaceWith(
                            chars.into_iter().collect::<String>().into_boxed_str(),
                        ),
                        Suggestion::InsertAfter(chars) => HarperSuggestion::InsertAfter(
                            chars.into_iter().collect::<String>().into_boxed_str(),
                        ),
                        Suggestion::Remove => HarperSuggestion::Remove,
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();

                Some(HarperDiagnostic {
                    start_byte,
                    end_byte,
                    kind: format!("{:?}", lint.lint_kind).into_boxed_str(),
                    message: lint.message.into_boxed_str(),
                    priority: lint.priority,
                    suggestions,
                })
            })
            .collect::<Vec<_>>();

        let result = HarperResult {
            revision: job.revision,
            elapsed: started.elapsed(),
            diagnostics: diagnostics.into(),
            truncated: total > MAX_DIAGNOSTICS,
        };

        if result_tx.send(result).is_err() {
            break;
        }
    }
}

fn char_to_byte_offsets(text: &str) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(text.chars().count().saturating_add(1));
    offsets.extend(text.char_indices().map(|(byte, _)| byte));
    offsets.push(text.len());
    offsets
}

#[cfg(test)]
mod tests {
    use super::char_to_byte_offsets;

    #[test]
    fn character_offsets_map_back_to_utf8_bytes() {
        assert_eq!(char_to_byte_offsets("aéz"), vec![0, 1, 3, 4]);
        assert_eq!(char_to_byte_offsets(""), vec![0]);
    }
}
