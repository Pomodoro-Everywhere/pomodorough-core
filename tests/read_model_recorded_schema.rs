//! Every read-model return recorded under `fixtures/` carries schema version 2
//! and the elapsed-time keys. Two captures of older builds are frozen and keep
//! version 1; `PWA_NATURAL_COMPLETION.md` names the build behind each. This
//! guard lists them exactly, so a new occurrence of the old shape fails here.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const OLD_CADENCE_KEY: &str = "completedFocusTodayPlannedDurationMs";
const NEW_CADENCE_KEY: &str = "completedFocusTodayElapsedMs";

/// `#` marks a JSON document stored as a string, such as a raw envelope.
const FROZEN_VERSION_1_RETURNS: [&str; 3] = [
    "pwa-natural-checker-v1.json/cases/6/priorEnvelopeRaw#/value",
    "pwa-natural-checker-v1.json/cases/6/priorReturn/value",
    "pwa-natural-completion-v1.json/installation/model",
];

/// Raw text occurrences of the old cadence key, per fixture file.
const FROZEN_OLD_KEY_TEXT: [(&str, usize); 2] = [
    ("pwa-natural-checker-v1.json", 2),
    ("pwa-natural-completion-v1.json", 1),
];

#[derive(Default)]
struct RecordedReturns {
    current: usize,
    version_1: BTreeSet<String>,
}

fn is_read_model(node: &Value) -> bool {
    ["schemaVersion", "canonical", "cadence", "availableIntents"]
        .iter()
        .all(|key| node.get(key).is_some())
}

fn has_current_shape(model: &Value) -> bool {
    let cadence = &model["cadence"];
    let tasks = model["tasks"]["completedFocusTodayByTask"].as_object();
    model["schemaVersion"] == 2
        && cadence.get(OLD_CADENCE_KEY).is_none()
        && cadence.get(NEW_CADENCE_KEY).is_some()
        && tasks.is_none_or(|tasks| {
            tasks.values().all(|task| {
                task.get("plannedDurationMs").is_none() && task.get("elapsedMs").is_some()
            })
        })
}

fn has_frozen_shape(model: &Value) -> bool {
    model["schemaVersion"] == 1
        && model["cadence"].get(OLD_CADENCE_KEY).is_some()
        && model["cadence"].get(NEW_CADENCE_KEY).is_none()
}

fn scan(node: &Value, path: &str, found: &mut RecordedReturns) {
    match node {
        Value::Object(fields) => {
            if is_read_model(node) && has_current_shape(node) {
                found.current += 1;
            } else if is_read_model(node) {
                assert!(has_frozen_shape(node), "{path} mixes both schema versions");
                found.version_1.insert(path.to_owned());
            }
            for (key, value) in fields {
                scan(value, &format!("{path}/{key}"), found);
            }
        }
        Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                scan(value, &format!("{path}/{index}"), found);
            }
        }
        Value::String(text) if text.starts_with(['{', '[']) => {
            if let Ok(embedded) = serde_json::from_str::<Value>(text) {
                scan(&embedded, &format!("{path}#"), found);
            }
        }
        _ => {}
    }
}

fn fixture_files() -> Vec<(String, String)> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut files: Vec<(String, String)> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| {
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            (name, fs::read_to_string(&path).unwrap())
        })
        .collect();
    files.sort();
    files
}

fn recorded_returns(files: &[(String, String)]) -> RecordedReturns {
    let mut found = RecordedReturns::default();
    for (name, text) in files {
        let document: Value = serde_json::from_str(text).unwrap();
        scan(&document, name, &mut found);
    }
    found
}

#[test]
fn only_the_listed_frozen_captures_keep_read_model_schema_version_1() {
    let found = recorded_returns(&fixture_files());
    let allowed: BTreeSet<String> = FROZEN_VERSION_1_RETURNS.map(str::to_owned).into();
    assert_eq!(found.version_1, allowed);
    assert_eq!(found.current, 26, "recorded schema version 2 returns");
}

#[test]
fn only_the_listed_frozen_captures_spell_the_old_cadence_key() {
    let spelled: Vec<(String, usize)> = fixture_files()
        .into_iter()
        .map(|(name, text)| (name, text.matches(OLD_CADENCE_KEY).count()))
        .filter(|(_, count)| *count > 0)
        .collect();
    let allowed: Vec<(String, usize)> = FROZEN_OLD_KEY_TEXT
        .iter()
        .map(|(name, count)| ((*name).to_owned(), *count))
        .collect();
    assert_eq!(spelled, allowed);
}

#[test]
fn the_guard_reports_a_version_1_return_outside_the_list() {
    let mut files = fixture_files();
    let stray = files
        .iter()
        .find(|(name, _)| name == "pwa-natural-completion-v1.json")
        .map(|(_, text)| text.clone())
        .unwrap();
    files.push(("stray.json".to_owned(), stray));
    let found = recorded_returns(&files);
    assert!(found.version_1.contains("stray.json/installation/model"));
    assert_eq!(found.version_1.len(), FROZEN_VERSION_1_RETURNS.len() + 1);
}
