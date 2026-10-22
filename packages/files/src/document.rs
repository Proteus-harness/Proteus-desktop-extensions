//! A workspace file or its diff in a document tab.

use proteus_extension::ui::{Node, button, code, fill, muted, row, segments, text};
use proteus_extension::{Host, Result, Value, ValueExt, json};

use crate::{Files, tree};

impl Files {
    pub fn document(&mut self, host: &Host, path: &str) -> Value {
        let mode = self
            .modes
            .get(path)
            .cloned()
            .unwrap_or_else(|| "file".to_owned());
        let body = self
            .body(host, path, &mode)
            .unwrap_or_else(|error| text(error).with("tone", "danger"));
        let choices = [
            (json!([path, "file"]), "Файл".to_owned()),
            (json!([path, "diff"]), "Изменения".to_owned()),
        ];
        let header = row(vec![
            text(path),
            segments("document-mode", &choices, &json!([path, mode])),
            button("", "copy")
                .with("value", path)
                .with("variant", "ghost")
                .with("icon", "copy")
                .with("hint", "Копировать путь"),
        ]);
        fill(vec![header, body])
    }

    fn body(&mut self, host: &Host, path: &str, mode: &str) -> Result<Value> {
        let diff = mode == "diff";
        let key = (path.to_owned(), mode.to_owned());
        if !self.documents.contains_key(&key) {
            let document = host.api(
                if diff {
                    "/workspace/diff"
                } else {
                    "/workspace/file"
                },
                &[("path", path)],
            )?;
            self.documents.insert(key.clone(), document);
        }
        let document = &self.documents[&key];
        Ok(
            match document.opt_str(if diff { "patch" } else { "text" }) {
                Some(content) => code(
                    "content",
                    content,
                    if diff { "diff" } else { tree::language(path) },
                ),
                None => muted(match document.str("kind") {
                    "binary" => "Двоичный файл",
                    "too_large" => "Файл превышает предел предпросмотра 512 КБ",
                    "unavailable" => "Изменений нет",
                    _ => "Нет текстового содержимого",
                }),
            },
        )
    }
}
