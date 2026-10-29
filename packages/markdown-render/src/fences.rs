// Адаптировано из Zed: crates/agent_ui/src/conversation_view/thread_view.rs
// (replace only completed top-level registered fences).
//! Exact source offsets and stable ordinal identity of root code/math blocks.
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Fence {
    pub ordinal: usize,
    pub range: Range<usize>,
    pub language: String,
    pub code: String,
    pub claimed: bool,
    pub closed: bool,
}

#[derive(Default)]
pub(super) struct Parsed {
    pub blocks: Vec<Fence>,
    pub definitions: Vec<super::references::Definition>,
}

pub(super) fn parse(text: &str, languages: &[String]) -> Parsed {
    let mut options = markdown::ParseOptions::gfm();
    // The existing parser recognizes display math and protects literal dollars
    // inside code. Inline math stays ordinary GFM until it has a native surface.
    options.constructs.math_flow = true;
    let Ok(root) = markdown::to_mdast(text, &options) else {
        return Parsed::default();
    };
    let definitions = super::references::collect(&root);
    let markdown::mdast::Node::Root(root) = root else {
        return Parsed::default();
    };
    let blocks = root
        .children
        .into_iter()
        .filter_map(|node| match node {
            markdown::mdast::Node::Code(code) => Some((
                code.position,
                code.lang.unwrap_or_default(),
                code.value,
                false,
            )),
            markdown::mdast::Node::Math(math) => {
                Some((math.position, "latex".into(), math.value, true))
            }
            _ => None,
        })
        .enumerate()
        .filter_map(|(ordinal, (position, language, value, math))| {
            let position = position?;
            let range = position.start.offset..position.end.offset;
            let raw = text.get(range.clone())?;
            let language = language.to_ascii_lowercase();
            let closed = closed_source(raw);
            // markdown-rs accepts EOF as a math terminator. During streaming
            // keep that unfinished source in prose, including its opening $$.
            if math && closed.is_none() {
                return None;
            }
            let language = if math
                && !languages.iter().any(|name| name == "latex")
                && languages.iter().any(|name| name == "tex")
            {
                "tex".into()
            } else {
                language
            };
            // Markdown fences are source examples inside a Markdown document,
            // even when this process is registered for md/markdown content.
            // Rendering them recursively turns a tutorial's "write this"
            // into a second copy of the separately authored rendered result.
            let claimed = closed.is_some()
                && !matches!(language.as_str(), "md" | "markdown")
                && languages.iter().any(|name| *name == language);
            Some(Fence {
                ordinal,
                range,
                language,
                closed: closed.is_some(),
                code: closed.map(|source| raw[source].to_owned()).unwrap_or(value),
                claimed,
            })
        })
        .collect();
    Parsed {
        blocks,
        definitions,
    }
}

fn closed_source(raw: &str) -> Option<Range<usize>> {
    let first_end = raw.find('\n')? + 1;
    let opening = raw[..first_end].trim_start_matches(' ');
    let marker = opening.as_bytes().first().copied()?;
    if !matches!(marker, b'`' | b'~' | b'$') {
        return None;
    }
    let length = opening.bytes().take_while(|byte| *byte == marker).count();
    if length < if marker == b'$' { 2 } else { 3 } {
        return None;
    }
    let end = raw.trim_end_matches(['\r', '\n']);
    let closing_start = end.rfind('\n')? + 1;
    if closing_start < first_end {
        return None;
    }
    let closing = end[closing_start..].trim_start_matches(' ');
    let count = closing.bytes().take_while(|byte| *byte == marker).count();
    if count < length || !closing[count..].trim().is_empty() {
        return None;
    }
    Some(first_end..closing_start)
}
