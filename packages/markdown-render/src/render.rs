//! GFM policy, retained segment conversions and closed-fence routing.
use super::fences;
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
        // A single prose segment already retains the complete input. Reuse
        // that exact source and the full language list, not a hash or another
        // copy of the document. Fixed parser/HTML options add no dynamic inputs.
        if self.languages == languages
            && let [segment] = self.segments.as_slice()
            && segment.source.as_deref() == Some(source)
        {
            #[cfg(test)]
            {
                self.parsed_bytes = 0;
            }
            return Ok(self.root());
        }
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
        let policy_changed = self.languages != languages;
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
                    piece.node = prose(&piece.id, source, &definitions, languages)?;
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
                    definitions_changed || policy_changed,
                    languages,
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
                definitions_changed || policy_changed,
                languages,
            )?);
        }
        self.segments = pieces;
        self.languages = languages.to_vec();
        self.definitions = definitions;
        Ok(self.root())
    }

    fn root(&self) -> Value {
        let children = self
            .segments
            .iter()
            // Keep retained segments (and streaming prefix indices) intact;
            // invisible prose must not introduce another stack gap.
            .filter(|segment| {
                !(segment.node["type"] == "rich_text"
                    && segment.node["html"]
                        .as_str()
                        .is_some_and(|html| html.trim().is_empty()))
            })
            .map(|segment| segment.node.clone())
            .collect::<Vec<_>>();
        json!({"type":"stack", "direction":"column", "gap":"medium", "children":children})
    }
}

fn segment(
    old: &mut Vec<Segment>,
    id: String,
    source: &str,
    definitions: &[String],
    definitions_changed: bool,
    languages: &[String],
) -> Result<Segment> {
    if !definitions_changed
        && let Some(index) = old
            .iter()
            .position(|segment| segment.id == id && segment.source.as_deref() == Some(source))
    {
        return Ok(old.remove(index));
    }
    let node = prose(&id, source, definitions, languages)?;
    Ok(Segment {
        id,
        source: Some(source.into()),
        node,
    })
}

fn prose(id: &str, source: &str, definitions: &[String], languages: &[String]) -> Result<Value> {
    // Definitions emit no HTML. Prepending the document's source-ordered
    // table also preserves first-definition-wins across separate segments.
    // Ordinary prose incurs no extra buffer or parse work for this table.
    let resolved = (!definitions.is_empty() && source.contains('[')).then(|| {
        let mut resolved = definitions.join("\n");
        resolved.push_str("\n\n");
        resolved.push_str(source);
        resolved
    });
    super::inlines::render(id, resolved.as_deref().unwrap_or(source), languages)
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
    fn unchanged_single_prose_reuses_exact_source_without_parsing() {
        let languages = vec!["mermaid".into(), "latex".into(), "md".into()];
        for source in [
            "",
            "# Привет 🦀\r\n\r\n**жирный**, ~~старый~~ и `x < 2`\r\n",
            "- [x] готово\n- [ ] осталось\n\n| А | Б |\n| - | - |\n| 1 | 2 |",
            "[api]: https://example.com/first \"API\"\n\n[docs][api]\n\n[api]: https://example.com/second",
            "> [nested]: https://example.com/nested\n\n[nested]\n\n> ```mermaid\n> А-->Б\n> ```",
            "$$\n\\frac{1}{",
            "$$\nx^2\n$$ invalid",
            "> $$\n> x^2\n> $$",
            "The price is $5 and `$$` stays literal.",
        ] {
            let mut document = Document::default();
            let first = document.render(source, &languages).unwrap();
            assert_eq!(document.parsed_bytes, source.len());
            assert_eq!(document.segments.len(), 1);
            assert_eq!(document.segments[0].source.as_deref(), Some(source));
            document.parsed_bytes = usize::MAX;
            assert_eq!(document.render(source, &languages).unwrap(), first);
            assert_eq!(document.parsed_bytes, 0);
        }
    }

    #[test]
    fn unchanged_inline_math_reuses_slots_and_provider_changes_revalidate() {
        let source = "Price $100 then **$x$** and $y$. Literal `$z$`, \\$a\\$ and emoji 🟢.\n\n| Formula |\n| --- |\n| $E=mc^2$ |";
        let enabled = vec!["latex".into(), "mermaid".into()];
        let disabled = vec!["mermaid".into()];
        let mut document = Document::default();
        let first = document.render(source, &enabled).unwrap();
        assert_eq!(first["children"][0]["inlines"].as_array().unwrap().len(), 3);
        assert_eq!(first["children"][0]["inlines"][0]["source"], "x");
        assert_eq!(first["children"][0]["inlines"][1]["source"], "y");
        let retained_source = document.segments[0].source.as_ref().unwrap().as_ptr();
        for _ in 0..3 {
            assert_eq!(document.render(source, &enabled).unwrap(), first);
            assert_eq!(document.parsed_bytes, 0);
            assert_eq!(
                document.segments[0].source.as_ref().unwrap().as_ptr(),
                retained_source
            );
        }
        let without_provider = document.render(source, &disabled).unwrap();
        assert_eq!(document.parsed_bytes, source.len());
        assert_eq!(
            without_provider,
            Document::default().render(source, &disabled).unwrap()
        );
        assert!(without_provider["children"][0]["inlines"].is_null());
        assert!(all_html(&without_provider).contains("$E=mc^2$"));
        assert_eq!(
            document.render(source, &disabled).unwrap(),
            without_provider
        );
        assert_eq!(document.parsed_bytes, 0);
        assert_eq!(document.render(source, &enabled).unwrap(), first);
        assert_eq!(document.parsed_bytes, source.len());
        let edited = source.replace("$y$", "$y_1$");
        let result = document.render(&edited, &enabled).unwrap();
        assert_eq!(document.parsed_bytes, edited.len());
        assert_eq!(
            result,
            Document::default().render(&edited, &enabled).unwrap()
        );
        assert_eq!(result["children"][0]["inlines"][1]["source"], "y_1");
    }

    #[test]
    fn prose_edits_and_ordered_language_changes_still_parse() {
        let source = "Ссылка [docs][api].\n\n[api]: https://example.com/first";
        let mut document = Document::default();
        for languages in [
            vec!["mermaid".into(), "json".into()],
            vec!["json".into(), "mermaid".into()],
            vec!["json".into()],
            vec![],
        ] {
            let result = document.render(source, &languages).unwrap();
            assert_eq!(document.parsed_bytes, source.len());
            assert_eq!(
                result,
                Document::default().render(source, &languages).unwrap()
            );
            assert_eq!(document.render(source, &languages).unwrap(), result);
            assert_eq!(document.parsed_bytes, 0);
        }
        let edited = source.replace("example.com/first", "example.com/second");
        let result = document.render(&edited, &[]).unwrap();
        assert_eq!(document.parsed_bytes, edited.len());
        assert_eq!(result, Document::default().render(&edited, &[]).unwrap());
        assert!(all_html(&result).contains("example.com/second"));
        assert!(!all_html(&result).contains("example.com/first"));
    }

    #[test]
    fn root_code_and_streaming_transitions_do_not_hit_prose_reuse() {
        let languages = vec!["mermaid".into()];
        let mut document = Document::default();
        for source in [
            "До",
            "До\n\n```mermaid\nА-->Б",
            "До\n\n```mermaid\nА-->Б\n```",
            "До\n\n```mermaid\nА-->Б\n``` больше не закрытие",
            "```mermaid\nА-->Б\n```",
            "    indented code",
            "Снова только абзац",
        ] {
            let result = document.render(source, &languages).unwrap();
            assert_eq!(document.parsed_bytes, source.len());
            assert_eq!(
                result,
                Document::default().render(source, &languages).unwrap()
            );
            let prose_only = document.segments.len() == 1
                && document.segments[0].source.as_deref() == Some(source);
            assert_eq!(document.render(source, &languages).unwrap(), result);
            assert_eq!(
                document.parsed_bytes,
                if prose_only { 0 } else { source.len() }
            );
        }
    }

    #[test]
    fn math_and_literal_markdown_transitions_revalidate_after_prose_noop() {
        let languages = vec![
            "mermaid".into(),
            "latex".into(),
            "tex".into(),
            "md".into(),
            "markdown".into(),
        ];
        let mut document = Document::default();
        for (source, has_content) in [
            ("Before", false),
            ("$$\nx^2", false),
            ("$$\nx^2\n$$", true),
            ("$$\nx^2\n$$ invalid", false),
            ("```md\n# Literal heading\n```", false),
            (
                "````markdown\n```mermaid\nА-->Б\n```\n\n$$\nx^2\n$$\n````",
                false,
            ),
            ("After\n\n- [x] semantic checkbox", false),
        ] {
            // These closing fences end at EOF: no immutable line prefix can
            // hide a false no-op when a closer or a literal example changes.
            let root = document.render(source, &languages).unwrap();
            assert_eq!(document.parsed_bytes, source.len());
            assert_eq!(
                root,
                Document::default().render(source, &languages).unwrap()
            );
            assert_eq!(
                root["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|node| node["type"] == "content"),
                has_content,
            );
            let prose_only = document.segments.len() == 1
                && document.segments[0].source.as_deref() == Some(source);
            document.parsed_bytes = usize::MAX;
            assert_eq!(document.render(source, &languages).unwrap(), root);
            assert_eq!(
                document.parsed_bytes,
                if prose_only { 0 } else { source.len() }
            );
        }
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
    fn task_status_survives_as_semantic_native_checkboxes() {
        let output = html("- [x] готово\n- [ ] осталось");
        assert!(
            output.contains("<input type=\"checkbox\" disabled=\"\" checked=\"\" /> готово"),
            "{output}"
        );
        assert!(
            output.contains("<input type=\"checkbox\" disabled=\"\" /> осталось"),
            "{output}"
        );
        assert!(!output.contains('☑') && !output.contains('☐'));
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
    fn adjacent_fences_have_one_gap_without_discarding_streaming_segments() {
        let languages = vec!["mermaid".into(), "md".into()];
        for separator in ["\n", "\n\n", "\r\n\r\n", "\n \t\n\n"] {
            let first = "```rust\nfn main() {}\n```\n";
            let second = format!("{first}{separator}```md\n# Literal\n```\n");
            let third = format!("{second}{separator}```mermaid\nA-->B\n```\n");
            let mut document = Document::default();
            let initial = document.render(first, &languages).unwrap();
            for (source, count) in [
                (first.to_owned(), 1),
                (format!("{first}{separator}"), 1),
                (second, 2),
                (third.clone(), 3),
                (format!("{third}\nХвост"), 4),
            ] {
                let result = document.render(&source, &languages).unwrap();
                assert_eq!(result["gap"], "medium");
                let children = result["children"].as_array().unwrap();
                assert_eq!(children.len(), count, "{source:?}: {result}");
                assert_eq!(children[0], initial["children"][0]);
                assert_eq!(
                    result,
                    Document::default().render(&source, &languages).unwrap()
                );
            }
            // Empty prose still contributes to retained-prefix indices, even
            // though only the three fences and visible tail reach the UI.
            assert!(document.prefix.segments > 3);
            assert!(document.segments.len() > 4);
            assert!(document.parsed_bytes < 32);
        }
    }

    #[test]
    fn empty_prose_emits_no_gap_child_but_remains_available_for_exact_reuse() {
        for source in ["", " \t\n\r\n", "[api]: https://example.com\n"] {
            let mut document = Document::default();
            let first = document.render(source, &[]).unwrap();
            assert_eq!(first["gap"], "medium");
            assert_eq!(first["children"], json!([]));
            assert_eq!(document.segments.len(), 1);
            assert_eq!(document.segments[0].source.as_deref(), Some(source));
            assert_eq!(document.render(source, &[]).unwrap(), first);
            assert_eq!(document.parsed_bytes, 0);
        }
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

    #[test]
    fn conventional_display_math_routes_exact_cyrillic_source_to_registered_renderer() {
        let formula = "\\text{Настроение} =\n\\frac{\\text{кофе} + \\text{интересные задачи}}\n{1 + \\text{количество багов}}\n";
        let source = format!("### Формула настроения\n\n$$\n{formula}$$\n\nПосле");
        for language in ["latex", "tex"] {
            let root = Document::default()
                .render(&source, &[language.into()])
                .unwrap();
            assert_eq!(root["children"][1]["type"], "content");
            assert_eq!(root["children"][1]["language"], language);
            assert_eq!(root["children"][1]["source"], formula);
            assert!(
                root["children"][0]["html"]
                    .as_str()
                    .unwrap()
                    .contains("Формула настроения")
            );
            assert_eq!(root["children"][2]["html"], "<p>После</p>");
        }
        let fallback = Document::default().render(&source, &[]).unwrap();
        assert_eq!(fallback["children"][1]["type"], "code");
        assert_eq!(fallback["children"][1]["text"], formula);
        assert_eq!(fallback["children"][1]["embedded"], true);
    }

    #[test]
    fn incomplete_math_and_dollars_in_code_are_not_visualizations() {
        let languages = vec!["latex".into()];
        for source in [
            "$$\n\\frac{1}{",
            "$$\nx\n$$ invalid",
            "$$\nx\n$",
            "The price is $5 and `$$` is literal.",
            "> $$\n> x\n> $$",
            "```text\n$$\nx\n$$\n```",
            "    $$\n    x\n    $$",
        ] {
            let root = Document::default().render(source, &languages).unwrap();
            assert!(
                root["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|node| node["type"] != "content"),
                "{source}: {root}"
            );
        }
        let root = Document::default()
            .render("$$\n\\frac{1}{", &languages)
            .unwrap();
        assert!(root["children"][0]["html"].as_str().unwrap().contains("$$"));
    }

    #[test]
    fn completed_math_reuses_prefix_and_revalidates_eof_closer() {
        let languages = vec!["latex".into()];
        let mut document = Document::default();
        let source = "$$$\r\n\\text{кофе} + 1\r\n$$$\r\n\r\nХвост";
        let before = document.render(source, &languages).unwrap();
        assert_eq!(before["children"][0]["source"], "\\text{кофе} + 1\r\n");
        let appended = format!("{source} дальше");
        let after = document.render(&appended, &languages).unwrap();
        assert_eq!(before["children"][0], after["children"][0]);
        assert!(document.parsed_bytes < appended.len());
        assert_eq!(
            after,
            Document::default().render(&appended, &languages).unwrap()
        );
        let eof = "$$\nx\n$$";
        assert_eq!(
            document.render(eof, &languages).unwrap()["children"][0]["type"],
            "content"
        );
        let changed = format!("{eof} invalid");
        assert!(
            document.render(&changed, &languages).unwrap()["children"]
                .as_array()
                .unwrap()
                .iter()
                .all(|node| node["type"] != "content")
        );
    }

    #[test]
    fn fenced_markdown_stays_literal_through_streamed_prose_and_definition_updates() {
        let table = "| Игрок | Монеты |\n|---|---|\n| Аня | 100 |\n| Боб | 50 |\n";
        let languages = vec!["md".into(), "markdown".into()];
        let source = format!("Таблица\n\n```md\n{table}```\n");
        let mut document = Document::default();
        let first = document.render(&source, &languages).unwrap();
        assert!(!all_html(&first).contains("<table>"));
        let child = &first["children"][1];
        assert_eq!(child["type"], "code");
        assert_eq!(child["language"], "md");
        assert_eq!(child["text"], table);
        for suffix in [
            "Хвост",
            "Хвост дописан\n\nДалее",
            "Хвост [docs][api]\n\n[api]: https://example.com",
        ] {
            let streamed = format!("{source}\n{suffix}");
            let updated = document.render(&streamed, &languages).unwrap();
            assert_eq!(updated["children"][1], *child);
            assert!(!all_html(&updated).contains("<table>"));
            assert_eq!(
                updated["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|node| node["type"] == "code")
                    .count(),
                1
            );
            assert_eq!(
                updated,
                Document::default().render(&streamed, &languages).unwrap()
            );
        }
    }

    #[test]
    fn separate_authored_markdown_examples_are_preserved_without_heuristic_deduplication() {
        let table = "| Игрок | Монеты |\n|---|---|\n| Аня | 100 |\n| Боб | 50 |\n";
        let languages = vec!["md".into()];
        let source = format!("```md\n{table}```\n\n{table}");
        let root = Document::default().render(&source, &languages).unwrap();
        assert_eq!(root["children"][0]["type"], "code");
        assert_eq!(root["children"][0]["text"], table);
        assert_eq!(all_html(&root).matches("<table>").count(), 1);
        let repeated = Document::default()
            .render(&format!("{table}\n{table}"), &languages)
            .unwrap();
        assert_eq!(all_html(&repeated).matches("<table>").count(), 2);
    }

    #[test]
    fn outer_markdown_example_never_starts_its_inner_renderers() {
        let languages = vec![
            "md".into(),
            "markdown".into(),
            "mermaid".into(),
            "latex".into(),
        ];
        for language in ["md", "markdown", "MD"] {
            let body = "```mermaid\nA-->B\n```\n\n$$\nx^2\n$$\n";
            let source = format!("````{language}\n{body}````\n");
            let root = Document::default().render(&source, &languages).unwrap();
            let children = root["children"].as_array().unwrap();
            assert_eq!(children[0]["type"], "code");
            assert_eq!(children[0]["text"], body);
            assert!(children.iter().all(|node| node["type"] != "content"));
        }
        // Bare documents and other fenced visualization languages still render.
        let source = "| A | B |\n|---|---|\n| 1 | 2 |\n\n```mermaid\nA-->B\n```\n";
        let root = Document::default().render(source, &languages).unwrap();
        assert_eq!(all_html(&root).matches("<table>").count(), 1);
        assert_eq!(root["children"][1]["type"], "content");
        assert_eq!(root["children"][1]["language"], "mermaid");
    }

    #[test]
    fn inline_math_streaming_retains_finished_prose_and_invalidates_provider_policy() {
        let languages = vec!["latex".into()];
        let mut document = Document::default();
        let before = "Энергия $E=mc^2$.\n\n```rust\nfn main() {}\n```\n\nХвост $x";
        let first = document.render(before, &languages).unwrap();
        assert_eq!(first["children"][0]["inlines"][0]["source"], "E=mc^2");
        assert!(first["children"][2].get("inlines").is_none());
        let after = format!("{before}^2$.");
        let second = document.render(&after, &languages).unwrap();
        assert_eq!(first["children"][0], second["children"][0]);
        assert_eq!(first["children"][1], second["children"][1]);
        assert_eq!(second["children"][2]["inlines"][0]["source"], "x^2");
        assert!(document.parsed_bytes < 32);
        assert_eq!(
            second,
            Document::default().render(&after, &languages).unwrap()
        );
        let disabled = document.render(&after, &[]).unwrap();
        assert!(disabled["children"][0].get("inlines").is_none());
        assert!(disabled["children"][2].get("inlines").is_none());
        assert!(
            disabled["children"][0]["html"]
                .as_str()
                .unwrap()
                .contains("$E=mc^2$")
        );
    }
}
