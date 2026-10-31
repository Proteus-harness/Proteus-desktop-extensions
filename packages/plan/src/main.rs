mod cache;
mod model;
mod settings;

use proteus_extension::{
    ApiError, Error, Extension, Host, Preferences, Result, Surface, Value, ValueExt, run, ui,
};
use ui::Node as _;

use model::Visibility;

#[derive(Default)]
struct Plan {
    always: cache::Cache,
}

const UNAVAILABLE: &str = "План недоступен";

fn unavailable_message(error: &Error) -> String {
    if error
        .downcast_ref::<ApiError>()
        .is_some_and(|error| error.status == 409)
    {
        "История чата ещё не готова. Повторите запрос.".into()
    } else {
        error.to_string()
    }
}

fn unavailable_details(message: &str) -> Value {
    ui::stack(vec![
        ui::note(message),
        ui::button("Повторить", "retry_plan").with("icon", "refresh-cw"),
    ])
}

impl Extension for Plan {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        if surface == Surface::Preview {
            // The host opens this only on demand. It owns no retained data and
            // never reads entries, session state, preferences, or the server.
            host.show(view, model::details(&model::demo()));
            return Ok(());
        }
        let preferences = Preferences::load("plan");
        let visibility = settings::visibility(&preferences);
        match surface {
            Surface::Page => {
                host.show(view, settings::page(visibility));
            }
            Surface::Badge | Surface::Popup => {
                let running = host
                    .state
                    .as_ref()
                    .is_some_and(|state| !state.at("execution").at("active").is_null());
                let current;
                let snapshot = if visibility == Visibility::Always {
                    self.always.always(host)
                } else {
                    self.always.clear();
                    current = model::latest(&host.entries, visibility, running);
                    Ok(current.as_ref())
                };
                let snapshot = match snapshot {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        let message = unavailable_message(&error);
                        if surface == Surface::Badge {
                            host.badge(
                                view,
                                UNAVAILABLE,
                                vec![message],
                                Some(proteus_extension::json!({
                                    "type":"text", "icon":true, "text":UNAVAILABLE, "tone":"muted"
                                })),
                            );
                        } else {
                            host.show(view, unavailable_details(&message));
                        }
                        return Ok(());
                    }
                };
                match (surface, snapshot) {
                    (Surface::Badge, Some(plan)) => {
                        let title = plan.summary();
                        host.badge(
                            view,
                            &title,
                            plan.current().map(str::to_owned).into_iter().collect(),
                            Some(proteus_extension::json!({
                                "type":"text", "icon":true, "text":title, "tone":"muted"
                            })),
                        );
                    }
                    (Surface::Badge, None) => host.hide_badge(view, "План"),
                    (_, Some(plan)) => host.show(view, model::details(&plan)),
                    (_, None) => host.show(view, ui::stack(Vec::new())),
                }
            }
            _ => host.show(view, ui::stack(Vec::new())),
        }
        Ok(())
    }

    fn session_changed(&mut self, _: &mut Host) {
        self.always.clear();
    }

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        if action == "retry_plan" && host.surface(view) == Some(Surface::Popup) {
            if settings::visibility(&Preferences::load("plan")) == Visibility::Always {
                // The SDK rerenders this process's open surfaces after the
                // action. Their shared cache admits exactly one new read.
                self.always.retry();
            }
            return Ok(());
        }
        if host.surface(view) != Some(Surface::Page) {
            return Ok(());
        }
        match action {
            "visibility" => {
                if let Some(mode) = value.as_str().and_then(Visibility::parse) {
                    settings::save(&mut Preferences::load("plan"), mode)?;
                    host.invalidate_surfaces()?;
                }
            }
            "reset_visibility" => {
                Preferences::load("plan").remove("visibility")?;
                host.invalidate_surfaces()?;
            }
            _ => {}
        }
        Ok(())
    }
}

fn main() {
    run(Plan::default());
}

#[cfg(test)]
mod tests;
