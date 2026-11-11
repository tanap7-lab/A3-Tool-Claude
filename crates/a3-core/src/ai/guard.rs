//! The Rust guardrails of section 9.6. These run on every model reply, whatever the model did.
//! G3 and G4 check citations. G5 checks that every number, unit, date, code and proper noun in an
//! output already exists in its input. Phrasing adds the rules of section 9.4.

use crate::domain::normalize;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{BTreeMap, HashSet};

/// G4: the quote must really be in the passage, ignoring spacing and case.
pub fn quote_in_text(quote: &str, text: &str) -> bool {
    let q = normalize(quote);
    !q.is_empty() && normalize(text).contains(&q)
}

static NUMBER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?:^|[^0-9A-Za-z.,])(\d+(?:[.,:/]\d+)*)(?:\s?(%|‰|°\s?[cf]|ppm|ppb|mm|cm|km|µm|um|nm|kg|mg|lb|lbs|oz|ml|kw|mw|kv|mv|ma|kn|nm|kpa|mpa|psi|hz|khz|mhz|rpm|db|pcs|ms|min|mins|hr|hrs|day|days|week|weeks|month|months|year|years|hour|hours|minute|minutes|second|seconds|m|g|l|s|h|t|v|a|w|n)\b|\s?%)?").unwrap()
});
/// Part numbers and other codes: letters and digits joined, like PN-4471, A3-2026-0042, X12B.
static CODE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(?:[A-Za-z]+[0-9][A-Za-z0-9]*(?:[-_/.][A-Za-z0-9]+)*|[A-Za-z]+(?:[-_/][A-Za-z0-9]+)*[-_/][0-9][A-Za-z0-9]*|[0-9]+[A-Za-z]+[A-Za-z0-9]*(?:[-_/.][A-Za-z0-9]+)*)\b").unwrap()
});
static ACRONYM: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b[A-Z]{2,}\b").unwrap());
static CAPITALISED: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b[A-Z][a-z][a-zA-Z'-]*\b").unwrap());
static WORD: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\p{L}\p{N}]+").unwrap());

/// Words the app's own card language capitalises, which are not claims about the world.
const ALLOWED_CAPITALISED: &[&str] = &["temporary", "permanent", "step", "steps", "early", "warning", "root", "cause"];

/// A number with thousands separators reads the same as one without.
fn canon_number(n: &str) -> String {
    let n = n.trim();
    static THOUSANDS: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\d{1,3}(,\d{3})+(\.\d+)?$").unwrap());
    if THOUSANDS.is_match(n) {
        n.replace(',', "")
    } else {
        n.to_string()
    }
}

fn canon_unit(u: &str) -> String {
    let u = u.to_lowercase().replace(' ', "");
    match u.as_str() {
        "mins" | "minute" | "minutes" => "min".into(),
        "hrs" | "hour" | "hours" | "h" => "hr".into(),
        "second" | "seconds" => "s".into(),
        "days" => "day".into(),
        "weeks" => "week".into(),
        "months" => "month".into(),
        "years" => "year".into(),
        "lbs" => "lb".into(),
        "um" => "µm".into(),
        _ => u,
    }
}

#[derive(Default)]
pub struct Tokens {
    numbers: HashSet<String>,
    numbers_with_unit: HashSet<(String, String)>,
    codes: HashSet<String>,
    words: HashSet<String>,
}

pub fn tokens(text: &str) -> Tokens {
    let mut t = Tokens::default();
    let mut measurements = Vec::new();
    for c in NUMBER.captures_iter(text) {
        let n = canon_number(&c[1]);
        if let Some(u) = c.get(2) {
            t.numbers_with_unit.insert((n.clone(), canon_unit(u.as_str())));
            measurements.push(c.get(1).expect("number capture").start()..u.end());
        }
        t.numbers.insert(n);
    }
    for m in CODE.find_iter(text) {
        if !measurements.iter().any(|range| range.start <= m.start() && m.end() <= range.end) {
            t.codes.insert(m.as_str().to_lowercase());
        }
    }
    for m in WORD.find_iter(text) {
        t.words.insert(m.as_str().to_lowercase());
    }
    t
}

/// Sentence starts are capitalised anyway, so only capitals later in a sentence count as names.
fn proper_nouns(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for m in CAPITALISED.find_iter(text) {
        let before = text[..m.start()].trim_end();
        let sentence_start = before.is_empty() || before.ends_with(['.', '!', '?', ':', ';', '\n', '-', '•', '(', '"']);
        if !sentence_start && !ALLOWED_CAPITALISED.contains(&m.as_str().to_lowercase().as_str()) {
            out.push(m.as_str().to_string());
        }
    }
    for m in ACRONYM.find_iter(text) {
        out.push(m.as_str().to_string());
    }
    out
}

/// G5: returns everything in `output` that the input does not contain. Empty means it passes.
pub fn unsupported_tokens(output: &str, input: &str) -> Vec<String> {
    let have = tokens(input);
    let got = tokens(output);
    let mut bad: Vec<String> = Vec::new();
    for n in &got.numbers {
        if !have.numbers.contains(n) {
            bad.push(n.clone());
        }
    }
    for pair in &got.numbers_with_unit {
        if !have.numbers_with_unit.contains(pair) && have.numbers.contains(&pair.0) && !have.words.contains(&pair.1) {
            bad.push(format!("{} {}", pair.0, pair.1));
        } else if have.numbers.contains(&pair.0)
            && !have.numbers_with_unit.contains(pair)
            && have.numbers_with_unit.iter().any(|(n, _)| *n == pair.0)
        {
            // The input gave this number a different unit: 5 cm has become 5 mm.
            bad.push(format!("{} {}", pair.0, pair.1));
        }
    }
    let norm_input = normalize(input);
    for c in &got.codes {
        if !have.codes.contains(c) && !norm_input.contains(&normalize(c)) {
            bad.push(c.clone());
        }
    }
    for name in proper_nouns(output) {
        if !have.words.contains(&name.to_lowercase()) {
            bad.push(name);
        }
    }
    bad.sort();
    bad.dedup();
    bad
}

/// Narrative quantities may use equivalent notation; original evidence is not rewritten here.
pub fn unsupported_story_tokens(output: &str, input: &str) -> Vec<String> {
    fn expand_thousands(text: &str) -> String {
        static SHORT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?P<prefix>^|[^0-9A-Za-z_./-])(?P<whole>\d+)(?:\.(?P<fraction>\d{1,3}))?[kK]\b").unwrap());
        SHORT.replace_all(text, |c: &regex::Captures<'_>| {
            let matched = c.get(0).expect("whole match");
            let mut suffix = text[matched.end()..].chars();
            if suffix.next().is_some_and(|c| ['-', '_', '/', '.'].contains(&c)) && suffix.next().is_some_and(|c| c.is_alphanumeric()) {
                return matched.as_str().to_string();
            }
            let fraction = c.name("fraction").map_or("", |m| m.as_str());
            let digits = format!("{}{}", &c["whole"], fraction);
            match digits.parse::<u128>().ok().and_then(|n| n.checked_mul(10_u128.pow(3 - fraction.len() as u32))) {
                Some(n) => format!("{}{n}", &c["prefix"]),
                None => matched.as_str().to_string(),
            }
        }).into_owned()
    }
    unsupported_tokens(&expand_thousands(output), &expand_thousands(input))
}

/// Words that state how sure a claim is. A rewrite cannot add, drop or swap these.
const CERTAINTY: &[&str] = &[
    "suspected", "suspect", "suspects", "possible", "possibly", "potential", "potentially", "probable", "probably", "likely", "unlikely",
    "may", "might", "could", "appears", "appear", "seems", "seem", "perhaps", "maybe", "unconfirmed", "tentative", "presumed", "alleged",
    "confirmed", "confirm", "confirms", "proven", "proved", "verified", "definitely", "certainly", "certain", "clearly", "will", "must", "always", "never",
];

fn certainty_words(text: &str) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for m in WORD.find_iter(text) {
        let w = m.as_str().to_lowercase();
        if CERTAINTY.contains(&w.as_str()) {
            *map.entry(w).or_insert(0) += 1;
        }
    }
    map
}

/// Section 9.4: every number, date, name and code stays, and certainty stays exactly as written.
/// Returns the reason a rewrite fails, or None if it passes.
pub fn phrasing_violation(original: &str, rewrite: &str) -> Option<String> {
    let added = unsupported_tokens(rewrite, original);
    if !added.is_empty() {
        return Some(format!("adds {}", added.join(", ")));
    }
    let dropped = unsupported_tokens(original, rewrite);
    if !dropped.is_empty() {
        return Some(format!("drops {}", dropped.join(", ")));
    }
    if certainty_words(original) != certainty_words(rewrite) {
        return Some("changes the certainty".to_string());
    }
    None
}

static CAUSE_LANGUAGE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(caused by|because|due to|root cause|as a result of|resulted from|stems? from|attributed to|traced to|the cause)\b").unwrap()
});

/// While step B is open, a warning draft may not state a cause (section 10).
pub fn states_a_cause(text: &str) -> bool {
    CAUSE_LANGUAGE.is_match(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INPUT: &str = "On 12 March 2026 line 4 at Plant Penang scrapped 3.5% of part PN-4471. The target is 1.0% and the gap is 2,500 pcs over 5 days.";

    #[test]
    fn quote_check_ignores_spacing_and_case() {
        assert!(quote_in_text("SCRAPPED  3.5%\nof part", INPUT));
        assert!(!quote_in_text("scrapped 4.5%", INPUT));
        assert!(!quote_in_text("   ", INPUT));
    }

    #[test]
    fn grounded_text_passes() {
        assert_eq!(unsupported_tokens("Line 4 scrapped 3.5% of PN-4471 against a 1.0% target.", INPUT), Vec::<String>::new());
    }

    #[test]
    fn injected_number_is_caught() {
        let bad = unsupported_tokens("Line 4 scrapped 7.2% of PN-4471.", INPUT);
        assert_eq!(bad, vec!["7.2".to_string()]);
    }

    #[test]
    fn numbers_inside_longer_numbers_do_not_count() {
        assert_eq!(unsupported_tokens("The gap was 500 pcs.", INPUT), vec!["500".to_string()]);
        assert_eq!(unsupported_tokens("Over 25 days.", "Over 250 days."), vec!["25".to_string()]);
    }

    #[test]
    fn thousands_separators_read_the_same() {
        assert!(unsupported_tokens("The gap is 2500 pcs.", INPUT).is_empty());
    }

    #[test]
    fn story_quantities_can_expand_thousands_without_changing_values_or_identifiers() {
        assert!(unsupported_story_tokens("Approximately 68,000 were delivered.", "~68k delivered").is_empty());
        assert!(unsupported_story_tokens("About 68,500 were delivered.", "About 68.5k were delivered.").is_empty());
        assert!(unsupported_story_tokens("Approximately 68,000.", "Approximately 68k.").is_empty());
        assert!(unsupported_story_tokens("About 0.09k were delivered.", "About 90 were delivered.").is_empty());
        assert!(!unsupported_story_tokens("Approximately 68,001 were delivered.", "~68k delivered").is_empty());
        assert!(!unsupported_story_tokens("Part PN-68000 failed.", "Part PN-68k failed.").is_empty());
        assert!(!unsupported_story_tokens("Part 68000-42 failed.", "Part 68k-42 failed.").is_empty());
        assert!(!unsupported_tokens("Approximately 68,000 were delivered.", "~68k delivered").is_empty(), "detailed evidence keeps its stricter wording rule");
    }

    #[test]
    fn measurement_spacing_does_not_create_spurious_part_codes() {
        assert_eq!(phrasing_violation("The gap is 0.09 mm.", "The gap is 0.09mm."), None);
        assert_eq!(phrasing_violation("The gap is 0.09mm.", "The gap is 0.09 mm."), None);
        assert_eq!(phrasing_violation("Over 5 days.", "Over 5days."), None);
        assert!(!unsupported_tokens("The gap is 0.09cm.", "The gap is 0.09 mm.").is_empty());
        assert!(!unsupported_tokens("The gap is 0.08mm.", "The gap is 0.09 mm.").is_empty());
    }

    #[test]
    fn changed_unit_is_caught() {
        assert!(!unsupported_tokens("The gap held over 5 hours.", INPUT).is_empty());
        assert!(unsupported_tokens("The gap held over 5 days.", INPUT).is_empty());
    }

    #[test]
    fn invented_code_and_name_are_caught() {
        assert!(unsupported_tokens("Part PN-9999 failed.", INPUT).contains(&"pn-9999".to_string()));
        assert_eq!(unsupported_tokens("It failed at the plant in Stuttgart.", INPUT), vec!["Stuttgart".to_string()]);
        assert_eq!(unsupported_tokens("It failed the ISO audit.", INPUT), vec!["ISO".to_string()]);
    }

    #[test]
    fn sentence_start_capitals_are_not_names() {
        assert!(unsupported_tokens("Scrapped parts rose. Line 4 stopped.", INPUT).is_empty());
    }

    #[test]
    fn phrasing_cannot_change_facts_or_certainty() {
        let o = "The suspected cause may be a worn die on line 4.";
        assert_eq!(phrasing_violation(o, "A worn die on line 4 may be the suspected cause."), None);
        assert!(phrasing_violation(o, "A worn die on line 4 may be the confirmed cause.").is_some());
        assert!(phrasing_violation(o, "The suspected cause will be a worn die on line 4.").is_some());
        assert!(phrasing_violation(o, "The suspected cause may be a worn die.").is_some(), "dropping 4 fails");
        assert!(phrasing_violation(o, "The suspected cause may be a worn die on line 5.").is_some());
    }

    #[test]
    fn cause_language_is_detected() {
        assert!(states_a_cause("Scrap rose because the die wore out."));
        assert!(states_a_cause("Root cause: worn die."));
        assert!(!states_a_cause("Scrap rose to 3.5% on line 4. Check your dies."));
    }
}
