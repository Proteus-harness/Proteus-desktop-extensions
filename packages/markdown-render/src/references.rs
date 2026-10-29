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
            let mut source = format!(
                "[{}]: <{}>",
                definition.identifier,
                escaped(&definition.url)
            );
            if let Some(title) = &definition.title {
                source.push_str(&format!(" \"{}\"", escaped(title)));
            }
            definitions.push(Definition {
                end: position.end.offset,
                source,
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

fn escaped(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
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
    result
}
