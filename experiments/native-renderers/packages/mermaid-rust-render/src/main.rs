//! Optional native Mermaid package; parser/layout come from mermaid-rs-renderer.
// Reuse the maintained bounded cache without a second implementation.
#[path = "../../../../../packages/mermaid-render/src/cache.rs"]
mod cache;
mod render;
mod settings;

use std::sync::{Arc, Mutex, OnceLock, Weak};

use anyhow::{Context, anyhow, ensure};
use proteus_extension::ui::{self, Node};
use proteus_extension::{Content, Extension, Host, Result, Surface, run};
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
            matches!(surface, Surface::Content | Surface::Preview),
            "Mermaid requires a content surface"
        );
        let sample;
        let content = if surface == Surface::Preview {
            sample = Content {
                language: "mermaid".into(),
                source: PREVIEW_SOURCE.into(),
                context: host
                    .content(view)
                    .map(|content| content.context.clone())
                    .unwrap_or_else(|| json!({"theme":{}})),
                source_revision: None,
            };
            &sample
        } else {
            host.content(view)
                .ok_or_else(|| anyhow!("content source is missing"))?
        };
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
            "radius-mode" | "radius-apply" | "radius-mode-reset" | "radius-reset" => {
                let outcome = self.apply_setting(action, value);
                self.settings_error = match outcome {
                    Ok(changed) => {
                        if changed {
                            host.invalidate_surfaces()?;
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
    /// Changes one parameter only; the caller invalidates existing content on change.
    fn apply_setting(&mut self, action: &str, value: &Value) -> Result<bool> {
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        shared.sync_settings();
        let mut next = shared.settings;
        match action {
            "radius-mode" => {
                next.mode =
                    settings::Mode::parse(value.as_str().context("Режим скругления отсутствует")?)?;
            }
            "radius-apply" => {
                next.custom_radius = settings::radius_input(&self.radius_draft)?;
            }
            "radius-mode-reset" => next.mode = settings::Settings::default().mode,
            "radius-reset" => next.custom_radius = settings::Settings::default().custom_radius,
            _ => return Err(anyhow!("Unknown Mermaid settings action")),
        }
        let changed = next != shared.settings;
        if changed {
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
        }
        if matches!(action, "radius-apply" | "radius-reset") {
            self.radius_draft = next.custom_radius.to_string();
            self.radius_dirty = false;
        }
        self.settings_error = None;
        Ok(changed)
    }

    fn settings_page(&self) -> Value {
        let shared = self
            .shared
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let selected = json!(shared.settings.mode.key());
        let mut body = vec![
            ui::setting_row(
                "mermaid-radius-mode",
                "Скругление блоков",
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
            )
            .with(
                "description",
                "Наследовать радиус темы приложения или задать свой радиус только для Mermaid.",
            )
            .maybe(
                "reset",
                (shared.settings.mode != settings::Settings::default().mode).then(|| {
                    ui::setting_reset("radius-mode-reset")
                        .with("hint", "Наследовать скругление приложения")
                }),
            ),
        ];
        if shared.settings.mode == settings::Mode::Custom {
            body.push(ui::setting_row(
                "mermaid-custom-radius",
                "Радиус, px",
                ui::input("radius", &self.radius_draft, "8", "radius-draft")
                    .with("submit", "radius-apply"),
            ).with("description", "От 0 до 64 px. 0 — острые углы. Enter сохраняет значение; свой радиус не зависит от темы приложения.")
                .maybe("reset", (shared.settings.custom_radius != settings::Settings::default().custom_radius
                    || settings::radius_input(&self.radius_draft).ok() != Some(settings::Settings::default().custom_radius))
                    .then(|| ui::setting_reset("radius-reset").with("hint", "По умолчанию: 8 px"))));
        }
        if let Some(error) = self.settings_error.as_ref().or(shared.load_error.as_ref()) {
            body.push(ui::text(error).with("tone", "danger"));
        }
        ui::stack(body).with("gap", "none")
    }
}

fn cache_key(source: &str, theme: &Value, settings: settings::Settings) -> String {
    json!({"source":source,"theme":theme,"settings":settings.value()}).to_string()
}

const PREVIEW_SOURCE: &str = "flowchart LR\n    A[Запрос] --> B{Проверка}\n    B -->|Готово| C[Результат]\n    B -->|Уточнить| A\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_preview_uses_the_production_renderer_and_current_settings() {
        let theme = json!({"foreground":"#abcdef", "background":"#111213", "muted":"#8a8b8c", "border":"#414243", "accent":"#2789fa", "danger":"#ee5566", "surface":"#222324", "surface_alt":"#303132", "font_family":"DejaVu Sans", "radius_lg":16.0});
        let custom = settings::Settings {
            mode: settings::Mode::Custom,
            custom_radius: 4.0,
        };
        let rendered = render::render(PREVIEW_SOURCE, &custom.theme(&theme).unwrap()).unwrap();
        assert_eq!(rendered["type"], "svg");
        assert!(rendered["source"].as_str().unwrap().contains("Результат"));
        assert_ne!(
            rendered,
            render::render(
                PREVIEW_SOURCE,
                &settings::Settings::default().theme(&theme).unwrap()
            )
            .unwrap()
        );
    }

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

    fn fixture(settings: settings::Settings, path: Option<PathBuf>) -> Mermaid {
        Mermaid {
            shared: Arc::new(Mutex::new(Shared {
                cache: cache::Cache::default(),
                settings,
                path,
                load_error: None,
                stamp: None,
            })),
            radius_draft: settings.custom_radius.to_string(),
            radius_dirty: false,
            settings_error: None,
        }
    }

    #[test]
    fn resets_preserve_the_other_setting_and_persist_before_clearing_drafts() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("build/test-settings-reset")
            .join(format!(
                "{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        let custom = settings::Settings {
            mode: settings::Mode::Custom,
            custom_radius: 24.0,
        };
        settings::save(&path, custom).unwrap();
        let mut extension = fixture(custom, Some(path.clone()));
        extension.shared.lock().unwrap().cache.insert(
            "old".into(),
            "diagram",
            json!({"type":"svg"}),
        );
        assert!(
            extension
                .apply_setting("radius-mode-reset", &Value::Null)
                .unwrap()
        );
        let inherited = settings::load(Some(&path)).unwrap();
        assert_eq!(inherited.mode, settings::Mode::Inherit);
        assert_eq!(inherited.custom_radius, 24.0);
        assert!(
            extension
                .shared
                .lock()
                .unwrap()
                .cache
                .get("old", "diagram")
                .is_none()
        );
        assert!(
            extension.settings_page()["children"][0]
                .get("reset")
                .is_none()
        );
        extension
            .apply_setting("radius-mode", &json!("custom"))
            .unwrap();
        extension.radius_draft = "invalid".into();
        extension.radius_dirty = true;
        extension.settings_error = Some("invalid radius".into());
        assert!(
            extension
                .apply_setting("radius-reset", &Value::Null)
                .unwrap()
        );
        let reset = settings::load(Some(&path)).unwrap();
        assert_eq!(reset.mode, settings::Mode::Custom);
        assert_eq!(reset.custom_radius, 8.0);
        assert_eq!(extension.radius_draft, "8");
        assert!(!extension.radius_dirty);
        assert!(extension.settings_error.is_none());
        assert!(
            extension.settings_page()["children"][1]
                .get("reset")
                .is_none()
        );
        // A bad draft needs reset even when the stored radius already equals 8.
        extension.radius_draft = "bad".into();
        extension.radius_dirty = true;
        assert_eq!(
            extension.settings_page()["children"][1]["reset"]["action"],
            "radius-reset"
        );
        assert!(
            !extension
                .apply_setting("radius-reset", &Value::Null)
                .unwrap()
        );
        assert_eq!(extension.radius_draft, "8");
        assert!(!extension.radius_dirty);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reset_save_failure_preserves_settings_and_the_pending_draft() {
        let custom = settings::Settings {
            mode: settings::Mode::Custom,
            custom_radius: 24.0,
        };
        let mut extension = fixture(custom, None);
        extension.radius_draft = "invalid".into();
        extension.radius_dirty = true;
        assert!(
            extension
                .apply_setting("radius-reset", &Value::Null)
                .is_err()
        );
        assert_eq!(extension.shared.lock().unwrap().settings, custom);
        assert_eq!(extension.radius_draft, "invalid");
        assert!(extension.radius_dirty);
    }

    #[test]
    fn settings_page_uses_shared_rows_and_enter_to_save_custom_radius() {
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
        assert_eq!(children.len(), 2);
        assert_eq!(children[0]["type"], "setting_row");
        assert_eq!(children[0]["control"]["type"], "select");
        assert_eq!(children[0]["reset"]["action"], "radius-mode-reset");
        assert!(children[1].get("reset").is_none());
        assert_eq!(children[0]["control"]["options"][0]["value"], "inherit");
        assert_eq!(children[0]["control"]["options"][1]["value"], "custom");
        assert_eq!(children[1]["control"]["type"], "input");
        assert_eq!(children[1]["control"]["value"], "8");
        assert_eq!(children[1]["control"]["submit"], "radius-apply");
        assert!(
            children[1]["description"]
                .as_str()
                .unwrap()
                .contains("Enter")
        );
        assert!(!serde_json::to_string(&page).unwrap().contains("Применить"));
    }
}

fn main() {
    run(Mermaid::default());
}
