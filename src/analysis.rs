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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexicalClass {
    Pronoun,
    Determiner,
    Preposition,
    Conjunction,
    Auxiliary,
    VerbLike,
    AdjectiveLike,
    AdverbLike,
    NounLike,
}

impl LexicalClass {
    pub const ALL: [Self; 9] = [
        Self::Pronoun,
        Self::Determiner,
        Self::Preposition,
        Self::Conjunction,
        Self::Auxiliary,
        Self::VerbLike,
        Self::AdjectiveLike,
        Self::AdverbLike,
        Self::NounLike,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Pronoun => "pronoun",
            Self::Determiner => "determiner",
            Self::Preposition => "preposition",
            Self::Conjunction => "conjunction",
            Self::Auxiliary => "auxiliary",
            Self::VerbLike => "verb-like",
            Self::AdjectiveLike => "adjective-like",
            Self::AdverbLike => "adverb-like",
            Self::NounLike => "noun-like",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Pronoun => 0,
            Self::Determiner => 1,
            Self::Preposition => 2,
            Self::Conjunction => 3,
            Self::Auxiliary => 4,
            Self::VerbLike => 5,
            Self::AdjectiveLike => 6,
            Self::AdverbLike => 7,
            Self::NounLike => 8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LexicalSpan {
    pub start: usize,
    pub end: usize,
    pub class: LexicalClass,
}

#[derive(Clone, Debug, Default)]
pub struct LexicalCounts {
    classified: [usize; 9],
    pub unclassified: usize,
}

impl LexicalCounts {
    fn increment(&mut self, class: Option<LexicalClass>) {
        match class {
            Some(class) => {
                self.classified[class.index()] =
                    self.classified[class.index()].saturating_add(1);
            }
            None => {
                self.unclassified = self.unclassified.saturating_add(1);
            }
        }
    }

    pub fn get(&self, class: LexicalClass) -> usize {
        self.classified[class.index()]
    }

    pub fn classified_total(&self) -> usize {
        self.classified.iter().copied().sum()
    }
}

#[derive(Clone, Debug)]
pub struct TextAnalysis {
    pub revision: u64,
    pub bytes: usize,
    pub chars: usize,
    pub words: usize,
    pub lines: usize,
    pub paragraphs: usize,
    pub elapsed: Duration,
    pub lexical_counts: LexicalCounts,
    pub lexical_spans: Arc<[LexicalSpan]>,
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
    let mut lines = if text.is_empty() { 0 } else { 1 };
    let mut paragraphs = 0;

    let mut line_has_content = false;
    let mut paragraph_open = false;

    for ch in text.chars() {
        chars += 1;

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

    let (words, lexical_counts, lexical_spans) = analyze_words(text);

    TextAnalysis {
        revision,
        bytes: text.len(),
        chars,
        words,
        lines,
        paragraphs,
        elapsed: started.elapsed(),
        lexical_counts,
        lexical_spans: lexical_spans.into(),
    }
}

fn analyze_words(text: &str) -> (usize, LexicalCounts, Vec<LexicalSpan>) {
    let mut words = 0;
    let mut counts = LexicalCounts::default();
    let mut spans = Vec::new();
    let mut iter = text.char_indices().peekable();

    while let Some((start, first)) = iter.next() {
        if !first.is_alphabetic() {
            continue;
        }

        let mut end = start + first.len_utf8();

        while let Some(&(byte_index, ch)) = iter.peek() {
            if ch.is_alphanumeric() || ch == '\'' || ch == '’' {
                iter.next();
                end = byte_index + ch.len_utf8();
            } else {
                break;
            }
        }

        // Do not treat a trailing apostrophe as part of the lexical token.
        while end > start
            && text[..end]
                .chars()
                .next_back()
                .is_some_and(|ch| ch == '\'' || ch == '’')
        {
            let ch = text[..end].chars().next_back().expect("non-empty token");
            end -= ch.len_utf8();
        }

        if end <= start {
            continue;
        }

        words += 1;
        let token = &text[start..end];
        let class = classify_word(token);
        counts.increment(class);

        if let Some(class) = class {
            spans.push(LexicalSpan { start, end, class });
        }
    }

    (words, counts, spans)
}

fn classify_word(word: &str) -> Option<LexicalClass> {
    if !word.is_ascii() {
        return None;
    }

    if any_eq(
        word,
        &[
            "i", "me", "my", "mine", "myself", "we", "us", "our", "ours",
            "ourselves", "you", "your", "yours", "yourself", "yourselves", "he",
            "him", "his", "himself", "she", "her", "hers", "herself", "it", "its",
            "itself", "they", "them", "their", "theirs", "themselves", "who", "whom",
            "whose", "which", "what", "someone", "anyone", "everyone", "nobody",
            "something", "anything", "everything", "nothing",
        ],
    ) {
        return Some(LexicalClass::Pronoun);
    }

    if any_eq(
        word,
        &[
            "a", "an", "the", "this", "that", "these", "those", "some", "any",
            "each", "every", "either", "neither", "no", "enough", "much", "many",
            "few", "several", "all", "both",
        ],
    ) {
        return Some(LexicalClass::Determiner);
    }

    if any_eq(
        word,
        &[
            "about", "above", "across", "after", "against", "along", "among", "around",
            "at", "before", "behind", "below", "beneath", "beside", "between", "beyond",
            "by", "despite", "down", "during", "except", "for", "from", "in", "inside",
            "into", "near", "of", "off", "on", "onto", "out", "outside", "over",
            "past", "through", "throughout", "to", "toward", "under", "underneath",
            "until", "up", "upon", "with", "within", "without",
        ],
    ) {
        return Some(LexicalClass::Preposition);
    }

    if any_eq(
        word,
        &[
            "and", "or", "but", "nor", "yet", "so", "although", "because", "since",
            "unless", "while", "whereas", "if", "when", "whenever", "though",
        ],
    ) {
        return Some(LexicalClass::Conjunction);
    }

    if any_eq(
        word,
        &[
            "be", "am", "is", "are", "was", "were", "been", "being", "have", "has",
            "had", "do", "does", "did", "can", "could", "may", "might", "must",
            "shall", "should", "will", "would",
        ],
    ) {
        return Some(LexicalClass::Auxiliary);
    }

    if any_eq(
        word,
        &[
            "not", "very", "too", "also", "just", "only", "never", "always", "often",
            "sometimes", "then", "now", "here", "there", "already", "still",
        ],
    ) || any_suffix(word, &["ly"])
    {
        return Some(LexicalClass::AdverbLike);
    }

    if any_eq(
        word,
        &[
            "good", "bad", "new", "old", "great", "little", "big", "high", "different",
            "small", "large", "next", "early", "young", "important", "public", "same",
            "able",
        ],
    ) || any_suffix(
        word,
        &[
            "ous", "ful", "ive", "al", "able", "ible", "less", "ic", "ish", "ary",
        ],
    ) {
        return Some(LexicalClass::AdjectiveLike);
    }

    if any_eq(
        word,
        &[
            "say", "says", "said", "make", "makes", "made", "go", "goes", "went",
            "gone", "get", "gets", "got", "know", "knows", "knew", "think", "thinks",
            "thought", "take", "takes", "took", "see", "sees", "saw", "come", "comes",
            "came", "want", "wants", "look", "looks", "use", "uses", "find", "found",
            "give", "gave", "tell", "told", "work", "works", "call", "try", "ask",
            "need", "feel", "felt", "become", "became", "leave", "left", "put", "keep",
            "kept", "let", "begin", "began", "seem", "help", "talk", "turn", "start",
            "show", "hear", "play", "run", "move", "live", "believe", "bring", "happen",
            "write", "provide", "sit", "stand", "lose", "pay", "meet", "include",
            "continue", "set", "learn", "change", "lead", "understand", "watch",
            "follow", "stop", "create", "speak", "read", "allow", "add", "spend",
            "grow", "open", "walk", "win", "offer", "remember", "love", "consider",
            "appear", "buy", "wait", "serve", "die", "send", "expect", "build", "stay",
            "fall", "cut", "reach", "kill", "remain", "suggest", "raise", "pass",
            "sell", "require", "report", "decide", "pull",
        ],
    ) || any_suffix(word, &["ing", "ed", "ize", "ise", "ify", "ate"])
    {
        return Some(LexicalClass::VerbLike);
    }

    if any_suffix(
        word,
        &[
            "tion", "sion", "ment", "ness", "ity", "ship", "ism", "ist", "ance",
            "ence", "hood", "dom",
        ],
    ) {
        return Some(LexicalClass::NounLike);
    }

    None
}

fn any_eq(word: &str, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| word.eq_ignore_ascii_case(candidate))
}

fn any_suffix(word: &str, suffixes: &[&str]) -> bool {
    suffixes.iter().any(|suffix| {
        word.len() >= suffix.len()
            && word[word.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
    })
}

#[cfg(test)]
mod tests {
    use super::{LexicalClass, analyze, classify_word};

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

    #[test]
    fn classifies_closed_class_words_exactly() {
        assert_eq!(classify_word("THE"), Some(LexicalClass::Determiner));
        assert_eq!(classify_word("they"), Some(LexicalClass::Pronoun));
        assert_eq!(classify_word("with"), Some(LexicalClass::Preposition));
        assert_eq!(classify_word("would"), Some(LexicalClass::Auxiliary));
    }

    #[test]
    fn marks_open_class_guesses_as_like_categories() {
        assert_eq!(
            classify_word("beautiful"),
            Some(LexicalClass::AdjectiveLike)
        );
        assert_eq!(classify_word("quickly"), Some(LexicalClass::AdverbLike));
        assert_eq!(classify_word("running"), Some(LexicalClass::VerbLike));
        assert_eq!(classify_word("happiness"), Some(LexicalClass::NounLike));
    }

    #[test]
    fn preserves_byte_ranges_for_unicode_context() {
        let result = analyze(1, "é the cat");
        assert_eq!(result.words, 3);
        assert_eq!(result.lexical_spans.len(), 1);
        let span = result.lexical_spans[0];
        assert_eq!(&"é the cat"[span.start..span.end], "the");
    }
}
