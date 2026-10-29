// Адаптировано из Zed: crates/agent_ui/src/conversation_view/thread_view.rs
// (replace only completed top-level registered fences).
//! Exact source offsets and stable ordinal identity of root code blocks.
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
    let Ok(root) = markdown::to_mdast(text, &markdown::ParseOptions::gfm()) else {
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
            markdown::mdast::Node::Code(code) => Some(code),
            _ => None,
        })
        .enumerate()
        .filter_map(|(ordinal, code)| {
            let position = code.position?;
            let range = position.start.offset..position.end.offset;
            let raw = text.get(range.clone())?;
            let language = code.lang.unwrap_or_default().to_ascii_lowercase();
            let closed = closed_source(raw);
            let claimed = closed.is_some() && languages.iter().any(|name| *name == language);
            Some(Fence {
                ordinal,
                range,
                language,
                closed: closed.is_some(),
                code: closed
                    .map(|source| raw[source].to_owned())
                    .unwrap_or(code.value),
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
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let length = opening.bytes().take_while(|byte| *byte == marker).count();
    if length < 3 {
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
