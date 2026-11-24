//! Window-scoped display preferences for indicators with optional data.

use crate::{Host, Result, Value, json};

use super::{Node, select, setting_reset, setting_row, stack};

const HIDE: &str = "hide_when_empty";
const DISPLAY: &str = "ring_display";
const DISPLAY_FEATURE: &str = "surfaces.badge.ring_display.v1";
const FORMATS: [(&str, &str); 3] = [
    ("ring_number", "Кольцо с числом"),
    ("ring", "Только кольцо"),
    ("number", "Только процент"),
];

pub fn hide_when_empty(host: &Host, default: bool) -> bool {
    host.preferences()
        .get(HIDE)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn format(host: &Host) -> (&'static str, &'static str) {
    let saved = host.preferences().get(DISPLAY).and_then(Value::as_str);
    FORMATS
        .into_iter()
        .find(|&(id, _)| Some(id) == saved)
        .unwrap_or(FORMATS[0])
}

pub fn ring(host: &Host, percent: f64, tone: &str) -> Value {
    let (display, _) = format(host);
    json!({"type":"ring", "percent":percent, "tone":tone}).maybe(
        "display",
        (display != FORMATS[0].0 && host.has_feature(DISPLAY_FEATURE)).then_some(display),
    )
}

pub fn page(host: &Host, default_hide: bool, configurable_ring: bool) -> Value {
    let hide = hide_when_empty(host, default_hide);
    let choices = [
        (json!(true), "Скрывать".into()),
        (json!(false), "Показывать".into()),
    ];
    let visibility = setting_row(
        "badge-visibility",
        "При отсутствии данных",
        select(
            HIDE,
            if hide {
                "Скрывать"
            } else {
                "Показывать"
            },
            &choices,
            &json!(hide),
            HIDE,
        ),
    )
    .maybe(
        "reset",
        (hide != default_hide).then(|| setting_reset("reset_hide_when_empty")),
    );
    let mut rows = vec![visibility];
    if configurable_ring {
        let (selected, label) = format(host);
        let choices = FORMATS.map(|(id, label)| (json!(id), label.to_owned()));
        rows.push(
            setting_row(
                "badge-display",
                "Вид индикатора",
                select(DISPLAY, label, &choices, &json!(selected), DISPLAY),
            )
            .maybe(
                "reset",
                (selected != FORMATS[0].0).then(|| setting_reset("reset_ring_display")),
            ),
        );
    }
    stack(rows)
}

pub fn action(
    host: &mut Host,
    action: &str,
    value: &Value,
    default_hide: bool,
    configurable_ring: bool,
) -> Result {
    match action {
        HIDE => {
            if let Some(hide) = value.as_bool() {
                host.set_visual_preference(HIDE, (hide != default_hide).then(|| json!(hide)))?;
            }
        }
        "reset_hide_when_empty" => host.set_visual_preference(HIDE, None)?,
        DISPLAY if configurable_ring => {
            if let Some(display) = value
                .as_str()
                .filter(|&id| FORMATS.iter().any(|format| format.0 == id))
            {
                host.set_visual_preference(
                    DISPLAY,
                    (display != FORMATS[0].0).then(|| json!(display)),
                )?;
            }
        }
        "reset_ring_display" if configurable_ring => host.set_visual_preference(DISPLAY, None)?,
        _ => {}
    }
    Ok(())
}
