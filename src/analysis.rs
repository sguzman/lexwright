use std::{
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

struct AnalysisJob {
    revision: u64,
    text: Arc<str>,
}

#[derive(Clone, Copy, Debug)]
pub struct TextAnalysis {
    pub revision: u64,
    pub bytes: usize,
    pub chars: usize,
    pub words: usize,
    pub lines: usize,
    pub paragraphs: usize,
    pub elapsed: Duration,
}

pub struct AnalysisWorker {
    job_tx: Sender<AnalysisJob>,
    result_rx: Receiver<TextAnalysis>,
}

impl AnalysisWorker {
    pub fn new() -> Self {
        let (job_tx, job_rx) = mpsc::channel::<AnalysisJob>();
        let (result_tx, result_rx) = mpsc::channel::<TextAnalysis>();

        thread::Builder::new()
            .name("lexwright-analysis".to_owned())
            .spawn(move || analysis_worker(job_rx, result_tx))
            .expect("failed to start analysis worker");

        Self { job_tx, result_rx }
    }

    pub fn queue(&self, revision: u64, text: Arc<str>) -> Result<(), String> {
        self.job_tx
            .send(AnalysisJob { revision, text })
            .map_err(|_| "analysis worker stopped".to_owned())
    }

    pub fn poll(&self) -> Option<TextAnalysis> {
        self.result_rx.try_recv().ok()
    }
}

fn analysis_worker(job_rx: Receiver<AnalysisJob>, result_tx: Sender<TextAnalysis>) {
    while let Ok(mut job) = job_rx.recv() {
        // Analysis is observational. If typing outruns us, stale work has no value:
        // collapse the queue to the newest immutable snapshot before doing any work.
        while let Ok(newer) = job_rx.try_recv() {
            job = newer;
        }

        let result = analyze(job.revision, &job.text);
        if result_tx.send(result).is_err() {
            break;
        }
    }
}

fn analyze(revision: u64, text: &str) -> TextAnalysis {
    let started = Instant::now();
    let mut chars = 0;
    let mut words = 0;
    let mut lines = if text.is_empty() { 0 } else { 1 };
    let mut paragraphs = 0;

    let mut in_word = false;
    let mut line_has_content = false;
    let mut paragraph_open = false;

    for ch in text.chars() {
        chars += 1;

        let word_char = ch.is_alphanumeric() || (ch == '\'' && in_word);
        if word_char {
            if !in_word {
                words += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }

        if ch == '\n' {
            lines += 1;
            if line_has_content {
                if !paragraph_open {
                    paragraphs += 1;
                    paragraph_open = true;
                }
            } else {
                paragraph_open = false;
            }
            line_has_content = false;
        } else if !ch.is_whitespace() {
            line_has_content = true;
        }
    }

    if line_has_content && !paragraph_open {
        paragraphs += 1;
    }

    TextAnalysis {
        revision,
        bytes: text.len(),
        chars,
        words,
        lines,
        paragraphs,
        elapsed: started.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::analyze;

    #[test]
    fn counts_basic_text_without_claiming_full_nlp() {
        let result = analyze(7, "Hello world.\n\nDon't panic.");
        assert_eq!(result.revision, 7);
        assert_eq!(result.words, 4);
        assert_eq!(result.lines, 3);
        assert_eq!(result.paragraphs, 2);
        assert_eq!(result.chars, 26);
    }

    #[test]
    fn empty_document_has_zero_lines_and_paragraphs() {
        let result = analyze(0, "");
        assert_eq!(result.words, 0);
        assert_eq!(result.lines, 0);
        assert_eq!(result.paragraphs, 0);
    }
}
