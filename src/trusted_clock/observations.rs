use super::*;
use serde_json::value::RawValue;
use std::collections::BTreeMap;

type Fields = BTreeMap<String, Box<RawValue>>;

pub(super) fn validate(input: &str) -> Result<(), CoreError> {
    let fields: Fields = serde_json::from_str(input)?;
    let profile: Profile = serde_json::from_str(
        fields
            .get("compatibility")
            .ok_or_else(|| invalid("missing compatibility"))?
            .get(),
    )?;
    if fields.contains_key("trustedAnchorSeconds") && profile != Profile::Apple {
        return Err(invalid("trustedAnchorSeconds requires Apple compatibility"));
    }
    if fields.contains_key("trustedAnchorSeconds") && fields.contains_key("trustedAnchorMs") {
        return Err(invalid("anchor observations are mutually exclusive"));
    }
    if let Some(reading) = fields.get("reading") {
        validate_fields(reading, reading_fields(profile), profile)?;
    }
    if let Some(server) = fields.get("server").filter(|raw| raw.get() != "null") {
        validate_fields(server, server_fields(profile), profile)?;
    }
    Ok(())
}

fn validate_fields(raw: &RawValue, allowed: &[&str], profile: Profile) -> Result<(), CoreError> {
    let fields: Fields = serde_json::from_str(raw.get())?;
    for (key, value) in fields {
        if !allowed.contains(&key.as_str()) {
            return Err(invalid(&format!(
                "observation {key} is unsupported by this profile"
            )));
        }
        if value.get() == "null" {
            continue;
        }
        if matches!(
            key.as_str(),
            "monotonicMs" | "requestMonotonicMs" | "responseMonotonicMs"
        ) && profile != Profile::Pwa
        {
            let integer_token = value
                .get()
                .parse::<i64>()
                .map_err(|_| invalid("monotonic reading must use an integer JSON token"))?;
            integer(integer_token, 0)?;
        }
    }
    Ok(())
}

fn reading_fields(profile: Profile) -> &'static [&'static str] {
    match profile {
        Profile::Apple => &["wallSeconds", "uptimeSeconds"],
        Profile::Android => &["wallMs", "monotonicMs", "bootId"],
        Profile::Desktop | Profile::Pwa => &["wallMs", "monotonicMs"],
    }
}

fn server_fields(profile: Profile) -> &'static [&'static str] {
    match profile {
        Profile::Apple => &[
            "serverTimeMs",
            "serverHlcWallMs",
            "requestWallMs",
            "requestUptimeSeconds",
            "responseUptimeSeconds",
        ],
        Profile::Android | Profile::Desktop => &[
            "serverTimeMs",
            "serverHlcWallMs",
            "requestWallMs",
            "responseWallMs",
            "requestMonotonicMs",
            "responseMonotonicMs",
        ],
        Profile::Pwa => &[
            "serverTimeMs",
            "serverHlcWallMs",
            "requestWallMs",
            "responseWallMs",
            "requestSequence",
        ],
    }
}
