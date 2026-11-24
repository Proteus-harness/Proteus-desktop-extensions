//! Reference definitions share document scope even across native code nodes.
//! Re-emit semantic destinations safely; container prefixes and multiline
//! titles must not leak into a standalone prose segment.
use markdown::mdast::Node;

pub(super) struct Definition {
    pub end: usize,
    pub source: String,
}

pub(super) fn collect(root: &Node) -> Vec<Definition> {
    fn visit(node: &Node, definitions: &mut Vec<Definition>) {
        if let Node::Definition(definition) = node {
            let Some(position) = &definition.position else {
                return;
            };
            // identifier retains Markdown escapes and entities, unlike label.
            // Its normalized whitespace/case still matches every reference.
            definitions.push(Definition {
                end: position.end.offset,
                source: definition_source(
                    &definition.identifier,
                    &definition.url,
                    definition.title.as_deref(),
                ),
            });
        }
        if let Some(children) = node.children() {
            for child in children {
                visit(child, definitions);
            }
        }
    }
    let mut definitions = Vec::new();
    visit(root, &mut definitions);
    definitions
}

fn definition_source(identifier: &str, url: &str, title: Option<&str>) -> String {
    let title_bytes = title.map_or(0, |title| title.len() + 3);
    let mut source = String::with_capacity(6 + identifier.len() + url.len() + title_bytes);
    source.push('[');
    source.push_str(identifier);
    source.push_str("]: <");
    escape_into(url, &mut source);
    source.push('>');
    if let Some(title) = title {
        source.push_str(" \"");
        escape_into(title, &mut source);
        source.push('"');
    }
    source
}

fn escape_into(value: &str, result: &mut String) {
    for character in value.chars() {
        match character {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\\' => result.push_str("&#92;"),
            '\n' => result.push_str("&#10;"),
            '\r' => result.push_str("&#13;"),
            '\t' => result.push_str("&#9;"),
            _ => result.push(character),
        }
    }
}
