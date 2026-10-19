//! Local, deterministic relevance ranking. A query never leaves the Desk.

use crate::sessions::Session;

#[derive(Default)]
pub struct Index {
    entries: Vec<Entry>,
}

struct Entry {
    fields: Vec<Field>,
}

struct Field {
    text: String,
    words: Vec<String>,
    weight: u32,
    label: &'static str,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub item: usize,
    pub score: u32,
    pub label: &'static str,
    pub approximate: bool,
    pub snippet: Option<String>,
}

pub(super) struct Term {
    pub text: String,
    pub phrase: bool,
    pub exclude: bool,
}

impl Index {
    pub fn new(items: &[Session]) -> Self {
        Self {
            entries: items
                .iter()
                .map(|item| Entry {
                    fields: vec![
                        Field::new(&item.title, 100, "Title match"),
                        Field::new(
                            std::path::Path::new(&item.cwd)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or_default(),
                            75,
                            "Project match",
                        ),
                        Field::new(&item.cwd, 35, "Folder match"),
                        Field::new(item.app(), 30, "App match"),
                        Field::new(item.target.id(), 20, "Session ID match"),
                    ],
                })
                .collect(),
        }
    }

    pub fn search(&self, query: &str) -> Vec<Hit> {
        let terms = parse(query);
        if terms.is_empty() {
            return Vec::new();
        }
        let mut hits = Vec::new();
        for (item, entry) in self.entries.iter().enumerate() {
            let mut hit = Hit {
                item,
                score: 0,
                label: "Keyword match",
                approximate: false,
                snippet: None,
            };
            let mut strongest = 0;
            let matched = terms.iter().all(|term| {
                let best = entry
                    .fields
                    .iter()
                    .filter_map(|field| field.matches(term).map(|quality| (field, quality)))
                    .max_by_key(|(field, quality)| field.weight * quality);
                if term.exclude {
                    return best.is_none();
                }
                let Some((field, quality)) = best else {
                    return false;
                };
                let score = field.weight * quality;
                hit.score += score;
                hit.approximate |= quality == 1;
                if score > strongest {
                    strongest = score;
                    hit.label = field.label;
                }
                true
            });
            if matched {
                // Reward the whole title/phrase while preserving all-keyword matching across fields.
                let positive: Vec<_> = terms.iter().filter(|t| !t.exclude).collect();
                let phrase = positive
                    .iter()
                    .map(|t| t.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                if !phrase.is_empty() {
                    let title = &entry.fields[0].text;
                    if *title == phrase {
                        hit.score += 1_000;
                    } else if title.contains(&phrase) {
                        hit.score += 250;
                    }
                }
                hits.push(hit);
            }
        }
        // Discovery order breaks ties: currently running, open, then most recently updated.
        hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.item.cmp(&b.item)));
        hits
    }
}

impl Field {
    fn new(text: &str, weight: u32, label: &'static str) -> Self {
        let text = normalize(text);
        let words = text.split_whitespace().map(str::to_owned).collect();
        Self {
            text,
            words,
            weight,
            label,
        }
    }

    fn matches(&self, term: &Term) -> Option<u32> {
        if term.phrase {
            return format!(" {} ", self.text)
                .contains(&format!(" {} ", term.text))
                .then_some(4);
        }
        if self.words.iter().any(|word| *word == term.text) {
            return Some(4);
        }
        if self.words.iter().any(|word| word.starts_with(&term.text)) {
            return Some(3);
        }
        if self.text.contains(&term.text) {
            return Some(2);
        }
        // Exclusions are literal: a misspelling must not silently hide a different chat.
        if !term.exclude && self.words.iter().any(|word| near_word(&term.text, word)) {
            return Some(1);
        }
        None
    }
}

fn normalize(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn parse(query: &str) -> Vec<Term> {
    let mut terms = Vec::new();
    let bounded: String = query.chars().take(256).collect();
    let mut chars = bounded.chars().peekable();
    while let Some(c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let exclude = *c == '-';
        if exclude {
            chars.next();
        }
        let phrase = chars.peek() == Some(&'"');
        if phrase {
            chars.next();
        }
        let mut raw = String::new();
        for c in chars.by_ref() {
            if phrase && c == '"' || !phrase && c.is_whitespace() {
                break;
            }
            raw.push(c);
        }
        let text = normalize(&raw);
        if phrase {
            if !text.is_empty() {
                terms.push(Term {
                    text,
                    phrase,
                    exclude,
                });
            }
        } else {
            terms.extend(text.split_whitespace().map(|word| Term {
                text: word.to_owned(),
                phrase: false,
                exclude,
            }));
        }
        if terms.len() >= 16 {
            terms.truncate(16);
            break;
        }
    }
    terms
}

/// Bounded edit distance with adjacent transpositions, for words of at least four characters.
fn near_word(query: &str, candidate: &str) -> bool {
    let a: Vec<_> = query.chars().take(65).collect();
    let b: Vec<_> = candidate.chars().take(65).collect();
    let allowed = if a.len() >= 8 { 2 } else { 1 };
    if a.len() < 4 || a.len() > 64 || b.len() > 64 || a.len().abs_diff(b.len()) > allowed {
        return false;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut before_previous = previous.clone();
    for (i, left) in a.iter().enumerate() {
        let mut row = vec![i + 1; b.len() + 1];
        for (j, right) in b.iter().enumerate() {
            row[j + 1] = (row[j] + 1)
                .min(previous[j + 1] + 1)
                .min(previous[j] + usize::from(left != right));
            if i > 0 && j > 0 && *left == b[j - 1] && a[i - 1] == *right {
                row[j + 1] = row[j + 1].min(before_previous[j - 1] + 1);
            }
        }
        if row.iter().all(|distance| *distance > allowed) {
            return false;
        }
        before_previous = previous;
        previous = row;
    }
    previous[b.len()] <= allowed
}
