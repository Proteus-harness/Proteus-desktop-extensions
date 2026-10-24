//! The popup: model, reasoning stepper and extra session settings.

use proteus_extension::ui::{
    Node, button, disclosure, divider, muted, note, picker, row, select, stack, text,
};
use proteus_extension::{Host, Value, ValueExt, json};

use crate::settings;

pub fn render(host: &Host, models_open: bool) -> Value {
    let model = host.setting("model");
    let name = settings::model_name(host).unwrap_or_else(|| "Model".to_owned());
    if let Some(error) = settings::catalog_error(host) {
        return stack(vec![
            text(&name).with("single_line", true),
            text("Каталог моделей недоступен").with("tone", "warning"),
            disclosure("model-catalog-error", "Подробности", vec![note(error)]),
        ])
        .with("transition", "catalog-error");
    }
    let efforts = settings::efforts(host);
    let extra = settings::extra(host);
    let stepper = efforts.len() >= 2;
    if models_open || !stepper && extra.is_empty() {
        return stack(models(host, model, &name, stepper || !extra.is_empty()))
            .with("transition", "models");
    }
    let mut header = vec![match model {
        Some(_) => button(&name, "models")
            .with("icon", "chevron-right")
            .with("variant", "ghost")
            .with("hint", "Выбрать модель"),
        None => text(&name),
    }];
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

fn models(host: &Host, model: Option<&Value>, name: &str, back: bool) -> Vec<Value> {
    let title = match settings::provider(host).filter(|provider| !provider.is_empty()) {
        Some(provider) => format!("Model · {provider}"),
        None => "Model".to_owned(),
    };
    let options = model.map(|model| model.list("options")).unwrap_or(&[]);
    if options.is_empty() {
        return vec![
            text(name).with("single_line", true),
            muted("Выбор модели недоступен"),
        ];
    }
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
        picker("models", &choices, &json!(name), "model")
            .with("title", title)
            .with("back_action", back.then_some("back"))
            .with("placeholder", "Поиск Model")
            .with("empty", "Подходящих Model нет"),
    ];
    rows
}
