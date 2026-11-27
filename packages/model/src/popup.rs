//! The popup: model, reasoning stepper and extra session settings.
//! Адаптировано из Zed: crates/agent_ui/src/language_model_selector.rs.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use proteus_extension::ui::{
    Node, button, disclosure, divider, muted, note, picker_from, row, select, stack, text,
};
use proteus_extension::{Host, Value, ValueExt, json};

use crate::settings::{self, GROUPED_PICKER, SelectedModel};

pub fn render(host: &Host, models_open: bool) -> Value {
    let selected = settings::selected_model(host);
    let name = selected
        .as_ref()
        .map_or("Model", |model| model.name.as_str());
    let grouped = has_groups(host, selected.as_ref());
    let error = settings::catalog_error(host);
    if let Some(error) = error
        && !grouped
    {
        return stack(vec![
            text(name).with("single_line", true),
            text("Каталог моделей недоступен").with("tone", "warning"),
            error_details("Подробности", error),
        ])
        .with("transition", "catalog-error");
    }
    let model = host.setting("model");
    let pending = host.pending_setting("model").is_some();
    let efforts = settings::efforts(host);
    let extra = settings::extra(host);
    let stepper = efforts.len() >= 2;
    let has_summary = pending || stepper || !extra.is_empty();
    if models_open || !has_summary {
        let back = has_summary;
        let mut rows = if grouped {
            vec![grouped_models(host, selected.as_ref(), back)]
        } else {
            models(host, model, name, back)
        };
        if let Some(error) = error {
            rows.push(error_details("Часть моделей недоступна", error));
        }
        return stack(rows).with("transition", "models");
    }
    let mut header = vec![if grouped || model.is_some() {
        button(name, "models")
            .with("icon", "chevron-right")
            .with("variant", "ghost")
            .with("hint", "Выбрать модель")
    } else {
        text(name)
    }];
    if pending {
        let mut rows = vec![row(header), divider()];
        if let Some(efforts) = selected
            .as_ref()
            .map(|model| settings::catalog_efforts(host, model))
        {
            if let Some(default) = efforts.default() {
                rows.push(note(format!("По умолчанию: {default}")));
            }
            let options = efforts.options();
            if !options.is_empty() {
                rows.push(note(format!("Доступно: {}", options.join(", "))));
            }
        }
        if rows.len() == 2 {
            rows.push(note("Переключаем модель"));
        }
        return stack(rows).with("transition", "summary");
    }
    if stepper && !settings::by_profile(host) {
        header.push(
            button("", "effort_reset")
                .with("variant", "ghost")
                .with("icon", "rotate-ccw")
                .with("hint", "Усилие как в профиле"),
        );
    }
    let mut rows = vec![row(header)];
    if stepper {
        let shown = settings::effort_position(host, &efforts);
        let options: Vec<&Value> = efforts.iter().map(|option| option.at("name")).collect();
        rows.extend([
            divider(),
            json!({"type": "steps", "id": "reasoning", "options": options, "selected": shown, "action": "effort"}),
        ]);
    }
    for setting in &extra {
        let id = setting.str("id");
        let choices: Vec<(Value, String)> = setting
            .at("control")
            .list("options")
            .iter()
            .map(|option| (option.at("value").clone(), option.str("name").to_owned()))
            .collect();
        let selected = host.setting_value(id).unwrap_or(Value::Null);
        rows.push(select(
            id,
            setting.str("name"),
            &choices,
            &selected,
            &format!("setting:{id}"),
        ));
    }
    stack(rows).with("transition", "summary")
}

fn error_details(title: &str, error: &str) -> Value {
    disclosure("model-catalog-error", title, vec![note(error)])
}

pub fn has_groups(host: &Host, selected: Option<&SelectedModel>) -> bool {
    host.has_feature(GROUPED_PICKER) && catalog_routes(host, selected).next().is_some()
}

fn catalog_routes<'a>(
    host: &'a Host,
    selected: Option<&'a SelectedModel>,
) -> impl Iterator<Item = (&'a str, &'a str, &'a Value)> {
    settings::config(host)
        .list("model_options")
        .iter()
        .filter_map(move |entry| {
            let provider = entry.opt_str("provider").filter(|id| !id.is_empty())?;
            let model = entry.opt_str("name").filter(|id| !id.is_empty())?;
            let current = selected.is_some_and(|current| current.matches(provider, model));
            (!entry.bool("hidden") || current).then_some((provider, model, entry))
        })
}

fn provider_title(provider: &str) -> &str {
    match provider {
        "openai_codex" => "ChatGPT",
        "opencode" => "OpenCode",
        _ => provider,
    }
}

fn grouped_models(host: &Host, selected: Option<&SelectedModel>, back: bool) -> Value {
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    let mut seen = BTreeSet::new();
    for (provider, model, entry) in catalog_routes(host, selected) {
        if seen.insert((provider, model)) {
            let group = provider_title(provider);
            let label = entry
                .opt_str("label")
                .filter(|label| !label.is_empty())
                .unwrap_or(model);
            groups.entry((group, provider)).or_default().push(json!({
                "group": group,
                "label": label,
                "value": {"provider": provider, "model": model},
                "selected": selected.is_some_and(|current| current.matches(provider, model)),
            }));
        }
    }
    let options: Vec<_> = groups.into_values().flatten().collect();
    picker_header(
        json!({"type": "picker", "id": "models", "action": "model", "options": options}),
        "Model",
        back,
    )
}

fn models(host: &Host, model: Option<&Value>, name: &str, back: bool) -> Vec<Value> {
    let options = model.map(|model| model.list("options")).unwrap_or(&[]);
    if options.is_empty() {
        return vec![
            text(name).with("single_line", true),
            muted("Выбор модели недоступен"),
        ];
    }
    let catalog = settings::config(host).list("model_options");
    let common = catalog
        .first()
        .map(|item| item.str("provider"))
        .filter(|first| catalog.iter().all(|item| item.str("provider") == *first));
    let title = match common {
        Some(provider) if !provider.is_empty() => format!("Model · {provider}"),
        _ => "Model".to_owned(),
    };
    // Values stay borrowed for this render. Repeated names keep the first
    // catalog match, just as a per-choice linear search would.
    let mut providers = HashMap::new();
    if common.is_none() {
        for item in catalog {
            providers
                .entry(item.at("name"))
                .or_insert_with(|| item.str("provider"));
        }
    }
    let choices = options.iter().map(|option| {
        let value = option.at("value");
        let name = option.str("name");
        let label = match providers.get(value) {
            Some(provider) => std::borrow::Cow::Owned(format!("{provider}/{name}")),
            None => std::borrow::Cow::Borrowed(name),
        };
        (value, label)
    });
    vec![picker_header(
        picker_from("models", choices, &json!(name), "model"),
        &title,
        back,
    )]
}

fn picker_header(chooser: Value, title: &str, back: bool) -> Value {
    chooser
        .with("title", title)
        .with("back_action", back.then_some("back"))
        .with("placeholder", "Поиск Model")
        .with("empty", "Подходящих Model нет")
}
