//! Mermaid remains a standalone process package. The host receives generic SVG.
mod cache;
mod worker;

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use anyhow::{anyhow, ensure};
use proteus_extension::{Extension, Host, Result, Surface, Value, run};
use serde::ser::{Serialize, SerializeStruct, Serializer};

struct Mermaid {
    shared: Arc<Mutex<Pool>>,
}

impl Default for Mermaid {
    fn default() -> Self {
        // SDK instances belong to individual sessions; the renderer belongs to
        // the package process. A Weak registry lets the last instance drop it.
        static SHARED: OnceLock<Mutex<Weak<Mutex<Pool>>>> = OnceLock::new();
        let mut registry = SHARED
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let shared = registry.upgrade().unwrap_or_else(|| {
            let shared = Arc::new(Mutex::new(Pool::default()));
            *registry = Arc::downgrade(&shared);
            shared
        });
        Self { shared }
    }
}

#[derive(Default)]
struct Pool {
    worker: Option<worker::Worker>,
    views: HashSet<String>,
    cache: cache::Cache,
}

impl Extension for Mermaid {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .render(host, view, surface)
    }

    fn closed(&mut self, _host: &mut Host, view: &str) {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closed(view);
    }
}

impl Pool {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface) -> Result {
        ensure!(
            matches!(surface, Surface::Content | Surface::Preview),
            "Mermaid requires a content surface"
        );
        let (source, theme) = if surface == Surface::Preview {
            (
                SAMPLE,
                host.content(view)
                    .and_then(|content| content.context.get("theme")),
            )
        } else {
            let content = host
                .content(view)
                .ok_or_else(|| anyhow!("content source is missing"))?;
            ensure!(
                content.language == "mermaid",
                "unsupported Mermaid language"
            );
            (content.source.as_str(), content.context.get("theme"))
        };
        ensure!(
            source.len() <= 50_000,
            "Mermaid source exceeds 50,000 bytes"
        );
        let key = render_request(source, theme)?;
        ensure!(key.len() <= 512 * 1024, "Mermaid request is too large");
        self.views.insert(view.to_owned());
        if let Some(root) = self.cache.get(&key, view) {
            host.show_shared_root(view, root);
            return Ok(());
        }
        if self.worker.is_none() {
            // The public extension launcher sets cwd to the package directory.
            self.worker = Some(worker::Worker::start(Path::new("worker.mjs"))?);
        }
        let (key, root) = match self.worker.as_mut().unwrap().render(key) {
            Ok(rendered) => rendered,
            Err(error) => {
                self.worker.take();
                return Err(error);
            }
        };
        let root = Arc::new(root);
        self.cache.insert(key, view, Arc::clone(&root));
        host.show_shared_root(view, root);
        Ok(())
    }

    fn closed(&mut self, view: &str) {
        self.views.remove(view);
        self.cache.closed(view);
        if self.views.is_empty() {
            self.worker.take(); // Drop kills and waits; no orphan renderer remains.
            self.cache = cache::Cache::default();
        }
    }
}

const SAMPLE: &str = "flowchart LR\n    A[Запрос] --> B{Проверка}\n    B -->|Готово| C[Результат]\n    B -->|Уточнить| A\n";

// Both the worker and the cache use the same compact JSON line. Serialize
// borrowed fields directly rather than first copying them into a JSON tree.
fn render_request(source: &str, theme: Option<&Value>) -> serde_json::Result<String> {
    struct Request<'a> {
        source: &'a str,
        theme: Option<&'a Value>,
    }

    impl Serialize for Request<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
            let mut request = serializer.serialize_struct("Request", 2)?;
            request.serialize_field("source", self.source)?;
            request.serialize_field("theme", &self.theme)?;
            request.end()
        }
    }

    serde_json::to_string(&Request { source, theme })
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pele_preview_passes_host_theme_through_the_production_request_builder() {
        let theme = json!({"foreground":"#abcdef","background":"#112233","radius_lg":16});
        let request: Value =
            serde_json::from_str(&render_request(SAMPLE, Some(&theme)).unwrap()).unwrap();
        assert_eq!(request["theme"], theme);
        assert_eq!(request["source"], SAMPLE);
    }

    #[test]
    fn borrowed_request_preserves_exact_json_order_escapes_and_null() {
        let themes = [
            Value::Null,
            json!("not an object"),
            json!({"z":"Кириллица 😀 \"\r\n\t", "a":[null, false, {"right": 0.125, "left":"<&>"}]}),
        ];
        for source in ["", SAMPLE, "flowchart TD\r\n A[\u{0000}雪🦀\"\\] --> B"] {
            for theme in std::iter::once(None).chain(themes.iter().map(Some)) {
                let old = json!({"source": source, "theme": theme}).to_string();
                assert_eq!(render_request(source, theme).unwrap(), old);
            }
        }
    }
}

fn main() {
    run(Mermaid::default());
}
