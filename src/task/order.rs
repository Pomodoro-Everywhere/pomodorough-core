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
//!
//! A sort builds one `TitleOrderKey` per title and compares the keys. Comparing
//! tokens directly folds both titles again at every comparison, which made a
//! sort of hundreds of titles several times slower than the bytewise sort
//! before it (R55-C15).

use std::char::{ToLowercase, ToUppercase};
use std::iter::FlatMap;

type FoldedChars = FlatMap<ToUppercase, ToLowercase, fn(char) -> ToLowercase>;

/// A digit run ranks where the character `0` would, so digits keep their
/// usual place after spaces and most punctuation and before letters. No text
/// token is `0`: ASCII digits always form a digit run, and no other character
/// folds to an ASCII digit.
const DIGIT_RUN_RANK: char = '0';

/// One comparison unit of a title. Field order is comparison order.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TitleToken<'a> {
    /// The folded character, or `DIGIT_RUN_RANK` for a digit run.
    rank: char,
    /// Digit count without leading zeros; a longer number is larger. This
    /// compares runs of any length without parsing them into an integer.
    significant_digits: usize,
    /// The digits without leading zeros. Empty for text and for zero.
    digits: &'a str,
}

impl<'a> TitleToken<'a> {
    fn text(folded: char) -> Self {
        Self {
            rank: folded,
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

    /// Appends bytes whose bytewise order is this token's order, and which
    /// never form a prefix of another token's bytes. Text is its UTF-8, which
    /// sorts by code point. A digit run is `0`, then its significant digit
    /// count as eight big-endian bytes, then those digits; equal counts mean
    /// equal lengths, so the digits compare like the numbers.
    fn encode(&self, key: &mut Vec<u8>) {
        if self.rank != DIGIT_RUN_RANK {
            let mut utf8 = [0; 4];
            key.extend_from_slice(self.rank.encode_utf8(&mut utf8).as_bytes());
            return;
        }
        key.push(b'0');
        key.extend_from_slice(&(self.significant_digits as u64).to_be_bytes());
        key.extend_from_slice(self.digits.as_bytes());
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
            if character.is_ascii() {
                // The fold of an ASCII character is its ASCII lowercase.
                return Some(TitleToken::text(character.to_ascii_lowercase()));
            }
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

/// A title's tokens as bytes whose bytewise order is the token order: a title
/// whose tokens are a prefix of another's lists first. Titles with equal keys
/// differ only in leading zeros or case; the raw title must break that tie.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TitleOrderKey(Vec<u8>);

impl TitleOrderKey {
    pub(crate) fn new(title: &str) -> Self {
        // A title of ASCII text without digits takes one byte per byte.
        let mut key = Vec::with_capacity(title.len());
        if title.is_ascii() {
            encode_ascii_title(title, &mut key);
        } else {
            for token in TitleTokens::new(title) {
                token.encode(&mut key);
            }
        }
        Self(key)
    }
}

/// The encoding of `TitleTokens` for an ASCII title, without decoding chars:
/// most titles are ASCII, and every client sorts its task list on each replay.
fn encode_ascii_title(title: &str, key: &mut Vec<u8>) {
    let bytes = title.as_bytes();
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if !byte.is_ascii_digit() {
            // The fold of an ASCII character is its ASCII lowercase.
            key.push(byte.to_ascii_lowercase());
            index += 1;
            continue;
        }
        let run_length = bytes[index..]
            .iter()
            .position(|byte| !byte.is_ascii_digit())
            .unwrap_or(bytes.len() - index);
        TitleToken::digit_run(&title[index..index + run_length]).encode(key);
        index += run_length;
    }
}

/// Natural, case-insensitive order of two task titles, with the raw bytes as
/// the tiebreak.
#[cfg(test)]
pub(crate) fn compare_titles(left: &str, right: &str) -> std::cmp::Ordering {
    TitleOrderKey::new(left)
        .cmp(&TitleOrderKey::new(right))
        .then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests;
