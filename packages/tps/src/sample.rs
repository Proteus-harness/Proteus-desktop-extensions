//! Read provider usage without retaining the session's full request history.

use proteus_extension::{Value, ValueExt};

pub const EXPLANATION: &str = "Выходные токены провайдера / полное время запроса. Ожидание ответа включено, работа инструментов — нет. Это средняя скорость последнего завершённого запроса, не скорость в реальном времени.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub provider: String,
    pub name: String,
}

impl Model {
    pub fn selected(state: &Value) -> Option<Self> {
        let model = state.at("config").at("model");
        Some(Self {
            provider: model.opt_str("provider")?.to_owned(),
            name: model.opt_str("name")?.to_owned(),
        })
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
        .filter(|request| {
            request.str("origin") == "direct"
                && request.str("status") == "completed"
                && request.at("finished_at_ms").as_i64().is_some()
                && selected.is_none_or(|selected| {
                    request.at("model").str("provider") == selected.provider
                        && request.at("model").str("model") == selected.name
                })
        })
        .max_by_key(|request| request.at("finished_at_ms").as_i64());
    let Some(request) = request else {
        return Ok(None);
    };
    let start = request
        .at("started_at_ms")
        .as_i64()
        .ok_or("Провайдер не сообщил время начала запроса.")?;
    let elapsed_ms = request
        .at("finished_at_ms")
        .as_i64()
        .and_then(|end| end.checked_sub(start))
        .filter(|duration| *duration > 0)
        .ok_or("Длительность запроса неизвестна или некорректна.")? as u64;
    let output = request
        .at("usage")
        .at("output_tokens")
        .as_u64()
        .ok_or("Провайдер не сообщил число выходных токенов.")?;
    Ok(Some(Sample {
        model: Model {
            provider: request.at("model").str("provider").to_owned(),
            name: request.at("model").str("model").to_owned(),
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
    fn positive_measurements_below_display_precision_stay_distinct_from_zero() {
        assert_eq!(decimal(0.0), "0,0");
        assert_eq!(decimal(1.0 / 30.0), "<0,1"); // One token over 30 seconds.
        assert_eq!(decimal(37.0 / 1000.0), "<0,1"); // A 37 ms request, in seconds.
        assert_eq!(decimal(0.05), "0,1");
        assert_eq!(decimal(12.5), "12,5");
    }
}
