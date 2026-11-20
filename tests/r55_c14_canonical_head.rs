use pomodorough_core::dispatch_envelope_json;
use serde_json::Value;
use std::panic::catch_unwind;

// R55-C14: the head that hides never-sent operations came from
// `serverHlcWallMs`/`serverHlcCounter`, which also covers the server's
// physical clock. A Start pressed while a sync was in flight is older than the
// server's now, so the response hid it until the next sync acknowledged it.
// The response may now carry `canonicalHlcWallMs`/`canonicalHlcCounter`, the
// head over ordered operations only. Every fixture row is one complete public
// request with a never-sent Start at (T0 + 2500, 0) and `serverHlc*` at
// (T0 + 3000, 0): `user` rows carry an older canonical pair and project the
// Start, hidden before; `other` rows carry a pair not older than the Start and
// keep it hidden; `fallback` rows carry no pair; `error` rows break the
// receiver rule and fail closed; `clock` rows prove v1 clock rebasing stays on
// `serverHlc*`. The `-duration-after-acknowledged-barrier` rows put a
// never-sent duration change at the Start's clock behind an acknowledged
// duration row: the display before the response cannot admit it, so only the
// display after the response, with the canonical pair as its head, can.
// R55-C14-r1 adds the boundary rows (2^53 - 1 accepted, 2^53 refused, float
// and `-0` spellings refused), rows whose head counter decides at the Start's
// wall under a server counter of 5, a Start-then-Pause row that pins the
// stored command order of the display before the response, and `server` rows:
// with the pair present, an invalid server HLC is blamed on the server HLC.

const START: &str = "01a0fc7f-0a01-7b1e-99b1-cfb471edbf8c";
const FRESH_DURATION: &str = "01a0fc7f-0d02-7b1e-99b1-cfb471edbf8e";
const PAUSE: &str = "01a0fc7f-0a02-7b1e-99b1-cfb471edbf90";
const SERVER_HLC: (i64, i64) = (1_790_942_580_005, 0);
const SERVER_HLC_ERROR: &str = "invalid shared-core input: invalid canonical response server HLC";

fn fixture_rows() -> Vec<Value> {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/r55-c14-canonical-head-v1.json")).unwrap();
    fixture["rows"].as_array().unwrap().clone()
}

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key].as_str().unwrap()
}

fn json(row: &Value, key: &str) -> Value {
    serde_json::from_str(text(row, key)).unwrap()
}

/// The live envelope for the row's request, parsed.
fn dispatch(row: &Value) -> Value {
    let envelope = dispatch_envelope_json(text(row, "operation"), text(row, "input"));
    serde_json::from_str(&envelope).unwrap()
}

fn kind(row: &Value) -> &str {
    text(row, "kind")
}

fn has_pair(row: &Value) -> bool {
    let response = &json(row, "input")["response"];
    response.get("canonicalHlcWallMs").is_some() || response.get("canonicalHlcCounter").is_some()
}

fn hlc(value: &Value, wall_ms: &str, counter: &str) -> (i64, i64) {
    (
        value[wall_ms].as_i64().unwrap(),
        value[counter].as_i64().unwrap(),
    )
}

/// The clock of the never-sent Start, or of the never-sent duration change.
fn never_sent_hlc(input: &Value) -> (i64, i64) {
    let local = &input["local"];
    let operation = [("commands", START), ("durationOperations", FRESH_DURATION)]
        .iter()
        .find_map(|(queue, id)| {
            local[*queue]
                .as_array()
                .and_then(|operations| operations.iter().find(|op| op["id"] == *id))
        })
        .unwrap();
    hlc(operation, "hlcWallMs", "hlcCounter")
}

/// Whether the envelope projects the never-sent Start or duration change, on
/// screen and, for the v3 display path, in the stored display context.
fn projects_start(envelope: &Value) -> bool {
    let value = &envelope["value"];
    let listed = |queues: &Value| {
        [("commands", START), ("durationOperations", FRESH_DURATION)]
            .iter()
            .any(|(queue, id)| {
                queues[*queue]
                    .as_array()
                    .is_some_and(|operations| operations.iter().any(|op| op["id"] == *id))
            })
    };
    let projected = listed(&value["projectionPending"]);
    if let Some(context) = value.get("displayContext") {
        assert_eq!(listed(&context["projectionPending"]), projected);
    }
    projected
}

#[test]
fn every_row_returns_its_exact_envelope_without_unwinding() {
    for row in fixture_rows() {
        let (name, operation, input) = (
            text(&row, "name"),
            text(&row, "operation"),
            text(&row, "input"),
        );
        let envelope = catch_unwind(|| dispatch_envelope_json(operation, input))
            .unwrap_or_else(|_| panic!("{name} unwound"));
        assert_eq!(envelope, text(&row, "expected"), "{name}");
    }
}

#[test]
fn an_older_canonical_pair_projects_the_in_flight_start() {
    for row in fixture_rows().iter().filter(|row| kind(row) == "user") {
        let name = text(row, "name");
        let input = json(row, "input");
        let response = &input["response"];
        let head = hlc(response, "canonicalHlcWallMs", "canonicalHlcCounter");
        let server = hlc(response, "serverHlcWallMs", "serverHlcCounter");
        let start = never_sent_hlc(&input);
        assert!(head < start && start < server, "{name}");
        assert!(
            !projects_start(&json(row, "before")),
            "{name} hid nothing before"
        );
        assert!(projects_start(&dispatch(row)), "{name}");
    }
}

#[test]
fn a_canonical_pair_not_older_than_the_start_keeps_it_hidden() {
    for row in fixture_rows() {
        let name = text(&row, "name");
        match kind(&row) {
            "other" => assert!(has_pair(&row), "{name}"),
            "fallback" => assert!(!has_pair(&row), "{name}"),
            _ => continue,
        }
        assert_eq!(text(&row, "expected"), text(&row, "before"), "{name}");
        assert!(!projects_start(&dispatch(&row)), "{name}");
    }
}

#[test]
fn a_broken_receiver_rule_fails_closed() {
    for row in fixture_rows().iter().filter(|row| kind(row) == "error") {
        let name = text(row, "name");
        assert_eq!(
            json(row, "before")["ok"],
            true,
            "{name} was accepted before"
        );
        assert_eq!(
            dispatch(row),
            serde_json::json!({"ok": false, "error": text(row, "message")}),
            "{name}"
        );
    }
}

#[test]
fn an_invalid_server_hlc_is_blamed_with_the_pair_present() {
    for row in fixture_rows().iter().filter(|row| kind(row) == "server") {
        let name = text(row, "name");
        assert!(has_pair(row), "{name}");
        assert_eq!(text(row, "message"), SERVER_HLC_ERROR, "{name}");
        assert_eq!(
            dispatch(row),
            serde_json::json!({"ok": false, "error": SERVER_HLC_ERROR}),
            "{name}"
        );
    }
}

#[test]
fn both_display_passes_keep_the_local_command_order() {
    let rows = fixture_rows();
    let row = rows
        .iter()
        .find(|row| text(row, "name") == "v3-terminal-user-start-then-pause-order")
        .unwrap();
    let ids = |envelope: &Value| -> Vec<String> {
        envelope["value"]["displayContext"]["projectionPending"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|op| op["id"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(ids(&json(row, "before")), [PAUSE]);
    assert_eq!(ids(&dispatch(row)), [START, PAUSE]);
}

#[test]
fn clock_rebasing_ignores_the_canonical_pair() {
    let rows = fixture_rows();
    let clock: Vec<_> = rows.iter().filter(|row| kind(row) == "clock").collect();
    let [with_pair, without_pair] = clock.as_slice() else {
        panic!("two clock rows");
    };
    assert!(has_pair(with_pair) && !has_pair(without_pair));
    assert_eq!(dispatch(with_pair), dispatch(without_pair));
    assert_eq!(text(with_pair, "expected"), text(with_pair, "before"));
    let pending = &dispatch(with_pair)["value"]["pending"][0];
    let rebased = (
        pending["hlcWallMs"].as_i64().unwrap(),
        pending["hlcCounter"].as_i64().unwrap(),
    );
    assert!(rebased > SERVER_HLC, "v1 rebases above serverHlc*");
}

#[test]
fn case_matrix_counts_every_kind_and_path() {
    let rows = fixture_rows();
    let count = |kind_name: &str| rows.iter().filter(|row| kind(row) == kind_name).count();
    let kinds = ["user", "other", "fallback", "error", "server", "clock"];
    assert_eq!(
        kinds.map(count),
        [14, 16, 4, 60, 16, 2],
        "R55-C14 case matrix"
    );
    for label in ["v2-", "v3-delivery-", "v3-terminal-"] {
        for kind_name in ["user", "other", "fallback", "error", "server"] {
            assert!(
                rows.iter()
                    .any(|row| kind(row) == kind_name && text(row, "name").starts_with(label)),
                "{label}{kind_name}"
            );
        }
    }
    let terminal = |row: &&Value| json(row, "input").get("displayContext").is_some();
    assert!(
        rows.iter()
            .filter(terminal)
            .all(|row| text(row, "name").starts_with("v3-terminal-"))
    );
}
