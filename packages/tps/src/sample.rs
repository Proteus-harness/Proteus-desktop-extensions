//! Read provider usage without retaining the session's full request history.

use proteus_extension::{Value, ValueExt};

pub const EXPLANATION: &str = "Выходные токены провайдера / полное время запроса. Ожидание ответа включено, работа инструментов — нет. Это средняя скорость последнего завершённого запроса, не скорость в реальном времени.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub provider: String,
    pub name: String,
}

impl Model {
    pub fn selected(state: &Value) -> Option<(&str, &str)> {
        let model = state.at("config").at("model");
        Some((model.opt_str("provider")?, model.opt_str("name")?))
    }

    pub fn key(&self) -> (&str, &str) {
        (&self.provider, &self.name)
    }

    pub fn label(&self) -> String {
        format!("{}/{}", self.provider, self.name)
    }
}

#[derive(Debug)]
pub struct Sample {
    pub model: Model,
    pub output: u64,
    pub elapsed_ms: u64,
}

impl Sample {
    pub fn rate(&self) -> f64 {
        self.output as f64 * 1000. / self.elapsed_ms as f64
    }
}

/// Pick the latest *finished* ordinary request. Missing usage on that request
/// remains unknown rather than silently displaying an older successful sample.
pub fn latest(snapshot: &Value, selected: Option<&Model>) -> Result<Option<Sample>, &'static str> {
    let requests = snapshot
        .get("requests")
        .and_then(Value::as_array)
        .ok_or("Журнал скорости недоступен для этого чата.")?;
    let request = requests
        .iter()
        .filter_map(|request| {
            if request.str("origin") != "direct" || request.str("status") != "completed" {
                return None;
            }
            let finished_at = request.at("finished_at_ms").as_i64()?;
            if let Some(selected) = selected {
                let model = request.at("model");
                if model.str("provider") != selected.provider || model.str("model") != selected.name
                {
                    return None;
                }
            }
            Some((request, finished_at))
        })
        .max_by_key(|&(_, finished_at)| finished_at);
    let Some((request, finished_at)) = request else {
        return Ok(None);
    };
    let start = request
        .at("started_at_ms")
        .as_i64()
        .ok_or("Провайдер не сообщил время начала запроса.")?;
    let elapsed_ms = finished_at
        .checked_sub(start)
        .filter(|duration| *duration > 0)
        .ok_or("Длительность запроса неизвестна или некорректна.")? as u64;
    let output = request
        .at("usage")
        .at("output_tokens")
        .as_u64()
        .ok_or("Провайдер не сообщил число выходных токенов.")?;
    let model = request.at("model");
    Ok(Some(Sample {
        model: Model {
            provider: model.str("provider").to_owned(),
            name: model.str("model").to_owned(),
        },
        output,
        elapsed_ms,
    }))
}

pub fn decimal(value: f64) -> String {
    // A small positive rate or duration would otherwise read exactly like zero.
    if value > 0.0 && value < 0.05 {
        return "<0,1".into();
    }
    format!("{value:.1}").replace('.', ",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_extension::json;

    fn request(end: i64, output: Value) -> Value {
        json!({"origin":"direct","status":"completed","started_at_ms":1000,
            "finished_at_ms":end,"model":{"provider":"fixture","model":"model"},
            "usage":{"output_tokens":output,"reasoning_output_tokens":99}})
    }

    #[test]
    fn selected_model_borrows_two_typed_fields() {
        let state = json!({"config":{"model":{"provider":"","name":"fixture"}}});
        let (provider, name) = Model::selected(&state).unwrap();
        assert_eq!((provider, name), ("", "fixture"));
        let original = state["config"]["model"]["name"].as_str().unwrap();
        assert!(std::ptr::eq(name, original));
        let malformed = json!({"config":{"model":{"provider":"","name":7}}});
        assert!(Model::selected(&malformed).is_none());
    }

    #[test]
    fn rate_uses_output_once_and_latest_finish_not_array_order_or_helpers() {
        let mut helper = request(9000, json!(900));
        helper["origin"] = json!("compaction");
        let mut running = request(10000, json!(999));
        running["status"] = json!("unfinished");
        let sample = latest(
            &json!({"requests":[request(3000,json!(100)),request(2000,json!(90)),helper,running]}),
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(sample.rate(), 50.);
        assert_eq!(decimal(sample.rate()), "50,0");
        assert_eq!(sample.output, 100);
    }

    #[test]
    fn unavailable_latest_sample_does_not_fall_back_to_old_speed() {
        for output in [Value::Null, json!(-1), json!(1.5)] {
            assert!(
                latest(
                    &json!({"requests":[request(2000,json!(100)),request(4000,output)]}),
                    None
                )
                .is_err()
            );
        }
        for end in [1000, 500] {
            assert!(latest(&json!({"requests":[request(end,json!(100))]}), None).is_err());
        }
        assert_eq!(
            latest(&json!({"requests":[request(2000,json!(0))]}), None)
                .unwrap()
                .unwrap()
                .rate(),
            0.
        );
        assert!(latest(&Value::Null, None).is_err());
    }

    #[test]
    fn selected_model_does_not_show_previous_models_rate() {
        let selected = Model {
            provider: "fixture".into(),
            name: "new-model".into(),
        };
        assert!(
            latest(
                &json!({"requests":[request(2000,json!(90))]}),
                Some(&selected)
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn equal_finish_keeps_the_last_eligible_request_of_the_selected_model() {
        let selected = Model {
            provider: "fixture".into(),
            name: "model".into(),
        };
        let mut other_name = request(4000, json!(400));
        other_name["model"]["model"] = json!("other");
        let mut other_provider = request(5000, json!(500));
        other_provider["model"]["provider"] = json!("other");
        let mut unfinished = request(6000, json!(600));
        unfinished["finished_at_ms"] = json!(null);
        let snapshot = json!({"requests":[request(3000,json!(50)), other_name,
            request(3000,json!(120)), other_provider, unfinished, request(2500,json!(25))]});
        let sample = latest(&snapshot, Some(&selected)).unwrap().unwrap();
        assert_eq!((sample.output, sample.elapsed_ms), (120, 2000));
        assert_eq!(sample.model, selected);
        let unfiltered = latest(&snapshot, None).unwrap().unwrap();
        assert_eq!((unfiltered.output, unfiltered.elapsed_ms), (500, 4000));
    }

    #[test]
    fn selected_finish_preserves_error_priority_and_checked_duration() {
        let first = request(i64::MAX, json!(30));
        let mut last = request(i64::MAX, Value::Null);
        last["started_at_ms"] = Value::Null;
        assert_eq!(
            latest(&json!({"requests":[&first, &last]}), None).unwrap_err(),
            "Провайдер не сообщил время начала запроса."
        );
        last["started_at_ms"] = json!(i64::MIN);
        assert_eq!(
            latest(&json!({"requests":[&first, &last]}), None).unwrap_err(),
            "Длительность запроса неизвестна или некорректна."
        );
        last["started_at_ms"] = json!(i64::MAX - 1000);
        assert_eq!(
            latest(&json!({"requests":[&first, &last]}), None).unwrap_err(),
            "Провайдер не сообщил число выходных токенов."
        );
    }

    #[test]
    fn positive_measurements_below_display_precision_stay_distinct_from_zero() {
        assert_eq!(decimal(0.0), "0,0");
        assert_eq!(decimal(1.0 / 30.0), "<0,1"); // One token over 30 seconds.
        assert_eq!(decimal(37.0 / 1000.0), "<0,1"); // A 37 ms request, in seconds.
        assert_eq!(decimal(0.05), "0,1");
        assert_eq!(decimal(12.5), "12,5");
    }
}
