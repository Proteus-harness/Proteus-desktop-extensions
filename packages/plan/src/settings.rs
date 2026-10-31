use proteus_extension::{Preferences, Result, Value, json, ui};
use ui::Node as _;

use crate::model::Visibility;

pub fn visibility(preferences: &Preferences) -> Visibility {
    preferences
        .get("visibility")
        .and_then(Value::as_str)
        .and_then(Visibility::parse)
        .unwrap_or_default()
}

pub fn save(preferences: &mut Preferences, visibility: Visibility) -> Result {
    if visibility == Visibility::Current {
        preferences.remove("visibility")
    } else {
        preferences.set("visibility", json!(visibility.value()))
    }
}

pub fn page(visibility: Visibility) -> Value {
    let choices = [
        (json!("current"), "По наличию".to_owned()),
        (json!("always"), "Всегда".to_owned()),
    ];
    let selected = json!(visibility.value());
    let label = choices
        .iter()
        .find(|(value, _)| value == &selected)
        .map(|(_, label)| label.as_str())
        .expect("visibility has a matching option");
    let mut row = ui::setting_row(
        "visibility-row",
        "Показывать план",
        ui::select(
            "visibility",
            label,
            &choices,
            &selected,
            "visibility",
        ),
    )
    .with(
        "description",
        "По наличию — план текущего хода; завершённый план скрывается после ответа. Всегда — последний план чата, в том числе завершённый. Если агент ещё не создавал план, он не показывается.",
    );
    if visibility != Visibility::default() {
        row = row.with("reset", ui::setting_reset("reset_visibility"));
    }
    ui::stack(vec![row])
}
