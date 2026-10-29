//! Optional native Mermaid package; parser/layout come from mermaid-rs-renderer.
// Reuse the maintained bounded cache without a second implementation.
#[path = "../../../../../packages/mermaid-render/src/cache.rs"]
mod cache;
mod render;
mod settings;

use std::sync::{Arc, Mutex, OnceLock, Weak};

use anyhow::{Context, anyhow, ensure};
use proteus_extension::ui::{self, Node};
use proteus_extension::{Extension, Host, Result, Surface, run};
use serde_json::{Value, json};
use std::path::PathBuf;

struct Shared {
    cache: cache::Cache,
    settings: settings::Settings,
    path: Option<PathBuf>,
    load_error: Option<String>,
    stamp: Option<(u64, std::time::SystemTime)>,
}

impl Default for Shared {
    fn default() -> Self {
        let path = settings::path();
        let stamp = settings::stamp(path.as_deref()).unwrap_or_default();
        let (settings, load_error) = match settings::load(path.as_deref()) {
            Ok(settings) => (settings, None),
            Err(error) => (settings::Settings::default(), Some(error.to_string())),
        };
        Self {
            cache: cache::Cache::default(),
            settings,
            path,
            load_error,
            stamp,
        }
    }
}

impl Shared {
    /// Different chats can have different extension processes. Check only on
    /// an existing render/action, never with a timer or a background watcher.
    fn sync_settings(&mut self) {
        let stamp = match settings::stamp(self.path.as_deref()) {
            Ok(stamp) => stamp,
            Err(error) => {
                self.load_error = Some(error.to_string());
                return;
            }
        };
        if self.stamp == stamp {
            return;
        }
        self.stamp = stamp;
        match settings::load(self.path.as_deref()) {
            Ok(settings) => {
                if self.settings != settings {
                    self.cache = cache::Cache::default();
                }
                self.settings = settings;
                self.load_error = None;
            }
            Err(error) => {
                self.load_error = Some(error.to_string());
            }
        }
    }
}

struct Mermaid {
    shared: Arc<Mutex<Shared>>,
    radius_draft: String,
    radius_dirty: bool,
    settings_error: Option<String>,
}

impl Default for Mermaid {
    fn default() -> Self {
        static SHARED: OnceLock<Mutex<Weak<Mutex<Shared>>>> = OnceLock::new();
        let mut registry = SHARED
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let shared = registry.upgrade().unwrap_or_else(|| {
            let shared = Arc::new(Mutex::new(Shared::default()));
            *registry = Arc::downgrade(&shared);
            shared
        });
        let current = shared.lock().unwrap_or_else(|error| error.into_inner());
        let radius_draft = current.settings.custom_radius.to_string();
        drop(current);
        Self {
            shared,
            radius_draft,
            radius_dirty: false,
            settings_error: None,
        }
    }
}

impl Extension for Mermaid {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .sync_settings();
        if surface == Surface::Page {
            if !self.radius_dirty {
                self.radius_draft = self
                    .shared
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .settings
                    .custom_radius
                    .to_string();
            }
            host.show(view, self.settings_page());
            return Ok(());
        }
        ensure!(
            surface == Surface::Content,
            "Mermaid requires a content surface"
        );
        let content = host
            .content(view)
            .ok_or_else(|| anyhow!("content source is missing"))?;
        ensure!(
            content.language == "mermaid",
            "unsupported Mermaid language"
        );
        ensure!(
            content.source.len() <= render::MAX_SOURCE_BYTES,
            "Mermaid source exceeds 50,000 bytes"
        );
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let key = cache_key(&content.source, &content.context["theme"], shared.settings);
        ensure!(key.len() <= 512 * 1024, "Mermaid request is too large");
        if let Some(root) = shared.cache.get(&key, view) {
            host.show(view, root);
            return Ok(());
        }
        let theme = shared.settings.theme(&content.context["theme"])?;
        let root = render::render(&content.source, &theme)?;
        shared.cache.insert(key, view, root.clone());
        host.show(view, root);
        Ok(())
    }

    fn closed(&mut self, _: &mut Host, view: &str) {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .cache
            .closed(view);
    }

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        ensure!(
            host.surface(view) == Some(Surface::Page),
            "Mermaid settings require a page surface"
        );
        match action {
            "radius-draft" => {
                if let Some(text) = value.as_str().filter(|text| text.len() <= 32) {
                    self.radius_draft = text.to_owned();
                    self.radius_dirty = true;
                    self.settings_error = None;
                } else {
                    self.settings_error = Some("Введите радиус от 0 до 64 px".into());
                }
            }
            "radius-mode" | "radius-apply" => {
                let outcome = (|| -> Result<bool> {
                    let mut shared = self
                        .shared
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    shared.sync_settings();
                    let mut next = shared.settings;
                    if action == "radius-mode" {
                        next.mode = settings::Mode::parse(
                            value.as_str().context("Режим скругления отсутствует")?,
                        )?;
                    } else {
                        next.custom_radius = settings::radius_input(&self.radius_draft)?;
                    }
                    if next != shared.settings {
                        settings::save(
                            shared
                                .path
                                .as_deref()
                                .context("Нет HOME или XDG_CONFIG_HOME для настроек Mermaid")?,
                            next,
                        )?;
                        shared.settings = next;
                        shared.stamp = None;
                        shared.cache = cache::Cache::default();
                        shared.load_error = None;
                        return Ok(true);
                    }
                    Ok(false)
                })();
                if outcome.is_ok() && action == "radius-apply" {
                    self.radius_dirty = false;
                }
                self.settings_error = match outcome {
                    Ok(changed) => {
                        if changed {
                            host.invalidate_content()?;
                        }
                        None
                    }
                    Err(error) => Some(error.to_string()),
                };
            }
            _ => return Err(anyhow!("Unknown Mermaid settings action")),
        }
        Ok(())
    }
}

impl Mermaid {
    fn settings_page(&self) -> Value {
        let shared = self
            .shared
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let selected = json!(shared.settings.mode.key());
        let mut body = vec![
            ui::text("Скругление блоков Mermaid"),
            ui::select(
                "radius-mode",
                match shared.settings.mode {
                    settings::Mode::Inherit => "Наследовать из приложения",
                    settings::Mode::Custom => "Свой радиус",
                },
                &[
                    (json!("inherit"), "Наследовать из приложения".into()),
                    (json!("custom"), "Свой радиус".into()),
                ],
                &selected,
                "radius-mode",
            ),
        ];
        if shared.settings.mode == settings::Mode::Custom {
            body.push(ui::text("Радиус, px"));
            body.push(ui::row(vec![
                ui::input("radius", &self.radius_draft, "8", "radius-draft")
                    .with("submit", "radius-apply"),
                ui::button("Применить", "radius-apply"),
            ]));
            body.push(ui::note(
                "От 0 до 64 px. 0 — острые углы. Свой радиус не зависит от темы приложения.",
            ));
        } else {
            body.push(ui::note("Используется радиус приложения. Для более мягкого скругления выберите свой радиус, например 8 px."));
        }
        if let Some(error) = self.settings_error.as_ref().or(shared.load_error.as_ref()) {
            body.push(ui::text(error).with("tone", "danger"));
        }
        ui::stack(body).with("gap", "medium")
    }
}

fn cache_key(source: &str, theme: &Value, settings: settings::Settings) -> String {
    json!({"source":source,"theme":theme,"settings":settings.value()}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_cache_keys_include_settings_and_app_theme() {
        let theme = json!({"radius_lg":16});
        let inherited = settings::Settings::default();
        let custom = settings::Settings {
            mode: settings::Mode::Custom,
            ..inherited
        };
        assert_ne!(
            cache_key("source", &theme, inherited),
            cache_key("source", &theme, custom)
        );
        assert_ne!(
            cache_key("source", &theme, custom),
            cache_key(
                "source",
                &theme,
                settings::Settings {
                    custom_radius: 4.0,
                    ..custom
                }
            )
        );
        assert_ne!(
            cache_key("source", &theme, inherited),
            cache_key("source", &json!({"radius_lg":32}), inherited)
        );
    }

    #[test]
    fn another_process_preferences_apply_on_render_without_polling_or_shared_sources() {
        let root = std::env::temp_dir().join(format!(
            "proteus-mermaid-pref-refresh-{}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("settings.json");
        let mut reader = Shared {
            cache: cache::Cache::default(),
            settings: settings::Settings::default(),
            path: Some(path.clone()),
            load_error: None,
            stamp: None,
        };
        reader
            .cache
            .insert("old".into(), "diagram", json!({"type":"svg"}));
        let custom = settings::Settings {
            mode: settings::Mode::Custom,
            custom_radius: 4.0,
        };
        settings::save(&path, custom).unwrap();
        reader.sync_settings();
        assert_eq!(reader.settings, custom);
        assert!(reader.cache.get("old", "diagram").is_none());
        let next = settings::Settings {
            custom_radius: 32.0,
            ..custom
        };
        settings::save(&path, next).unwrap();
        reader.sync_settings();
        assert_eq!(reader.settings, next);
        std::fs::write(&path, "invalid JSON").unwrap();
        reader.sync_settings();
        assert_eq!(reader.settings, next);
        assert!(reader.load_error.is_some());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn settings_page_uses_existing_public_controls_and_custom_default_eight() {
        let custom = settings::Settings {
            mode: settings::Mode::Custom,
            ..settings::Settings::default()
        };
        let page = Mermaid {
            shared: Arc::new(Mutex::new(Shared {
                cache: cache::Cache::default(),
                settings: custom,
                path: None,
                load_error: None,
                stamp: None,
            })),
            radius_draft: "8".into(),
            radius_dirty: false,
            settings_error: None,
        }
        .settings_page();
        let children = page["children"].as_array().unwrap();
        assert_eq!(children[1]["type"], "select");
        assert_eq!(children[1]["options"][0]["value"], "inherit");
        assert_eq!(children[1]["options"][1]["value"], "custom");
        assert_eq!(children[3]["children"][0]["type"], "input");
        assert_eq!(children[3]["children"][0]["value"], "8");
        assert_eq!(children[3]["children"][0]["submit"], "radius-apply");
    }
}

fn main() {
    run(Mermaid::default());
}
