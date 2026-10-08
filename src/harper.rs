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
const MAX_INCREMENTAL_WINDOW: usize = 4 * 1024;
const FALLBACK_CONTEXT_BYTES: usize = 768;

struct HarperJob {
    revision: u64,
    text: Arc<str>,
}

struct HarperState {
    text: Arc<str>,
    diagnostics: Arc<[HarperDiagnostic]>,
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
    pub start_char: usize,
    pub end_char: usize,
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
    pub linted_bytes: usize,
    pub total_bytes: usize,
    pub incremental: bool,
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
    let Ok(mut job) = job_rx.recv() else {
        return;
    };

    while let Ok(newer) = job_rx.try_recv() {
        job = newer;
    }

    // Lazy construction remains entirely on the Harper worker.
    let parser = PlainEnglish;
    let dictionary = FstDictionary::curated();
    let mut linter = LintGroup::new_curated(dictionary.clone(), Dialect::American);
    let mut previous: Option<HarperState> = None;

    loop {
        let started = Instant::now();

        let (diagnostics, linted_bytes, incremental) = match previous.as_ref() {
            Some(state) if state.text.as_ref() == job.text.as_ref() => {
                (Arc::clone(&state.diagnostics), 0, true)
            }
            Some(state) => {
                lint_incremental(state, &job.text, &parser, dictionary.as_ref(), &mut linter)
            }
            None => {
                let diagnostics =
                    lint_window(&job.text, 0, 0, &parser, dictionary.as_ref(), &mut linter);
                let (diagnostics, _) = cap_diagnostics(diagnostics);
                (diagnostics, job.text.len(), false)
            }
        };

        let truncated = diagnostics.len() >= MAX_DIAGNOSTICS;
        let result = HarperResult {
            revision: job.revision,
            elapsed: started.elapsed(),
            diagnostics: Arc::clone(&diagnostics),
            truncated,
            linted_bytes,
            total_bytes: job.text.len(),
            incremental,
        };

        previous = Some(HarperState {
            text: Arc::clone(&job.text),
            diagnostics,
        });

        if result_tx.send(result).is_err() {
            return;
        }

        let Ok(next) = job_rx.recv() else {
            return;
        };
        job = next;

        // If typing outruns Harper, only the newest snapshot matters. The incremental
        // diff is computed from the last completed snapshot directly to the newest one.
        while let Ok(newer) = job_rx.try_recv() {
            job = newer;
        }
    }
}

fn lint_incremental(
    previous: &HarperState,
    new_text: &str,
    parser: &PlainEnglish,
    dictionary: &FstDictionary,
    linter: &mut impl Linter,
) -> (Arc<[HarperDiagnostic]>, usize, bool) {
    let old_text = previous.text.as_ref();
    let Some(diff) = diff_ranges(old_text, new_text) else {
        return (Arc::clone(&previous.diagnostics), 0, true);
    };

    let byte_delta = new_text.len() as isize - old_text.len() as isize;
    let char_delta = new_text.chars().count() as isize - old_text.chars().count() as isize;

    // The left boundary lives in the unchanged prefix. The right boundary lives in
    // the unchanged suffix (or EOF), so it can be mapped into the new document by
    // applying the total byte delta.
    let (window_start, old_window_end) = dirty_window(old_text, diff.start, diff.old_end);
    let new_window_end = shift_index(old_window_end, byte_delta)
        .unwrap_or(new_text.len())
        .min(new_text.len());

    if window_start > new_window_end || !new_text.is_char_boundary(new_window_end) {
        let diagnostics = lint_window(new_text, 0, 0, parser, dictionary, linter);
        let (diagnostics, _) = cap_diagnostics(diagnostics);
        return (diagnostics, new_text.len(), false);
    }

    let mut merged = Vec::with_capacity(previous.diagnostics.len());

    for diagnostic in previous.diagnostics.iter() {
        if diagnostic.end_byte <= window_start {
            merged.push(diagnostic.clone());
        } else if diagnostic.start_byte >= old_window_end
            && let Some(shifted) = shift_diagnostic(diagnostic, byte_delta, char_delta)
        {
            merged.push(shifted);
        }
    }

    let base_char = new_text[..window_start].chars().count();
    merged.extend(lint_window(
        &new_text[window_start..new_window_end],
        window_start,
        base_char,
        parser,
        dictionary,
        linter,
    ));

    merged.sort_by_key(|diagnostic| (diagnostic.start_byte, diagnostic.end_byte));
    let (diagnostics, _) = cap_diagnostics(merged);

    (
        diagnostics,
        new_window_end.saturating_sub(window_start),
        true,
    )
}

fn lint_window(
    text: &str,
    base_byte: usize,
    base_char: usize,
    parser: &PlainEnglish,
    dictionary: &FstDictionary,
    linter: &mut impl Linter,
) -> Vec<HarperDiagnostic> {
    let document = Document::new(text, parser, dictionary);
    let lints = linter.lint(&document);
    let byte_offsets = char_to_byte_offsets(text);

    lints
        .into_iter()
        .filter_map(|lint| {
            let local_start_byte = *byte_offsets.get(lint.span.start)?;
            let local_end_byte = *byte_offsets.get(lint.span.end)?;

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
                start_byte: base_byte.saturating_add(local_start_byte),
                end_byte: base_byte.saturating_add(local_end_byte),
                start_char: base_char.saturating_add(lint.span.start),
                end_char: base_char.saturating_add(lint.span.end),
                kind: format!("{:?}", lint.lint_kind).into_boxed_str(),
                message: lint.message.into_boxed_str(),
                priority: lint.priority,
                suggestions,
            })
        })
        .collect()
}

fn cap_diagnostics(mut diagnostics: Vec<HarperDiagnostic>) -> (Arc<[HarperDiagnostic]>, bool) {
    let truncated = diagnostics.len() > MAX_DIAGNOSTICS;
    if truncated {
        diagnostics.truncate(MAX_DIAGNOSTICS);
    }
    (diagnostics.into(), truncated)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DiffRange {
    start: usize,
    old_end: usize,
    new_end: usize,
}

fn diff_ranges(old: &str, new: &str) -> Option<DiffRange> {
    if old == new {
        return None;
    }

    let max_prefix = old.len().min(new.len());
    let mut start = 0;

    while start < max_prefix && old.as_bytes()[start] == new.as_bytes()[start] {
        start += 1;
    }

    while start > 0 && (!old.is_char_boundary(start) || !new.is_char_boundary(start)) {
        start -= 1;
    }

    let mut suffix = 0;
    let old_remaining = old.len().saturating_sub(start);
    let new_remaining = new.len().saturating_sub(start);
    let max_suffix = old_remaining.min(new_remaining);

    while suffix < max_suffix
        && old.as_bytes()[old.len() - 1 - suffix] == new.as_bytes()[new.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let mut old_end = old.len().saturating_sub(suffix);
    let mut new_end = new.len().saturating_sub(suffix);

    while old_end < old.len()
        && new_end < new.len()
        && (!old.is_char_boundary(old_end) || !new.is_char_boundary(new_end))
    {
        old_end += 1;
        new_end += 1;
    }

    Some(DiffRange {
        start,
        old_end,
        new_end,
    })
}

fn dirty_window(text: &str, changed_start: usize, changed_end: usize) -> (usize, usize) {
    let first_sentence_start = sentence_start(text, changed_start);
    let start = if first_sentence_start > 0 {
        sentence_start(text, previous_char_boundary(text, first_sentence_start))
    } else {
        0
    };

    let first_sentence_end = sentence_end(text, changed_end);
    let end = if first_sentence_end < text.len() {
        sentence_end(text, next_char_boundary(text, first_sentence_end))
    } else {
        text.len()
    };

    if end.saturating_sub(start) <= MAX_INCREMENTAL_WINDOW {
        return (start, end);
    }

    let fallback_start = word_boundary_left(
        text,
        floor_char_boundary(text, changed_start.saturating_sub(FALLBACK_CONTEXT_BYTES)),
    );
    let fallback_end = word_boundary_right(
        text,
        ceil_char_boundary(
            text,
            changed_end
                .saturating_add(FALLBACK_CONTEXT_BYTES)
                .min(text.len()),
        ),
    );

    (
        fallback_start,
        fallback_end.max(changed_end).min(text.len()),
    )
}

fn sentence_start(text: &str, pos: usize) -> usize {
    let pos = floor_char_boundary(text, pos.min(text.len()));
    let mut boundary = 0;

    for (index, ch) in text[..pos].char_indices() {
        if is_sentence_boundary(ch) {
            boundary = index + ch.len_utf8();
        }
    }

    boundary
}

fn sentence_end(text: &str, pos: usize) -> usize {
    let pos = ceil_char_boundary(text, pos.min(text.len()));

    for (relative, ch) in text[pos..].char_indices() {
        if is_sentence_boundary(ch) {
            return pos + relative + ch.len_utf8();
        }
    }

    text.len()
}

fn is_sentence_boundary(ch: char) -> bool {
    matches!(ch, '.' | '!' | '?' | '\n')
}

fn word_boundary_left(text: &str, mut pos: usize) -> usize {
    pos = floor_char_boundary(text, pos.min(text.len()));

    while pos > 0 {
        let previous = previous_char_boundary(text, pos);
        let ch = text[previous..pos].chars().next().unwrap_or(' ');
        if ch.is_whitespace() || is_sentence_boundary(ch) {
            break;
        }
        pos = previous;
    }

    pos
}

fn word_boundary_right(text: &str, mut pos: usize) -> usize {
    pos = ceil_char_boundary(text, pos.min(text.len()));

    while pos < text.len() {
        let next = next_char_boundary(text, pos);
        let ch = text[pos..next].chars().next().unwrap_or(' ');
        pos = next;
        if ch.is_whitespace() || is_sentence_boundary(ch) {
            break;
        }
    }

    pos
}

fn previous_char_boundary(text: &str, pos: usize) -> usize {
    let mut pos = pos.min(text.len()).saturating_sub(1);
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

fn next_char_boundary(text: &str, pos: usize) -> usize {
    let mut pos = pos.min(text.len());
    if pos < text.len() {
        pos += 1;
    }
    while pos < text.len() && !text.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

fn floor_char_boundary(text: &str, mut pos: usize) -> usize {
    pos = pos.min(text.len());
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

fn ceil_char_boundary(text: &str, mut pos: usize) -> usize {
    pos = pos.min(text.len());
    while pos < text.len() && !text.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

fn shift_diagnostic(
    diagnostic: &HarperDiagnostic,
    byte_delta: isize,
    char_delta: isize,
) -> Option<HarperDiagnostic> {
    Some(HarperDiagnostic {
        start_byte: shift_index(diagnostic.start_byte, byte_delta)?,
        end_byte: shift_index(diagnostic.end_byte, byte_delta)?,
        start_char: shift_index(diagnostic.start_char, char_delta)?,
        end_char: shift_index(diagnostic.end_char, char_delta)?,
        kind: diagnostic.kind.clone(),
        message: diagnostic.message.clone(),
        priority: diagnostic.priority,
        suggestions: diagnostic.suggestions.clone(),
    })
}

fn shift_index(index: usize, delta: isize) -> Option<usize> {
    if delta >= 0 {
        index.checked_add(delta as usize)
    } else {
        index.checked_sub(delta.unsigned_abs())
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
    use super::{DiffRange, char_to_byte_offsets, diff_ranges, dirty_window, shift_index};

    #[test]
    fn character_offsets_map_back_to_utf8_bytes() {
        assert_eq!(char_to_byte_offsets("aéz"), vec![0, 1, 3, 4]);
        assert_eq!(char_to_byte_offsets(""), vec![0]);
    }

    #[test]
    fn diff_finds_small_ascii_insert() {
        assert_eq!(
            diff_ranges("hello world", "hello brave world"),
            Some(DiffRange {
                start: 6,
                old_end: 6,
                new_end: 12,
            })
        );
    }

    #[test]
    fn diff_never_splits_utf8_characters() {
        let diff = diff_ranges("café", "cafeteria").expect("missing diff");
        assert!("café".is_char_boundary(diff.start));
        assert!("cafeteria".is_char_boundary(diff.start));
        assert!("café".is_char_boundary(diff.old_end));
        assert!("cafeteria".is_char_boundary(diff.new_end));
    }

    #[test]
    fn dirty_window_is_local_for_multi_sentence_text() {
        let text = "First sentence. Second sentence has typo. Third sentence. Fourth sentence.";
        let start = text.find("typo").expect("missing typo");
        let (window_start, window_end) = dirty_window(text, start, start + 4);
        let window = &text[window_start..window_end];

        assert!(window.contains("Second sentence"));
        assert!(window.contains("Third sentence"));
        assert!(window.len() < text.len());
    }

    #[test]
    fn shift_index_handles_insertions_and_deletions() {
        assert_eq!(shift_index(10, 4), Some(14));
        assert_eq!(shift_index(10, -4), Some(6));
        assert_eq!(shift_index(2, -4), None);
    }
}
