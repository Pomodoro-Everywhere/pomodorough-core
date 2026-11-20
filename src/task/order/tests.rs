use super::{compare_titles, fold_case};
use std::cmp::Ordering;

fn listed(titles: &[&str]) -> Vec<String> {
    let mut titles = titles
        .iter()
        .map(|title| (*title).to_owned())
        .collect::<Vec<_>>();
    titles.sort_by(|left, right| compare_titles(left, right));
    titles
}

/// The expected list, given in order, must come back from its reverse and
/// from a rotation. The comparator ends on the raw bytes, so the order of the
/// input cannot matter.
fn assert_listed(expected: &[&str]) {
    let reversed = expected.iter().rev().copied().collect::<Vec<_>>();
    assert_eq!(listed(&reversed), expected, "from reversed input");
    let mut rotated = expected.to_vec();
    rotated.rotate_left(expected.len() / 2);
    assert_eq!(listed(&rotated), expected, "from rotated input");
}

#[test]
fn bulk_tasks_list_by_number() {
    let titles = (1..=12)
        .map(|number| format!("Bulk task {number}"))
        .collect::<Vec<_>>();
    let expected = titles.iter().map(String::as_str).collect::<Vec<_>>();
    assert_listed(&expected);
    assert_eq!(
        compare_titles("Bulk task 2", "Bulk task 10"),
        Ordering::Less
    );
}

#[test]
fn numbers_inside_a_title_compare_by_value() {
    assert_listed(&["v2 draft", "v2 final", "v10 draft", "v10.2", "v10.10"]);
}

#[test]
fn leading_zeros_do_not_change_the_number() {
    // "007" and "7" are the same number; the raw bytes then put "007" first.
    assert_listed(&[
        "Task 0", "Task 00", "Task 1", "Task 007", "Task 7", "Task 08",
    ]);
    assert_eq!(compare_titles("Task 007 b", "Task 7 a"), Ordering::Greater);
}

#[test]
fn digit_runs_longer_than_u64_compare_without_overflow() {
    let u64_max = u64::MAX.to_string();
    let above_u64 = format!("{u64_max}0");
    let nines = "9".repeat(40);
    let one_and_zeros = format!("1{}", "0".repeat(40));
    let padded_two = format!("{}2", "0".repeat(60));
    assert_listed(&[
        &format!("Run {padded_two}"),
        &format!("Run {u64_max}"),
        &format!("Run {above_u64}"),
        &format!("Run {nines}"),
        &format!("Run {one_and_zeros}"),
    ]);
}

#[test]
fn case_does_not_separate_titles() {
    assert_listed(&["Apple", "apple", "Banana", "banana", "cherry", "Zebra"]);
    assert_eq!(compare_titles("apple", "Zebra"), Ordering::Less);
}

#[test]
fn non_ascii_titles_fold_case_without_locale() {
    assert_listed(&["Арбуз", "Яблоко", "яблоко", "яблоко 2", "ЯБЛОКО 10"]);
    assert_listed(&["Zebra", "École 2", "école 10", "日本語 2", "日本語 10"]);
    // Final sigma and sharp s fold to the same text as their capitals.
    assert_listed(&["ΟΔΟΣ 2", "οδος 3", "οδοσ 10"]);
    assert_listed(&["STRASSE 2", "Straße 3", "strasse 10"]);
}

#[test]
fn digits_keep_their_place_between_punctuation_and_letters() {
    assert_listed(&[
        "", "Task", "Task 2", "Task 10", "Task A", "Task-1", "Task9", "Task10", "TaskA", "Taska",
    ]);
}

#[test]
fn letters_list_after_the_punctuation_between_capitals_and_small_letters() {
    // "[", "\", "]", "^", "_" and "`" lie between "Z" and "a". Letters rank as
    // small letters, so all six list before every letter and "{" lists after.
    assert_listed(&[
        "Task10", "Task[", "Task\\", "Task]", "Task^", "Task_1", "Task`", "TaskA", "Taska",
        "TaskZ", "Taskz", "Task{",
    ]);
    assert_eq!(compare_titles("Task_1", "TaskA"), Ordering::Less);
}

#[test]
fn digit_runs_list_before_the_punctuation_after_nine() {
    // ":", ";", "<", "=", ">", "?" and "@" follow "9". A digit run of any
    // value ranks at "0", so it lists before all seven and after "/".
    assert_listed(&[
        "Task/", "Task0", "Task1", "Task9", "Task10", "Task:", "Task;", "Task<", "Task=", "Task>",
        "Task?", "Task@", "TaskA",
    ]);
    assert_eq!(compare_titles("a1", "a:"), Ordering::Less);
}

#[test]
fn capital_sharp_s_lists_with_sharp_s() {
    // "ẞ" (U+1E9E) folds to "ss" like "ß"; the raw bytes order the spellings.
    assert_listed(&["SS", "ss", "ß", "ẞ"]);
    assert_listed(&["STRASSE 2", "STRAẞE 3", "Straße 4", "strasse 10", "Strast"]);
    assert_eq!(compare_titles("ẞ", "st"), Ordering::Less);
}

#[test]
fn folding_folded_text_changes_nothing_for_every_character() {
    for character in char::MIN..=char::MAX {
        let folded = fold_case(character).collect::<String>();
        let refolded = folded.chars().flat_map(fold_case).collect::<String>();
        assert_eq!(folded, refolded, "U+{:04X}", u32::from(character));
    }
}

#[test]
fn dotted_and_dotless_i_fold_without_a_turkish_rule() {
    // "ı" folds to "i", so it lists with "I" and "i" and the numbers decide.
    assert_listed(&["I 2", "ı 3", "i 10"]);
    // "İ" folds to "i" and a combining dot (U+0307). It lists with its
    // decomposed spelling, after every "i" that an ASCII letter follows.
    assert_listed(&["i", "iz", "i\u{307} 2", "İ 3", "i\u{307} 10", "j"]);
    // The raw bytes order titles with equal tokens.
    assert_listed(&["I", "i", "ı", "i\u{307}", "İ"]);
}

#[test]
fn only_ascii_digits_are_numbers() {
    // Fullwidth digits are text: they compare by code point, one at a time.
    assert_eq!(compare_titles("１０", "２"), Ordering::Less);
    assert_eq!(compare_titles("10", "2"), Ordering::Greater);
}

#[test]
fn the_empty_title_lists_first_and_equals_itself() {
    assert_eq!(compare_titles("", ""), Ordering::Equal);
    assert_listed(&["", " ", "0", "2", "10", "a", "B"]);
}

#[test]
fn equal_only_for_the_same_string() {
    assert_eq!(compare_titles("Task 7", "Task 7"), Ordering::Equal);
    assert_ne!(compare_titles("Task 7", "task 7"), Ordering::Equal);
    assert_ne!(compare_titles("Task 7", "Task 07"), Ordering::Equal);
}

const FRAGMENTS: &[&str] = &[
    "", "a", "A", "b", "B", "z", "0", "1", "2", "9", "00", "10", "007", " ", "-", ".", "ß", "ss",
    "SS", "é", "É", "σ", "ς", "Σ", "ı", "İ", "я", "Я", "日", "２",
];

/// Deterministic xorshift64 generator; the corpus is the same on every run.
fn next_random(state: &mut u64) -> usize {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state % FRAGMENTS.len() as u64) as usize
}

fn generated_titles(count: usize) -> Vec<String> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    (0..count)
        .map(|_| {
            let fragments = next_random(&mut state) % 5;
            (0..fragments)
                .map(|_| FRAGMENTS[next_random(&mut state)])
                .collect::<String>()
        })
        .collect()
}

#[test]
fn generated_titles_have_a_total_order() {
    let titles = generated_titles(120);
    for left in &titles {
        assert_eq!(compare_titles(left, left), Ordering::Equal, "{left:?}");
        for right in &titles {
            let forward = compare_titles(left, right);
            assert_eq!(forward, compare_titles(right, left).reverse());
            assert_eq!(forward == Ordering::Equal, left == right);
            assert_transitive(left, right, forward, &titles);
        }
    }
}

fn assert_transitive(left: &str, middle: &str, forward: Ordering, titles: &[String]) {
    if forward == Ordering::Greater {
        return;
    }
    for right in titles {
        if compare_titles(middle, right) != Ordering::Greater {
            assert_ne!(
                compare_titles(left, right),
                Ordering::Greater,
                "{left:?} <= {middle:?} <= {right:?}"
            );
        }
    }
}

#[test]
fn generated_titles_list_the_same_from_any_input_order() {
    let mut titles = generated_titles(200);
    let mut forward = titles.clone();
    forward.sort_by(|left, right| compare_titles(left, right));
    titles.reverse();
    titles.sort_by(|left, right| compare_titles(left, right));
    forward.dedup();
    titles.dedup();
    assert_eq!(forward, titles);
}

/// A slow reference for the same rules that parses every number into an
/// integer. Generated digit runs are short enough for `u128`.
fn reference_key(title: &str) -> Vec<(u32, u128)> {
    let mut key = Vec::new();
    let mut characters = title.chars().peekable();
    while let Some(character) = characters.next() {
        if !character.is_ascii_digit() {
            let folded = character.to_uppercase().flat_map(char::to_lowercase);
            key.extend(folded.map(|folded| (u32::from(folded), 0)));
            continue;
        }
        let mut number = u128::from(character.to_digit(10).unwrap());
        while let Some(digit) = characters.next_if(char::is_ascii_digit) {
            number = number * 10 + u128::from(digit.to_digit(10).unwrap());
        }
        key.push((u32::from('0'), number));
    }
    key
}

#[test]
fn generated_titles_agree_with_a_reference_that_parses_numbers() {
    let titles = generated_titles(200);
    for left in &titles {
        for right in &titles {
            let expected = reference_key(left)
                .cmp(&reference_key(right))
                .then_with(|| left.cmp(right));
            assert_eq!(compare_titles(left, right), expected, "{left:?} {right:?}");
        }
    }
}
