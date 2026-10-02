use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

pub(super) const SAFE: i64 = 9_007_199_254_740_991;
pub(super) const SKEW: i64 = 300_000;
pub(super) const UNCERTAINTY: i64 = 30_000;
pub(super) const DRIFT: i64 = 1_000;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq)]
pub(super) enum Profile {
    #[serde(rename = "appleTrustedClock")]
    Apple,
    #[serde(rename = "androidTrustedClock")]
    Android,
    #[serde(rename = "desktopTrustedClock")]
    Desktop,
    #[serde(rename = "pwaTrustedClock")]
    Pwa,
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) enum Action {
    Sample,
    Advance,
    Current,
    Restore,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Input {
    pub schema_version: u8,
    pub compatibility: Profile,
    pub action: Action,
    pub state: Box<RawValue>,
    #[serde(default)]
    pub reading: Reading,
    pub server: Option<Server>,
    pub trusted_anchor_ms: Option<i64>,
    #[serde(default, deserialize_with = "exact_optional_float")]
    pub trusted_anchor_seconds: Option<f64>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Reading {
    pub wall_ms: Option<i64>,
    // Apple Date and uptime are observed in seconds, before millisecond rounding.
    #[serde(default, deserialize_with = "exact_optional_float")]
    pub wall_seconds: Option<f64>,
    #[serde(default, deserialize_with = "exact_optional_float")]
    pub uptime_seconds: Option<f64>,
    #[serde(default, deserialize_with = "exact_optional_float")]
    pub monotonic_ms: Option<f64>,
    pub boot_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Server {
    pub server_time_ms: i64,
    pub server_hlc_wall_ms: Option<i64>,
    pub request_wall_ms: Option<i64>,
    pub response_wall_ms: Option<i64>,
    #[serde(default, deserialize_with = "exact_optional_float")]
    pub request_monotonic_ms: Option<f64>,
    #[serde(default, deserialize_with = "exact_optional_float")]
    pub response_monotonic_ms: Option<f64>,
    #[serde(default, deserialize_with = "exact_optional_float")]
    pub request_uptime_seconds: Option<f64>,
    #[serde(default, deserialize_with = "exact_optional_float")]
    pub response_uptime_seconds: Option<f64>,
    pub request_sequence: Option<i64>,
}

pub(super) fn exact_optional_float<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<Box<RawValue>>::deserialize(deserializer)?;
    raw.map(|raw| raw.get().parse::<f64>().map_err(serde::de::Error::custom))
        .transpose()
}

pub(super) fn exact_float<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    exact_optional_float(deserializer)?
        .ok_or_else(|| serde::de::Error::custom("missing fractional reading"))
}
