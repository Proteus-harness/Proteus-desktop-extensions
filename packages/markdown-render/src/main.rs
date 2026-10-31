//! Markdown policy and parsing live in this process; the client only renders
//! generic native HTML text and child content surfaces.
mod fences;
mod inlines;
mod references;
mod render;

use anyhow::anyhow;
use proteus_extension::{Extension, Host, Result, Surface, run};
use std::collections::HashMap;

#[derive(Default)]
struct Markdown {
    views: HashMap<String, render::Document>,
}

impl Extension for Markdown {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        if surface == Surface::Preview {
            // Render this package's own sample directly: competing language
            // registrations must never choose another renderer for the demo.
            let root = self
                .views
                .entry(view.into())
                .or_default()
                .render(SAMPLE, &[])?;
            host.show(view, root);
            return Ok(());
        }
        if surface != Surface::Content {
            return Err(anyhow!("Markdown requires a content surface"));
        }
        let content = host
            .content(view)
            .ok_or_else(|| anyhow!("content source is missing"))?;
        let languages = content
            .context
            .get("languages")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let root = self
            .views
            .entry(view.into())
            .or_default()
            .render(&content.source, &languages)?;
        host.show(view, root);
        Ok(())
    }

    fn closed(&mut self, _: &mut Host, view: &str) {
        self.views.remove(view);
    }
}

const SAMPLE: &str = "### Форматированный ответ\n\n**Жирный текст**, *курсив* и `код` в строке.\n\n- [x] Проверка готова\n- [ ] Выпустить обновление\n\n| Этап | Результат |\n| --- | --- |\n| Сборка | Готово |\n| Проверка | 24 теста |\n\n```rust\nfn main() {\n    println!(\"Proteus\");\n}\n```\n";

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn demo_uses_own_renderer_without_language_dispatch() {
        let root = render::Document::default().render(SAMPLE, &[]).unwrap();
        let encoded = root.to_string();
        assert!(encoded.contains("rich_text"));
        assert!(encoded.contains("println!"));
        assert!(!encoded.contains("\"type\":\"content\""));
    }
}

fn main() {
    run(Markdown::default());
}
