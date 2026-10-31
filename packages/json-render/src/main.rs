//! The package interprets completed fenced sources; the host only mounts
//! its public native content surface and keeps the original source intact.

mod catalog;

use anyhow::anyhow;
use proteus_extension::{Extension, Host, Result, Surface, run};

#[derive(Default)]
struct Visualizations;

impl Extension for Visualizations {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        if surface == Surface::Preview {
            host.show(view, catalog::render("json-render", SAMPLE)?);
            return Ok(());
        }
        if surface != Surface::Content {
            return Err(anyhow!("visualizations require a content surface"));
        }
        let content = host
            .content(view)
            .ok_or_else(|| anyhow!("content source is missing"))?;
        let root = catalog::render(&content.language, &content.source)?;
        host.show(view, root);
        Ok(())
    }
}

const SAMPLE: &str = r#"{"type":"stack","gap":"medium","children":[{"type":"metric","label":"Выполнено","value":"24","detail":"Синтетический пример"},{"type":"bar_chart","unit":"задач","bars":[{"label":"Пн","value":6},{"label":"Вт","value":10},{"label":"Ср","value":8}]},{"type":"table","columns":["Этап","Состояние"],"rows":[["Проверка","Готово"],["Выпуск","Ожидает"]]}]}"#;

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn demo_uses_the_production_catalog() {
        let root = catalog::render("json-render", SAMPLE).unwrap();
        assert_eq!(root["children"][1]["type"], "bar_chart");
        assert_eq!(root["children"][2]["rows"][0][1], "Готово");
    }
}

fn main() {
    run(Visualizations);
}
