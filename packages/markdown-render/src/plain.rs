// Адаптировано из Zed: crates/markdown/src/parser.rs (parse_links_only).
//! Plain messages retain every authored character. Only http(s) URL spans
//! receive links; the existing parser supplies Unicode/punctuation boundaries.
use anyhow::anyhow;
use markdown::{Constructs, ParseOptions, mdast::Node};
use proteus_extension::{Result, Value, ui};
use std::ops::Range;

pub(super) fn render(id: &str, source: &str) -> Result<Value> {
    let links = links(source)?;
    let mut html = String::from("<div style=\"white-space: pre-wrap\">");
    let mut cursor = 0;
    for range in links {
        escape(&source[cursor..range.start], &mut html, "");
        html.push_str("<a href=\"");
        let escaped_start = html.len();
        escape(&source[range.clone()], &mut html, "");
        let escaped_end = html.len();
        html.push_str("\">");
        html.extend_from_within(escaped_start..escaped_end);
        html.push_str("</a>");
        cursor = range.end;
    }
    escape(&source[cursor..], &mut html, "</div>");
    Ok(ui::rich_text(id, html))
}

fn links(source: &str) -> Result<Vec<Range<usize>>> {
    // Only a literal lowercase HTTP(S) prefix can pass collect's source check.
    if !source.match_indices("://").any(|(end, _)| {
        let prefix = &source[..end];
        prefix.ends_with("http") || prefix.ends_with("https")
    }) {
        return Ok(Vec::new());
    }
    // No emphasis, escapes, code, HTML, images or authored Markdown links are
    // interpreted. Autolink literal is the only enabled construct.
    let constructs = Constructs {
        attention: false,
        autolink: false,
        block_quote: false,
        character_escape: false,
        character_reference: false,
        code_indented: false,
        code_fenced: false,
        code_text: false,
        definition: false,
        frontmatter: false,
        gfm_autolink_literal: true,
        gfm_footnote_definition: false,
        gfm_label_start_footnote: false,
        gfm_strikethrough: false,
        gfm_table: false,
        gfm_task_list_item: false,
        hard_break_escape: false,
        hard_break_trailing: false,
        heading_atx: false,
        heading_setext: false,
        html_flow: false,
        html_text: false,
        label_start_image: false,
        label_start_link: false,
        label_end: false,
        list_item: false,
        math_flow: false,
        math_text: false,
        mdx_esm: false,
        mdx_expression_flow: false,
        mdx_expression_text: false,
        mdx_jsx_flow: false,
        mdx_jsx_text: false,
        thematic_break: false,
    };
    let root = markdown::to_mdast(
        source,
        &ParseOptions {
            constructs,
            ..Default::default()
        },
    )
    .map_err(|error| anyhow!("{error}"))?;
    let mut links = Vec::new();
    collect(&root, source, &mut links);
    Ok(links)
}

fn collect(node: &Node, source: &str, links: &mut Vec<Range<usize>>) {
    if let Node::Link(link) = node
        && let Some(position) = &link.position
    {
        let range = position.start.offset..position.end.offset;
        if source
            .get(range.clone())
            .is_some_and(|text| text.starts_with("https://") || text.starts_with("http://"))
        {
            links.push(range);
        }
        return;
    }
    if let Some(children) = node.children() {
        for child in children {
            collect(child, source, links);
        }
    }
}

fn escape(source: &str, output: &mut String, ending: &str) {
    let mut start = 0;
    for (index, byte) in source.bytes().enumerate() {
        let replacement = match byte {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            _ => continue,
        };
        // Matched ASCII bytes cannot be inside a UTF-8 code point.
        if start < index {
            output.push_str(&source[start..index]);
        }
        output.push_str(replacement);
        start = index + 1;
    }
    let tail = &source[start..];
    if !ending.is_empty() {
        // Reserve the final literal bytes and closing tag together. A large
        // ordinary tail can then move into rich_text without a growth or copy
        // just to append the footer.
        output.reserve_exact(tail.len() + ending.len());
    }
    output.push_str(tail);
    output.push_str(ending);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_message_only_marks_urls_and_keeps_markdown_html_and_whitespace_literal() {
        let source = "  **жирный** `код` <b>&amp;</b>\n\n[текст](https://example.com/a?q=1&b=2)\nhttps://example.org/path).  ";
        let ranges = links(source).unwrap();
        assert_eq!(
            ranges
                .iter()
                .map(|range| &source[range.clone()])
                .collect::<Vec<_>>(),
            ["https://example.com/a?q=1&b=2", "https://example.org/path"]
        );
        let node = render("plain", source).unwrap();
        let html = node["html"].as_str().unwrap();
        assert!(html.starts_with("<div style=\"white-space: pre-wrap\">  **жирный** `код` &lt;b&gt;&amp;amp;&lt;/b&gt;\n\n[текст](<a href="));
        assert!(html.ends_with("</a>).  </div>"));
        assert!(!html.contains("<strong>"));
        assert!(!html.contains("<code>"));
        assert!(!html.contains("<img"));
    }

    #[test]
    fn plaintext_links_do_not_create_email_javascript_or_image_actions() {
        let source =
            "a@example.com www.example.com javascript:alert(1) ![x](http://example.com/a.png)";
        assert_eq!(
            links(source)
                .unwrap()
                .iter()
                .map(|range| &source[range.clone()])
                .collect::<Vec<_>>(),
            ["http://example.com/a.png"]
        );
        assert_eq!(
            render("plain", "<script>alert(1)</script>").unwrap()["html"],
            "<div style=\"white-space: pre-wrap\">&lt;script&gt;alert(1)&lt;/script&gt;</div>"
        );
    }
}
