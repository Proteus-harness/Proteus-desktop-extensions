//! The popup: model, reasoning stepper and extra session settings.

use proteus_extension::ui::{Node, button, divider, muted, picker, row, select, stack, text};
use proteus_extension::{Host, Value, ValueExt, json};

use crate::settings;

pub fn render(host: &Host, models_open: bool) -> Value {
    let model = host.setting("model");
    let name = settings::model_name(host).unwrap_or_else(|| "model".to_owned());
    let efforts = settings::efforts(host);
    let extra = settings::extra(host);
    let stepper = efforts.len() >= 2;
    if models_open || !stepper && extra.is_empty() {
        return stack(models(host, model, &name, stepper || !extra.is_empty()));
    }
    let mut rows = vec![match model {
        Some(_) => button(&name, "models")
            .with("icon", "chevron-right")
            .with("variant", "ghost")
            .with("hint", "Выбрать модель"),
        None => text(&name),
    }];
    if stepper {
        let shown = settings::effort_position(host, &efforts);
        let by_profile = settings::by_profile(host);
        let title = if by_profile {
            "как в профиле"
        } else {
            efforts[shown].str("name")
        };
        let mut header = vec![muted(format!("Рассуждения · {title}"))];
        if !by_profile {
            header.push(
                button("", "effort_reset")
                    .with("variant", "ghost")
                    .with("icon", "rotate-ccw")
                    .with("hint", "Как в профиле"),
            );
        }
        let options: Vec<&Value> = efforts.iter().map(|option| option.at("name")).collect();
        rows.extend([
            divider(),
            row(header),
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
    stack(rows)
}

fn models(host: &Host, model: Option<&Value>, name: &str, back: bool) -> Vec<Value> {
    let title = match settings::provider(host).filter(|provider| !provider.is_empty()) {
        Some(provider) => format!("Model · {provider}"),
        None => "Model".to_owned(),
    };
    let title = muted(title);
    let header = if back {
        row(vec![
            button("", "back")
                .with("variant", "ghost")
                .with("icon", "chevron-left")
                .with("hint", "Назад"),
            title,
        ])
    } else {
        title
    };
    let options = model.map(|model| model.list("options")).unwrap_or(&[]);
    let choices: Vec<_> = options
        .iter()
        .map(|option| {
            let value = option.at("value");
            (
                value.clone(),
                settings::model_label(host, value, option.str("name")),
            )
        })
        .collect();
    let rows = vec![
        header,
        picker("models", &choices, &json!(name), "model")
            .with("placeholder", "Поиск Model…")
            .with("empty", "Подходящих Model нет"),
    ];
    rows
}
