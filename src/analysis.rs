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
                self.classified[class.index()] = self.classified[class.index()].saturating_add(1);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MorphClass {
    Prefix,
    Stem,
    Suffix,
}

#[derive(Clone, Copy, Debug)]
pub struct MorphSpan {
    pub start: usize,
    pub end: usize,
    pub class: MorphClass,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MorphCounts {
    pub decomposed_words: usize,
    pub prefixes: usize,
    pub suffixes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexemeRule {
    SurfaceStem,
    IToY,
    UndoubleFinalConsonant,
    RestoreFinalE,
    RestoreFinalLe,
}

impl LexemeRule {
    pub fn label(self) -> &'static str {
        match self {
            Self::SurfaceStem => "surface stem",
            Self::IToY => "i→y",
            Self::UndoubleFinalConsonant => "undouble final consonant",
            Self::RestoreFinalE => "restore final e",
            Self::RestoreFinalLe => "restore final le",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexemeCandidate {
    pub word_start: usize,
    pub word_end: usize,
    pub stem_start: usize,
    pub stem_end: usize,
    pub lexeme: Box<str>,
    pub rule: LexemeRule,
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
    pub morph_counts: MorphCounts,
    pub morph_spans: Arc<[MorphSpan]>,
    pub lexeme_candidates: Arc<[LexemeCandidate]>,
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

    let (
        words,
        lexical_counts,
        lexical_spans,
        morph_counts,
        morph_spans,
        lexeme_candidates,
    ) = analyze_words(text);

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
        morph_counts,
        morph_spans: morph_spans.into(),
        lexeme_candidates: lexeme_candidates.into(),
    }
}

fn analyze_words(
    text: &str,
) -> (
    usize,
    LexicalCounts,
    Vec<LexicalSpan>,
    MorphCounts,
    Vec<MorphSpan>,
    Vec<LexemeCandidate>,
) {
    let mut words = 0;
    let mut counts = LexicalCounts::default();
    let mut spans = Vec::new();
    let mut morph_counts = MorphCounts::default();
    let mut morph_spans = Vec::new();
    let mut lexeme_candidates = Vec::new();
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

        analyze_morphology(
            token,
            start,
            &mut morph_counts,
            &mut morph_spans,
            &mut lexeme_candidates,
        );
    }

    (
        words,
        counts,
        spans,
        morph_counts,
        morph_spans,
        lexeme_candidates,
    )
}

fn analyze_morphology(
    word: &str,
    absolute_start: usize,
    counts: &mut MorphCounts,
    spans: &mut Vec<MorphSpan>,
    lexemes: &mut Vec<LexemeCandidate>,
) {
    if !word.is_ascii() || word.len() < 5 {
        return;
    }

    const EXCEPTIONS: &[&str] = &[
        "anything",
        "ceiling",
        "during",
        "everything",
        "morning",
        "nothing",
        "something",
    ];

    if any_eq(word, EXCEPTIONS) {
        return;
    }

    const PREFIXES: &[&str] = &[
        "counter", "under", "inter", "trans", "super", "over", "anti", "auto", "post", "pre",
        "sub", "non", "dis", "mis", "un", "re", "de", "en", "em",
    ];
    const SUFFIXES: &[&str] = &[
        "ization", "isation", "ability", "ibility", "ically", "ingly", "edly", "tion", "sion",
        "ment", "ness", "ance", "ence", "hood", "ship", "able", "ible", "less", "ful", "ous",
        "ive", "ize", "ise", "ify", "ing", "est", "ed", "ly", "er", "ism", "ist", "ity", "al",
        "ic", "s",
    ];

    let mut suffix_cursor = word.len();
    let mut suffix_ranges = Vec::new();

    for _ in 0..2 {
        let remaining = &word[..suffix_cursor];
        let Some(suffix) = SUFFIXES
            .iter()
            .copied()
            .find(|suffix| suffix_is_supported(remaining, suffix))
        else {
            break;
        };

        let start = suffix_cursor - suffix.len();
        suffix_ranges.push((start, suffix_cursor));
        suffix_cursor = start;

        // Once an inflectional ending is removed, do not immediately reinterpret the
        // resulting lexical base as another derivation. This is what previously turned
        // "decisions" into "ci + sion + s".
        if matches!(suffix, "s" | "ed" | "ing" | "er" | "est") {
            break;
        }
    }

    let nearest_suffix = suffix_ranges
        .last()
        .map(|(start, end)| &word[*start..*end]);

    let mut prefix_cursor = 0;
    let mut prefix_ranges = Vec::new();

    for _ in 0..2 {
        let remaining = &word[prefix_cursor..suffix_cursor];
        let Some(prefix) = PREFIXES.iter().copied().find(|prefix| {
            if remaining.len() < prefix.len() + 3
                || !starts_with_ascii_case(remaining, prefix)
            {
                return false;
            }

            let candidate = &remaining[prefix.len()..];
            let (normalized, _) = normalize_lexeme(candidate, nearest_suffix);
            is_known_morph_base(&normalized)
        }) else {
            break;
        };

        let end = prefix_cursor + prefix.len();
        prefix_ranges.push((prefix_cursor, end));
        prefix_cursor = end;
    }

    if prefix_ranges.is_empty() && suffix_ranges.is_empty() {
        return;
    }

    if prefix_cursor >= suffix_cursor {
        return;
    }

    let surface_stem = &word[prefix_cursor..suffix_cursor];
    let (lexeme, rule) = normalize_lexeme(surface_stem, nearest_suffix);

    if !is_known_morph_base(&lexeme)
        && !surface_stem_has_productive_shape(surface_stem, nearest_suffix)
    {
        return;
    }

    counts.decomposed_words = counts.decomposed_words.saturating_add(1);

    lexemes.push(LexemeCandidate {
        word_start: absolute_start,
        word_end: absolute_start + word.len(),
        stem_start: absolute_start + prefix_cursor,
        stem_end: absolute_start + suffix_cursor,
        lexeme: lexeme.into_boxed_str(),
        rule,
    });

    for (start, end) in prefix_ranges {
        spans.push(MorphSpan {
            start: absolute_start + start,
            end: absolute_start + end,
            class: MorphClass::Prefix,
        });
        counts.prefixes = counts.prefixes.saturating_add(1);
    }

    spans.push(MorphSpan {
        start: absolute_start + prefix_cursor,
        end: absolute_start + suffix_cursor,
        class: MorphClass::Stem,
    });

    suffix_ranges.reverse();
    for (start, end) in suffix_ranges {
        spans.push(MorphSpan {
            start: absolute_start + start,
            end: absolute_start + end,
            class: MorphClass::Suffix,
        });
        counts.suffixes = counts.suffixes.saturating_add(1);
    }
}

fn suffix_is_supported(word: &str, suffix: &str) -> bool {
    if word.len() < suffix.len() + 3 || !ends_with_ascii_case(word, suffix) {
        return false;
    }

    let stem = &word[..word.len() - suffix.len()];

    match suffix {
        "s" => {
            if any_eq(word, &["this", "his", "is", "was", "has", "us", "yes"])
                || word.ends_with("ss")
            {
                return false;
            }

            is_known_morph_base(stem)
                || any_suffix(
                    stem,
                    &[
                        "tion", "sion", "ment", "ness", "ity", "ship", "ism", "ist", "ance",
                        "ence", "hood", "dom",
                    ],
                )
        }
        "ing" | "ed" => {
            let (normalized, _) = normalize_lexeme(stem, Some(suffix));
            is_known_morph_base(&normalized)
        }
        "er" | "est" => {
            let (normalized, _) = normalize_lexeme(stem, Some(suffix));
            is_known_morph_base(&normalized)
        }
        "ly" => {
            let (normalized, _) = normalize_lexeme(stem, Some(suffix));
            is_known_morph_base(&normalized)
        }
        "able" | "ible" => {
            let candidate = base_after_one_supported_prefix(stem, Some(suffix));
            is_known_morph_base(&candidate)
        }
        "ness" => {
            let candidate = base_after_one_supported_prefix(stem, Some(suffix));
            is_known_morph_base(&candidate)
                || any_suffix(stem, &["ful", "less", "ous", "ive", "al", "able", "ible", "ic"])
        }
        "ful" | "less" => {
            let candidate = base_after_one_supported_prefix(stem, Some(suffix));
            is_known_morph_base(&candidate)
        }
        "tion" | "sion" | "ment" | "ance" | "ence" | "hood" | "ship" | "ism" | "ist"
        | "ity" | "al" | "ic" | "ous" | "ive" | "ize" | "ise" | "ify" | "ization"
        | "isation" | "ability" | "ibility" | "ically" | "ingly" | "edly" => {
            let candidate = base_after_one_supported_prefix(stem, Some(suffix));
            is_known_morph_base(&candidate)
        }
        _ => false,
    }
}

fn base_after_one_supported_prefix(stem: &str, suffix: Option<&str>) -> String {
    const PREFIXES: &[&str] = &[
        "counter", "under", "inter", "trans", "super", "over", "anti", "auto", "post", "pre",
        "sub", "non", "dis", "mis", "un", "re", "de", "en", "em",
    ];

    let (normalized, _) = normalize_lexeme(stem, suffix);
    if is_known_morph_base(&normalized) {
        return normalized;
    }

    for prefix in PREFIXES {
        if stem.len() < prefix.len() + 3 || !starts_with_ascii_case(stem, prefix) {
            continue;
        }

        let candidate = &stem[prefix.len()..];
        let (normalized, _) = normalize_lexeme(candidate, suffix);
        if is_known_morph_base(&normalized) {
            return normalized;
        }
    }

    normalized
}

fn surface_stem_has_productive_shape(stem: &str, suffix: Option<&str>) -> bool {
    let Some(suffix) = suffix else {
        return false;
    };

    matches!(suffix, "ness" | "ful" | "less" | "able" | "ible")
        && stem.len() >= 4
}

fn normalize_lexeme(stem: &str, nearest_suffix: Option<&str>) -> (String, LexemeRule) {
    let Some(suffix) = nearest_suffix else {
        return (stem.to_owned(), LexemeRule::SurfaceStem);
    };

    if matches!(suffix, "ness" | "ly") && stem.len() >= 2 && stem.ends_with('i') {
        let mut lexeme = stem[..stem.len() - 1].to_owned();
        lexeme.push('y');
        if is_known_morph_base(&lexeme) {
            return (lexeme, LexemeRule::IToY);
        }
    }

    if matches!(suffix, "ing" | "ed" | "er" | "est") {
        let mut chars = stem.chars().rev();
        if let (Some(last), Some(previous)) = (chars.next(), chars.next())
            && last == previous
            && matches!(
                last.to_ascii_lowercase(),
                'b' | 'd' | 'g' | 'm' | 'n' | 'p' | 'r' | 't'
            )
        {
            let mut lexeme = stem.to_owned();
            lexeme.pop();
            if is_known_morph_base(&lexeme) {
                return (lexeme, LexemeRule::UndoubleFinalConsonant);
            }
        }
    }

    if suffix.eq_ignore_ascii_case("ly") {
        let mut lexeme = stem.to_owned();
        lexeme.push_str("le");
        if is_known_morph_base(&lexeme) {
            return (lexeme, LexemeRule::RestoreFinalLe);
        }
    }

    if matches!(suffix, "able" | "ible" | "ing" | "ed") {
        let mut lexeme = stem.to_owned();
        lexeme.push('e');
        if is_known_morph_base(&lexeme) {
            return (lexeme, LexemeRule::RestoreFinalE);
        }
    }

    (stem.to_owned(), LexemeRule::SurfaceStem)
}

fn is_known_morph_base(word: &str) -> bool {
    any_eq(
        word,
        &[
            "able", "act", "appear", "ask", "bad", "be", "begin", "believe", "big", "bring",
            "build", "buy", "call", "care", "change", "consider", "continue", "create", "cut",
            "decide", "decision", "different", "die", "early", "expect", "fall", "feel", "find",
            "follow", "give", "good", "great", "grow", "happen", "happy", "hear", "help", "high",
            "hope", "include", "keep", "kind", "know", "large", "lead", "learn", "leave", "little",
            "live", "look", "lose", "love", "make", "meet", "move", "need", "new", "old", "open",
            "offer", "pay", "play", "probable", "provide", "public", "pull", "quick", "raise",
            "reach", "read", "real", "remain", "remember", "report", "require", "run", "same",
            "say", "see", "sell", "send", "serve", "set", "show", "sit", "small", "speak", "spend",
            "stand", "start", "stay", "stop", "suggest", "take", "talk", "tell", "think", "try",
            "turn", "understand", "use", "wait", "walk", "want", "watch", "win", "work", "write",
            "young",
        ],
    )
}

fn starts_with_ascii_case(word: &str, prefix: &str) -> bool {
    word.len() >= prefix.len() && word[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn ends_with_ascii_case(word: &str, suffix: &str) -> bool {
    word.len() >= suffix.len() && word[word.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

fn classify_word(word: &str) -> Option<LexicalClass> {
    if !word.is_ascii() {
        return None;
    }

    if any_eq(
        word,
        &[
            "i",
            "me",
            "my",
            "mine",
            "myself",
            "we",
            "us",
            "our",
            "ours",
            "ourselves",
            "you",
            "your",
            "yours",
            "yourself",
            "yourselves",
            "he",
            "him",
            "his",
            "himself",
            "she",
            "her",
            "hers",
            "herself",
            "it",
            "its",
            "itself",
            "they",
            "them",
            "their",
            "theirs",
            "themselves",
            "who",
            "whom",
            "whose",
            "which",
            "what",
            "someone",
            "anyone",
            "everyone",
            "nobody",
            "something",
            "anything",
            "everything",
            "nothing",
        ],
    ) {
        return Some(LexicalClass::Pronoun);
    }

    if any_eq(
        word,
        &[
            "a", "an", "the", "this", "that", "these", "those", "some", "any", "each", "every",
            "either", "neither", "no", "enough", "much", "many", "few", "several", "all", "both",
        ],
    ) {
        return Some(LexicalClass::Determiner);
    }

    if any_eq(
        word,
        &[
            "about",
            "above",
            "across",
            "after",
            "against",
            "along",
            "among",
            "around",
            "at",
            "before",
            "behind",
            "below",
            "beneath",
            "beside",
            "between",
            "beyond",
            "by",
            "despite",
            "down",
            "during",
            "except",
            "for",
            "from",
            "in",
            "inside",
            "into",
            "near",
            "of",
            "off",
            "on",
            "onto",
            "out",
            "outside",
            "over",
            "past",
            "through",
            "throughout",
            "to",
            "toward",
            "under",
            "underneath",
            "until",
            "up",
            "upon",
            "with",
            "within",
            "without",
        ],
    ) {
        return Some(LexicalClass::Preposition);
    }

    if any_eq(
        word,
        &[
            "and", "or", "but", "nor", "yet", "so", "although", "because", "since", "unless",
            "while", "whereas", "if", "when", "whenever", "though",
        ],
    ) {
        return Some(LexicalClass::Conjunction);
    }

    if any_eq(
        word,
        &[
            "be", "am", "is", "are", "was", "were", "been", "being", "have", "has", "had", "do",
            "does", "did", "can", "could", "may", "might", "must", "shall", "should", "will",
            "would",
        ],
    ) {
        return Some(LexicalClass::Auxiliary);
    }

    if any_eq(
        word,
        &[
            "not",
            "very",
            "too",
            "also",
            "just",
            "only",
            "never",
            "always",
            "often",
            "sometimes",
            "then",
            "now",
            "here",
            "there",
            "already",
            "still",
        ],
    ) || any_suffix(word, &["ly"])
    {
        return Some(LexicalClass::AdverbLike);
    }

    if any_eq(
        word,
        &[
            "good",
            "bad",
            "new",
            "old",
            "great",
            "little",
            "big",
            "high",
            "different",
            "small",
            "large",
            "next",
            "early",
            "young",
            "important",
            "public",
            "same",
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
            "say",
            "says",
            "said",
            "make",
            "makes",
            "made",
            "go",
            "goes",
            "went",
            "gone",
            "get",
            "gets",
            "got",
            "know",
            "knows",
            "knew",
            "think",
            "thinks",
            "thought",
            "take",
            "takes",
            "took",
            "see",
            "sees",
            "saw",
            "come",
            "comes",
            "came",
            "want",
            "wants",
            "look",
            "looks",
            "use",
            "uses",
            "find",
            "found",
            "give",
            "gave",
            "tell",
            "told",
            "work",
            "works",
            "call",
            "try",
            "ask",
            "need",
            "feel",
            "felt",
            "become",
            "became",
            "leave",
            "left",
            "put",
            "keep",
            "kept",
            "let",
            "begin",
            "began",
            "seem",
            "help",
            "talk",
            "turn",
            "start",
            "show",
            "hear",
            "play",
            "run",
            "move",
            "live",
            "believe",
            "bring",
            "happen",
            "write",
            "provide",
            "sit",
            "stand",
            "lose",
            "pay",
            "meet",
            "include",
            "continue",
            "set",
            "learn",
            "change",
            "lead",
            "understand",
            "watch",
            "follow",
            "stop",
            "create",
            "speak",
            "read",
            "allow",
            "add",
            "spend",
            "grow",
            "open",
            "walk",
            "win",
            "offer",
            "remember",
            "love",
            "consider",
            "appear",
            "buy",
            "wait",
            "serve",
            "die",
            "send",
            "expect",
            "build",
            "stay",
            "fall",
            "cut",
            "reach",
            "kill",
            "remain",
            "suggest",
            "raise",
            "pass",
            "sell",
            "require",
            "report",
            "decide",
            "pull",
        ],
    ) || any_suffix(word, &["ing", "ed", "ize", "ise", "ify", "ate"])
    {
        return Some(LexicalClass::VerbLike);
    }

    if any_suffix(
        word,
        &[
            "tion", "sion", "ment", "ness", "ity", "ship", "ism", "ist", "ance", "ence", "hood",
            "dom",
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
        word.len() >= suffix.len() && word[word.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        LexemeRule, LexicalClass, MorphClass, analyze, analyze_morphology, classify_word,
        normalize_lexeme,
    };

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
    fn decomposes_multiple_affix_layers_without_claiming_lemmatization() {
        let mut counts = super::MorphCounts::default();
        let mut spans = Vec::new();
        let mut lexemes = Vec::new();
        analyze_morphology(
            "unhelpfulness",
            0,
            &mut counts,
            &mut spans,
            &mut lexemes,
        );

        assert_eq!(counts.decomposed_words, 1);
        assert_eq!(counts.prefixes, 1);
        assert_eq!(counts.suffixes, 2);
        assert_eq!(lexemes.len(), 1);
        assert_eq!(lexemes[0].lexeme.as_ref(), "help");

        let pieces: Vec<_> = spans
            .iter()
            .map(|span| (&"unhelpfulness"[span.start..span.end], span.class))
            .collect();

        assert_eq!(
            pieces,
            vec![
                ("un", MorphClass::Prefix),
                ("help", MorphClass::Stem),
                ("ful", MorphClass::Suffix),
                ("ness", MorphClass::Suffix),
            ]
        );
    }

    #[test]
    fn normalizes_conservative_lexeme_spelling_alternations() {
        assert_eq!(
            normalize_lexeme("happi", Some("ness")),
            ("happy".to_owned(), LexemeRule::IToY)
        );
        assert_eq!(
            normalize_lexeme("runn", Some("ing")),
            ("run".to_owned(), LexemeRule::UndoubleFinalConsonant)
        );
        assert_eq!(
            normalize_lexeme("believ", Some("able")),
            ("believe".to_owned(), LexemeRule::RestoreFinalE)
        );
        assert_eq!(
            normalize_lexeme("probab", Some("ly")),
            ("probable".to_owned(), LexemeRule::RestoreFinalLe)
        );
        assert_eq!(
            normalize_lexeme("quick", Some("ly")),
            ("quick".to_owned(), LexemeRule::SurfaceStem)
        );
    }

    #[test]
    fn morphology_skips_obvious_false_suffix_words() {
        let result = analyze(3, "something nothing everything anything");
        assert!(result.morph_spans.is_empty());
        assert!(result.lexeme_candidates.is_empty());
    }

    #[test]
    fn sample_words_keep_only_supported_morphology() {
        let text = "really probably quickly reconsider unbelievable unhelpfulness decisions";
        let result = analyze(4, text);

        let candidates: Vec<_> = result
            .lexeme_candidates
            .iter()
            .map(|candidate| {
                (
                    &text[candidate.word_start..candidate.word_end],
                    candidate.lexeme.as_ref(),
                )
            })
            .collect();

        assert!(candidates.contains(&("really", "real")));
        assert!(candidates.contains(&("probably", "probable")));
        assert!(candidates.contains(&("quickly", "quick")));
        assert!(candidates.contains(&("reconsider", "consider")));
        assert!(candidates.contains(&("unbelievable", "believe")));
        assert!(candidates.contains(&("unhelpfulness", "help")));
        assert!(candidates.contains(&("decisions", "decision")));

        assert!(!candidates.iter().any(|(_, lexeme)| {
            matches!(*lexeme, "ally" | "probab" | "consid" | "cision")
        }));
    }

    #[test]
    fn morphology_ranges_remain_absolute_in_document() {
        let result = analyze(2, "A very unhelpfulness example");
        let pieces: Vec<_> = result
            .morph_spans
            .iter()
            .map(|span| {
                (
                    &"A very unhelpfulness example"[span.start..span.end],
                    span.class,
                )
            })
            .collect();

        assert!(pieces.contains(&("un", MorphClass::Prefix)));
        assert!(pieces.contains(&("help", MorphClass::Stem)));
        assert!(pieces.contains(&("ness", MorphClass::Suffix)));
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
