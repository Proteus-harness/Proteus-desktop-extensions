//! GFM policy, retained segment conversions and closed-fence routing.
use super::fences;
use anyhow::anyhow;
use proteus_extension::{Result, Value, json, ui};

#[derive(Default)]
pub(super) struct Document {
    segments: Vec<Segment>,
    prefix: Prefix,
    languages: Vec<String>,
    definitions: Vec<String>,
    #[cfg(test)]
    parsed_bytes: usize,
}

#[derive(Default)]
struct Prefix {
    bytes: usize,
    hash: u64,
    segments: usize,
    blocks: usize,
    definitions: usize,
}
struct Segment {
    id: String,
    source: Option<String>,
    node: Value,
}

impl Document {
    pub fn render(&mut self, source: &str, languages: &[String]) -> Result<Value> {
        use std::hash::{Hash, Hasher};
        fn fingerprint(source: &str) -> u64 {
            let mut hash = std::hash::DefaultHasher::new();
            source.hash(&mut hash);
            hash.finish()
        }
        let mut old = std::mem::take(&mut self.segments);
        let reuse = self.prefix.bytes > 0
            && self.languages == languages
            && source
                .get(..self.prefix.bytes)
                .is_some_and(|prefix| fingerprint(prefix) == self.prefix.hash);
        let (boundary, ordinal) = if reuse {
            (self.prefix.bytes, self.prefix.blocks)
        } else {
            (0, 0)
        };
        let parsed = fences::parse(&source[boundary..], languages);
        let prefix_definitions = if reuse { self.prefix.definitions } else { 0 };
        let mut definitions = self.definitions[..prefix_definitions].to_vec();
        definitions.extend(
            parsed
                .definitions
                .iter()
                .map(|definition| definition.source.clone()),
        );
        let definitions_changed = self.definitions != definitions;
        let mut pieces = if reuse {
            old.drain(..self.prefix.segments).collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if definitions_changed {
            for piece in &mut pieces {
                if let Some(source) = piece
                    .source
                    .as_deref()
                    .filter(|source| source.contains('['))
                {
                    piece.node = prose(&piece.id, source, &definitions)?;
                }
            }
        }
        if !reuse {
            self.prefix = Prefix::default();
        }
        let mut start = boundary;
        let mut blocks = ordinal;
        // A closing fence is immutable only once its line has ended. Parse
        // just the tail on streamed appends; an edit or provider change
        // invalidates the prefix and runs a full document parse.
        #[cfg(test)]
        {
            self.parsed_bytes = source.len() - boundary;
        }
        let mut definitions_before = 0;
        for block in parsed.blocks {
            let end = boundary + block.range.end;
            let begin = boundary + block.range.start;
            let index = ordinal + block.ordinal;
            if start < begin {
                pieces.push(segment(
                    &mut old,
                    format!("prose-{index}"),
                    &source[start..begin],
                    &definitions,
                    definitions_changed,
                )?);
            }
            let id = format!("code-{index}");
            let node = if block.claimed {
                ui::content(&id, &block.language, block.code)
            } else {
                json!({"type":"code", "id":id, "text":block.code, "language":block.language, "embedded":true})
            };
            pieces.push(Segment {
                id,
                source: None,
                node,
            });
            start = end;
            blocks = index + 1;
            if block.closed && matches!(source.as_bytes().get(end), Some(b'\r' | b'\n')) {
                while parsed
                    .definitions
                    .get(definitions_before)
                    .is_some_and(|definition| definition.end <= block.range.end)
                {
                    definitions_before += 1;
                }
                self.prefix = Prefix {
                    bytes: end,
                    hash: fingerprint(&source[..end]),
                    segments: pieces.len(),
                    blocks,
                    definitions: prefix_definitions + definitions_before,
                };
            }
        }
        if start < source.len() || pieces.is_empty() {
            pieces.push(segment(
                &mut old,
                format!("prose-{blocks}"),
                &source[start..],
                &definitions,
                definitions_changed,
            )?);
        }
        let children = pieces
            .iter()
            .map(|segment| segment.node.clone())
            .collect::<Vec<_>>();
        self.segments = pieces;
        self.languages = languages.to_vec();
        self.definitions = definitions;
        Ok(json!({"type":"stack", "direction":"column", "gap":"none", "children":children}))
    }
}

fn segment(
    old: &mut Vec<Segment>,
    id: String,
    source: &str,
    definitions: &[String],
    definitions_changed: bool,
) -> Result<Segment> {
    if (!definitions_changed || !source.contains('['))
        && let Some(index) = old
            .iter()
            .position(|segment| segment.id == id && segment.source.as_deref() == Some(source))
    {
        return Ok(old.remove(index));
    }
    let node = prose(&id, source, definitions)?;
    Ok(Segment {
        id,
        source: Some(source.into()),
        node,
    })
}

fn prose(id: &str, source: &str, definitions: &[String]) -> Result<Value> {
    // Definitions emit no HTML. Prepending the document's source-ordered
    // table also preserves first-definition-wins across separate segments.
    // Ordinary prose incurs no extra buffer or parse work for this table.
    let resolved = (!definitions.is_empty() && source.contains('[')).then(|| {
        let mut resolved = definitions.join("\n");
        resolved.push_str("\n\n");
        resolved.push_str(source);
        resolved
    });
    let html = markdown::to_html_with_options(
        resolved.as_deref().unwrap_or(source),
        &markdown::Options::gfm(),
    )
    .map_err(|error| anyhow!("{error}"))?
    .replace(
        "<input type=\"checkbox\" disabled=\"\" checked=\"\" /> ",
        "☑ ",
    )
    .replace("<input type=\"checkbox\" disabled=\"\" /> ", "☐ ");
    Ok(ui::rich_text(id, html))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn render(source: &str) -> Value {
        Document::default()
            .render(source, &["mermaid".into()])
            .unwrap()
    }
    fn html(source: &str) -> String {
        render(source)["children"][0]["html"]
            .as_str()
            .unwrap()
            .into()
    }

    #[test]
    fn gfm_keeps_cyrillic_heading_styles_lists_table_and_links() {
        let output = html(
            "# Привет 🦀\n\n**жирный** и *курсив*, ~~старый~~, `x < 2`\n\n- один\n- два\n\n| А | Б |\n| - | - |\n| 1 | 2 |\n\n[ссылка](https://example.com)",
        );
        for fragment in [
            "<h1>Привет 🦀</h1>",
            "<strong>жирный</strong>",
            "<em>курсив</em>",
            "<del>старый</del>",
            "<code>x &lt; 2</code>",
            "<ul>",
            "<table>",
            "href=\"https://example.com\"",
        ] {
            assert!(output.contains(fragment), "{fragment}: {output}");
        }
    }

    #[test]
    fn task_status_survives_native_html_without_input_controls() {
        let output = html("- [x] готово\n- [ ] осталось");
        assert!(output.contains("☑ готово"), "{output}");
        assert!(output.contains("☐ осталось"), "{output}");
        assert!(!output.contains("<input"));
    }

    #[test]
    fn registered_closed_fences_route_exact_unicode_source_and_unknown_code_stays_native() {
        let output = render(
            "До\n\n```mermaid\nflowchart LR\n А-->Б\n```\n\n```rust\nlet x = \"<>&\";\n```\n\n```mermaid\nunfinished",
        );
        let children = output["children"].as_array().unwrap();
        let visual = children
            .iter()
            .find(|node| node["type"] == "content")
            .unwrap();
        assert_eq!(visual["source"], "flowchart LR\n А-->Б\n");
        let codes = children
            .iter()
            .filter(|node| node["type"] == "code")
            .collect::<Vec<_>>();
        assert_eq!(codes.len(), 2);
        assert_eq!(codes[0]["text"], "let x = \"<>&\";\n");
        assert_eq!(codes[1]["text"], "unfinished");
    }

    #[test]
    fn nested_or_invalid_closing_fences_are_not_routed() {
        for source in [
            "> ```mermaid\n> x\n> ```",
            "- item\n\n  ```mermaid\n  x\n  ```",
            "```mermaid\nx\n``` invalid",
        ] {
            assert!(
                render(source)["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|node| node["type"] != "content")
            );
        }
    }

    #[test]
    fn append_keeps_finished_child_identity_and_updates_only_tail() {
        let mut document = Document::default();
        let before = "```mermaid\nflowchart LR\n A-->B\n```\n\nначало";
        let first = document.render(before, &["mermaid".into()]).unwrap();
        let second = document
            .render(&format!("{before} продолжение"), &["mermaid".into()])
            .unwrap();
        assert_eq!(first["children"][0], second["children"][0]);
        assert_eq!(first["children"][1]["id"], second["children"][1]["id"]);
        assert!(
            second["children"][1]["html"]
                .as_str()
                .unwrap()
                .contains("начало продолжение")
        );
        let disabled = document.render(before, &[]).unwrap();
        assert_eq!(disabled["children"][0]["type"], "code");
    }

    #[test]
    fn large_finished_source_is_not_reparsed_during_tail_streaming() {
        let languages = vec!["mermaid".into()];
        let source = format!("```mermaid\n{}\n```\n\nХвост", " А-->Б\n".repeat(10_000));
        let mut document = Document::default();
        document.render(&source, &languages).unwrap();
        let next = format!("{source} продолжается\n\n```rust\nfn main() {{}}\n```\n");
        let incremental = document.render(&next, &languages).unwrap();
        assert!(
            document.parsed_bytes < 128,
            "parsed {} bytes",
            document.parsed_bytes
        );
        assert_eq!(
            incremental,
            Document::default().render(&next, &languages).unwrap()
        );
        let edited = next.replace("А-->Б", "А-->В");
        assert_eq!(
            document.render(&edited, &languages).unwrap(),
            Document::default().render(&edited, &languages).unwrap()
        );
        assert_eq!(document.parsed_bytes, edited.len());
    }

    fn all_html(root: &Value) -> String {
        root["children"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|node| node["html"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn references_before_and_after_fences_share_the_whole_document_scope() {
        let fence = "\n\n```rust\nfn main() {}\n```\n\n";
        for source in [
            format!("Ссылка [docs][api].{fence}[api]: https://example.com/api"),
            format!("[api]: https://example.com/api{fence}Ссылка [docs][api]."),
        ] {
            let result = render(&source);
            let html = all_html(&result);
            assert!(
                html.contains("href=\"https://example.com/api\">docs</a>"),
                "{html}"
            );
            assert!(!html.contains("[api]:"), "{html}");
            assert_eq!(
                result["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|node| node["type"] == "code")
                    .count(),
                1
            );
        }
    }

    #[test]
    fn streamed_definitions_invalidate_reference_prose_without_reparsing_finished_code() {
        let mut document = Document::default();
        let source = format!(
            "Ссылка [docs][api].\n\n```rust\n{}\n```\n\n",
            "fn main() {}\n".repeat(10_000)
        );
        let initial = document.render(&source, &[]).unwrap();
        assert!(!all_html(&initial).contains("href="));
        let defined = format!("{source}[api]: https://example.com/first \"API\"\n");
        let first = document.render(&defined, &[]).unwrap();
        assert!(all_html(&first).contains("href=\"https://example.com/first\" title=\"API\""));
        assert_eq!(initial["children"][1], first["children"][1]);
        assert!(document.parsed_bytes < 128);
        let edited = defined.replace("example.com/first", "example.com/second");
        let second = document.render(&edited, &[]).unwrap();
        assert!(all_html(&second).contains("href=\"https://example.com/second\""));
        assert!(!all_html(&second).contains("example.com/first"));
        assert!(document.parsed_bytes < 128);
        assert_eq!(second, Document::default().render(&edited, &[]).unwrap());
        // A second finished block advances the prefix past the definition.
        // Later references must still use its retained document-wide table.
        let finished = format!("{edited}\n```rust\nlet tail = 1;\n```\n\n");
        document.render(&finished, &[]).unwrap();
        let last = format!("{finished}Ещё [docs][api].");
        let result = document.render(&last, &[]).unwrap();
        assert_eq!(
            all_html(&result)
                .matches("href=\"https://example.com/second\"")
                .count(),
            2
        );
        assert!(document.parsed_bytes < 128);
        assert_eq!(result, Document::default().render(&last, &[]).unwrap());
    }

    #[test]
    fn references_preserve_first_definition_titles_and_container_definitions() {
        let source = "[api]: https://example.com/first?x=1&y=2 \"A & B\"\n\n```rust\n// [api]: https://invalid.example\n```\n\n[docs][api]\n\n[api]: https://example.com/second\n\n> [nested]: https://example.com/nested\n\n[nested]";
        let result = all_html(&render(source));
        assert!(
            result.contains("href=\"https://example.com/first?x=1&amp;y=2\" title=\"A &amp; B\""),
            "{result}"
        );
        assert!(!result.contains("example.com/second"), "{result}");
        assert!(
            result.contains("href=\"https://example.com/nested\""),
            "{result}"
        );
    }

    #[test]
    fn closing_at_eof_is_revalidated_when_append_changes_the_line() {
        let languages = vec!["mermaid".into()];
        let mut document = Document::default();
        let before = "```mermaid\nА-->Б\n```";
        assert_eq!(
            document.render(before, &languages).unwrap()["children"][0]["type"],
            "content"
        );
        let after = format!("{before} больше не закрытие");
        assert_eq!(
            document.render(&after, &languages).unwrap()["children"][0]["type"],
            "code"
        );
    }

    #[test]
    fn longer_fence_and_crlf_preserve_inner_source() {
        let output = render("~~~~mermaid\r\n```inner\r\nА\r\n```\r\n~~~~\r\n");
        assert_eq!(output["children"][0]["source"], "```inner\r\nА\r\n```\r\n");
    }
}
