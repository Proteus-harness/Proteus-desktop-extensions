//! Reads of the session config: models, reasoning efforts and extra settings.

use std::collections::BTreeSet;

use proteus_extension::{Host, Value, ValueExt};

pub const DEFAULT: &str = "_default";
pub const GROUPED_PICKER: &str = "widgets.picker.v4";

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

pub struct SelectedModel {
    pub provider: Option<String>,
    pub name: String,
}

impl SelectedModel {
    pub fn matches(&self, provider: &str, model: &str) -> bool {
        self.provider.as_deref() == Some(provider) && self.name == model
    }
}

/// A pending qualified choice carries its own provider until Core acknowledges it.
pub fn selected_model(host: &Host) -> Option<SelectedModel> {
    let chosen = host.setting_value("model").unwrap_or(Value::Null);
    let confirmed = config(host).at("model");
    let name = chosen
        .opt_str("model")
        .or_else(|| chosen.as_str())
        .filter(|name| !name.is_empty())
        .or_else(|| confirmed.opt_str("name").filter(|name| !name.is_empty()))?;
    let provider = chosen
        .opt_str("provider")
        .or_else(|| confirmed.opt_str("provider"))
        .filter(|provider| !provider.is_empty())
        .map(str::to_owned);
    Some(SelectedModel {
        provider,
        name: name.to_owned(),
    })
}

pub fn model_name(host: &Host) -> Option<String> {
    selected_model(host).map(|model| model.name)
}

/// Keep the bare name accepted by older Core versions when the current
/// provider and its advertised control establish an unambiguous route.
pub fn changed_model_value(host: &Host, value: &Value) -> Option<Value> {
    let selected = selected_model(host);
    if let (Some(provider), Some(model)) = (value.opt_str("provider"), value.opt_str("model")) {
        if selected
            .as_ref()
            .is_some_and(|selected| selected.matches(provider, model))
        {
            return None;
        }
        // A queued qualified choice may change the provider before this
        // command runs, even when its target matches the confirmed provider.
        let same_provider = config(host).at("model").opt_str("provider") == Some(provider)
            && host
                .setting_value("model")
                .is_none_or(|value| !value.is_object());
        if same_provider
            && host.setting("model").is_some_and(|control| {
                control
                    .list("options")
                    .iter()
                    .any(|option| option.opt_str("value") == Some(model))
            })
        {
            return Some(model.into());
        }
        return Some(value.clone());
    }
    let name = value.as_str()?;
    (selected.as_ref().map(|model| model.name.as_str()) != Some(name)).then(|| value.clone())
}

/// Unique explicit efforts in provider order; the profile default is the reset button.
pub fn efforts(host: &Host) -> Vec<&Value> {
    if host.setting_value("model").is_some_and(|value| {
        value
            .opt_str("provider")
            .is_some_and(|provider| config(host).at("model").opt_str("provider") != Some(provider))
    }) {
        return Vec::new();
    }
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
        .collect()
}

/// The effort follows the profile instead of an explicit choice.
pub fn by_profile(host: &Host) -> bool {
    host.setting_value("reasoning_effort")
        .as_ref()
        .and_then(Value::as_str)
        == Some(DEFAULT)
}

pub fn effort_position(host: &Host, efforts: &[&Value]) -> usize {
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
    let Some(model) = selected_model(host) else {
        return &Value::Null;
    };
    config(host)
        .list("model_options")
        .iter()
        .find(|option| {
            option.opt_str("name") == Some(model.name.as_str())
                && model
                    .provider
                    .as_deref()
                    .is_none_or(|provider| option.opt_str("provider") == Some(provider))
        })
        .unwrap_or(&Value::Null)
}

/// Select settings beyond mode, model and reasoning.
pub fn extra(host: &Host) -> Vec<&Value> {
    host.session_settings()
        .into_iter()
        .filter(|setting| !matches!(setting.str("id"), "mode" | "model" | "reasoning_effort"))
        .collect()
}
