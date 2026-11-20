//! The order in which task titles are listed.
//!
//! Titles compare as a sequence of tokens. A run of ASCII digits is one token
//! and compares by numeric value, so "Task 2" lists before "Task 10". Every
//! other character compares by its case-folded code points, so "apple" and
//! "Apple" list together. The fold is `to_lowercase(to_uppercase(c))` from the
//! pinned Rust toolchain: it uses no locale, and it is the same in every native
//! build and in WASM. Capital sharp s folds like "ß", which keeps the fold
//! stable: folding folded text changes nothing.
//!
//! Titles have equal tokens when they differ only in leading zeros or in
//! characters that fold to the same text, such as "A" and "a", "ß" and "ss", or
//! "ı" and "i". The raw bytes break that tie, so two titles compare equal only
//! when they are the same string. The result is a total order.

use std::char::{ToLowercase, ToUppercase};
use std::cmp::Ordering;
use std::iter::FlatMap;

type FoldedChars = FlatMap<ToUppercase, ToLowercase, fn(char) -> ToLowercase>;

/// A digit run ranks where the character `0` would, so digits keep their
/// usual place after spaces and most punctuation and before letters.
const DIGIT_RUN_RANK: u32 = '0' as u32;

/// One comparison unit of a title. Field order is comparison order.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TitleToken<'a> {
    /// The folded code point, or `DIGIT_RUN_RANK` for a digit run.
    rank: u32,
    /// Digit count without leading zeros; a longer number is larger. This
    /// compares runs of any length without parsing them into an integer.
    significant_digits: usize,
    /// The digits without leading zeros. Empty for text and for zero.
    digits: &'a str,
}

impl<'a> TitleToken<'a> {
    fn text(folded: char) -> Self {
        Self {
            rank: u32::from(folded),
            significant_digits: 0,
            digits: "",
        }
    }

    fn digit_run(run: &'a str) -> Self {
        let digits = run.trim_start_matches('0');
        Self {
            rank: DIGIT_RUN_RANK,
            significant_digits: digits.len(),
            digits,
        }
    }
}

struct TitleTokens<'a> {
    rest: &'a str,
    /// Folded code points of the last text character that are not yet emitted.
    /// One character can fold to several, for example "ß" to "ss".
    folded: Option<FoldedChars>,
}

impl<'a> TitleTokens<'a> {
    fn new(title: &'a str) -> Self {
        Self {
            rest: title,
            folded: None,
        }
    }

    fn take_digit_run(&mut self) -> TitleToken<'a> {
        let end = self
            .rest
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(self.rest.len());
        let (run, rest) = self.rest.split_at(end);
        self.rest = rest;
        TitleToken::digit_run(run)
    }
}

impl<'a> Iterator for TitleTokens<'a> {
    type Item = TitleToken<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(folded) = self.folded.as_mut().and_then(Iterator::next) {
                return Some(TitleToken::text(folded));
            }
            let character = self.rest.chars().next()?;
            if character.is_ascii_digit() {
                return Some(self.take_digit_run());
            }
            self.rest = &self.rest[character.len_utf8()..];
            self.folded = Some(fold_case(character));
        }
    }
}

/// "ẞ", capital sharp s. `to_uppercase` leaves it unchanged and `to_lowercase`
/// gives "ß", so on its own it would stop at "ß" while "ß" folds to "ss".
const CAPITAL_SHARP_S: char = '\u{1E9E}';

fn fold_case(character: char) -> FoldedChars {
    let character = if character == CAPITAL_SHARP_S {
        'ß'
    } else {
        character
    };
    character.to_uppercase().flat_map(char::to_lowercase)
}

/// Natural, case-insensitive order of two task titles, with the raw bytes as
/// the tiebreak.
pub(crate) fn compare_titles(left: &str, right: &str) -> Ordering {
    TitleTokens::new(left)
        .cmp(TitleTokens::new(right))
        .then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests;
