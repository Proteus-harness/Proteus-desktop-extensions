// Адаптировано из Zed: crates/agent_ui/src/conversation_view/thread_view.rs
// (replace only completed top-level registered fences).
//! Exact source offsets and stable ordinal identity of root code/math blocks.
use std::{borrow::Cow, ops::Range};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Fence<'a> {
    pub ordinal: usize,
    pub range: Range<usize>,
    pub language: String,
    pub code: Cow<'a, str>,
    pub claimed: bool,
    pub closed: bool,
}

#[derive(Default)]
pub(super) struct Parsed<'a> {
    pub blocks: Vec<Fence<'a>>,
    pub definitions: Vec<super::references::Definition>,
}

pub(super) fn parse<'a>(text: &'a str, languages: &[String]) -> Parsed<'a> {
    // This tree only supplies code/display-math blocks and reference definitions.
    // Skip it only if none can occur; ordinary GFM HTML is still parsed normally.
    if !["```", "~~~", "$$", "    ", "\t", "]:"]
        .iter()
        .any(|marker| text.contains(marker))
    {
        return Parsed::default();
    }
    let mut options = markdown::ParseOptions::gfm();
    // Block routing uses display math; prose handles inline math independently.
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
                // A closed block uses exact input bytes, not the normalized
                // parser body. Borrow them until the view needs an owned node.
                code: match closed {
                    Some(source) => Cow::Borrowed(&raw[source]),
                    None => Cow::Owned(value),
                },
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
    let first_break = raw.find(['\r', '\n'])?;
    let first_end = first_break
        + if raw[first_break..].starts_with("\r\n") {
            2
        } else {
            1
        };
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
    let closing_start = end.rfind(['\r', '\n'])? + 1;
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

#[cfg(test)]
mod line_ending_tests {
    use super::{closed_source, parse};

    #[test]
    fn registered_fences_keep_parser_offsets_and_original_payload() {
        for separator in ["\n", "\r\n", "\r"] {
            let source = format!("```mermaid{separator}α --> β{separator}```");
            let document = parse(&source, &["mermaid".into()]);
            assert_eq!(document.blocks.len(), 1);
            let fence = &document.blocks[0];
            assert!(fence.claimed && fence.closed);
            assert_eq!(fence.code, format!("α --> β{separator}"));
            assert_eq!(&source[fence.range.clone()], source);
        }
    }

    #[test]
    fn completed_fences_preserve_payload_across_line_endings() {
        for first in ["\n", "\r\n", "\r"] {
            for last in ["\n", "\r\n", "\r"] {
                for (open, close) in [("```mermaid", "```"), ("~~~json", "~~~"), ("$$", "$$")] {
                    let source = format!("{open}{first}α + β{last}{close}");
                    let range = closed_source(&source).expect("completed fence");
                    assert_eq!(&source[range], format!("α + β{last}"));
                }
            }
        }
    }

    #[test]
    fn incomplete_or_invalid_closers_remain_unclaimed() {
        for source in [
            "```mermaid",
            "```mermaid\rα",
            "```mermaid\rα\r``",
            "```mermaid\rα\r```oops",
        ] {
            assert_eq!(closed_source(source), None);
        }
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;

    #[test]
    fn retains_indented_code_both_fence_markers_math_and_nested_definitions() {
        let languages = ["mermaid".to_string(), "latex".to_string()];
        for eol in ["\n", "\r", "\r\n"] {
            for indent in ["    ", "\t", "  \t"] {
                let input = format!("before{eol}{eol}{indent}indented");
                let result = parse(&input, &languages);
                assert_eq!(result.blocks.len(), 1, "{input:?}");
                assert_eq!(result.blocks[0].code, "indented");
            }
            for (open, close, language) in [
                ("```mermaid", "```", "mermaid"),
                ("~~~mermaid", "~~~", "mermaid"),
                ("$$", "$$", "latex"),
            ] {
                let input = format!("   {open}{eol}α{eol}{close}{eol}");
                let result = parse(&input, &languages);
                assert_eq!(result.blocks.len(), 1);
                assert_eq!(result.blocks[0].language, language);
                assert!(result.blocks[0].claimed);
            }
            let input = format!("> [late{eol}> label]: https://example.org/x");
            assert_eq!(parse(&input, &languages).definitions.len(), 1);
        }
    }
}
