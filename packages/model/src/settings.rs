//! Reads of the session config: models, reasoning efforts and extra settings.

use std::collections::BTreeSet;

use proteus_extension::{Host, Value, ValueExt};

pub const DEFAULT: &str = "_default";

pub fn config(host: &Host) -> &Value {
    host.state
        .as_ref()
        .map(|state| state.at("config"))
        .unwrap_or(&Value::Null)
}

pub fn catalog_error(host: &Host) -> Option<&str> {
    config(host)
        .opt_str("model_catalog_error")
        .filter(|error| !error.trim().is_empty())
}

/// The chosen model, or the profile model.
pub fn model_name(host: &Host) -> Option<String> {
    let chosen = host
        .setting_value("model")
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|name| !name.is_empty());
    chosen.or_else(|| {
        config(host)
            .at("model")
            .opt_str("name")
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
    })
}

/// Unique explicit efforts in provider order; the profile default is the reset button.
pub fn efforts(host: &Host) -> Vec<Value> {
    let options = host
        .setting("reasoning_effort")
        .map(|control| control.list("options"))
        .unwrap_or(&[]);
    let mut seen = BTreeSet::new();
    options
        .iter()
        .filter(|option| {
            option
                .at("value")
                .as_str()
                .is_some_and(|value| value != DEFAULT && seen.insert(value))
        })
        .cloned()
        .collect()
}

/// The effort follows the profile instead of an explicit choice.
pub fn by_profile(host: &Host) -> bool {
    host.setting_value("reasoning_effort")
        .as_ref()
        .and_then(Value::as_str)
        == Some(DEFAULT)
}

pub fn effort_position(host: &Host, efforts: &[Value]) -> usize {
    let effort = host
        .setting_value("reasoning_effort")
        .unwrap_or(Value::Null);
    if let Some(position) = efforts
        .iter()
        .position(|option| *option.at("value") == effort)
    {
        return position;
    }
    let fallback = current_option(host)
        .opt_str("default_reasoning_effort")
        .filter(|fallback| !fallback.is_empty());
    fallback
        .and_then(|fallback| {
            let value = format!("effort:{fallback}");
            efforts
                .iter()
                .position(|option| option.at("value").as_str() == Some(value.as_str()))
        })
        .unwrap_or(0)
}

fn current_option(host: &Host) -> &Value {
    let name = model_name(host);
    let options = config(host).list("model_options");
    options
        .iter()
        .find(|option| option.opt_str("name") == name.as_deref())
        .unwrap_or(&Value::Null)
}

/// Select settings beyond mode, model and reasoning.
pub fn extra(host: &Host) -> Vec<Value> {
    host.session_settings()
        .into_iter()
        .filter(|setting| !matches!(setting.str("id"), "mode" | "model" | "reasoning_effort"))
        .cloned()
        .collect()
}
