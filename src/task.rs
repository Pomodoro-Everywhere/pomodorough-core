use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_general_category::{GeneralCategory, get_general_category};
use unicode_normalization::UnicodeNormalization;

use crate::CoreError;

const TASK_NAMESPACE: &[u8] = b"pomodorough.task.v1\0";
const MAX_TASK_TITLE_BYTES: usize = 512;

#[derive(Deserialize)]
struct TaskIdentityInput {
    title: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskIdentityOutput {
    id: String,
    title: String,
    utf8_bytes: usize,
}

pub(crate) fn identity_json(input: &str) -> Result<String, CoreError> {
    let input: TaskIdentityInput = serde_json::from_str(input)?;
    let title = normalize_title(&input.title);
    let utf8_bytes = title.len();
    if title.is_empty() {
        return Err(CoreError::InvalidInput(
            "task title is empty after normalization".into(),
        ));
    }
    if utf8_bytes > MAX_TASK_TITLE_BYTES {
        return Err(CoreError::InvalidInput(format!(
            "task title exceeds {MAX_TASK_TITLE_BYTES} UTF-8 bytes"
        )));
    }
    let id = task_id(&title);
    Ok(serde_json::to_string(&TaskIdentityOutput {
        id,
        title,
        utf8_bytes,
    })?)
}

fn normalize_title(value: &str) -> String {
    let normalized = value.nfc().collect::<String>();
    let mut start = 0;
    let mut end = normalized.len();
    for (index, character) in normalized.char_indices() {
        if is_printable(character) {
            start = index;
            break;
        }
        start = index + character.len_utf8();
    }
    for (index, character) in normalized.char_indices().rev() {
        if is_printable(character) {
            end = index + character.len_utf8();
            break;
        }
        end = index;
    }
    if start >= end {
        String::new()
    } else {
        normalized[start..end].to_owned()
    }
}

fn is_printable(value: char) -> bool {
    if value == ' ' {
        return true;
    }
    matches!(
        get_general_category(value),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
            | GeneralCategory::NonspacingMark
            | GeneralCategory::SpacingMark
            | GeneralCategory::EnclosingMark
            | GeneralCategory::DecimalNumber
            | GeneralCategory::LetterNumber
            | GeneralCategory::OtherNumber
            | GeneralCategory::ConnectorPunctuation
            | GeneralCategory::DashPunctuation
            | GeneralCategory::OpenPunctuation
            | GeneralCategory::ClosePunctuation
            | GeneralCategory::InitialPunctuation
            | GeneralCategory::FinalPunctuation
            | GeneralCategory::OtherPunctuation
            | GeneralCategory::MathSymbol
            | GeneralCategory::CurrencySymbol
            | GeneralCategory::ModifierSymbol
            | GeneralCategory::OtherSymbol
    )
}

fn task_id(normalized_title: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(TASK_NAMESPACE);
    digest.update(normalized_title.as_bytes());
    let mut bytes: [u8; 16] = digest.finalize()[..16]
        .try_into()
        .expect("SHA-256 prefix is exactly 16 bytes");
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}
