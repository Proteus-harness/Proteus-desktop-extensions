//! Markdown-rs identifies spans; only the generic inline-content contract is
//! emitted here. No TeX parsing or unsafe HTML mode is involved.
use anyhow::anyhow;
use markdown::mdast::Node;
use proteus_extension::{Result, Value, json, ui, ui::Node as _};
use std::{borrow::Cow, collections::HashSet, ops::Range};

const MAX_INLINES: usize = 256;
const MAX_SOURCE: usize = 32 * 1024;

struct Math {
    range: Range<usize>,
    source: String,
}

pub(super) fn render(id: &str, source: &str, languages: &[String]) -> Result<Value> {
    let language = ["latex", "tex"]
        .into_iter()
        .find(|language| languages.iter().any(|enabled| enabled == language));
    let mut maths = Vec::new();
    if language.is_some() && source.contains('$') {
        let mut options = markdown::ParseOptions::gfm();
        options.constructs.math_text = true;
        if let Some(guarded) = guarded_source(source)? {
            let root =
                markdown::to_mdast(&guarded, &options).map_err(|error| anyhow!("{error}"))?;
            collect(&root, source, &mut maths);
        }
    }
    if maths.is_empty() {
        return compile(id, source);
    }
    // Plain tokens are introduced only at parser-approved text positions. They
    // cannot collide with authored text, cannot become attributes (image alt
    // trees are excluded), and retain punctuation at the original boundaries.
    // The GFM compiler continues to suppress all authored raw HTML.
    let prefix = token_prefix(source);
    let mut rewritten = String::with_capacity(source.len());
    let mut end = 0;
    for (index, math) in maths.iter().enumerate() {
        rewritten.push_str(&source[end..math.range.start]);
        rewritten.push_str(&format!("{prefix}{index}:"));
        end = math.range.end;
    }
    rewritten.push_str(&source[end..]);
    let mut root = compile(id, &rewritten)?;
    let html = root["html"].as_str().unwrap();
    let mut replacements = Vec::with_capacity(maths.len());
    let mut inlines = Vec::with_capacity(maths.len());
    for (index, math) in maths.into_iter().enumerate() {
        let slot = format!("inline-{index}");
        let fallback = escape(&source[math.range]);
        replacements.push(format!("<span data-content=\"{slot}\">{fallback}</span>"));
        inlines.push(json!({"id":slot,"language":language.unwrap(),"source":math.source}));
    }
    // HTML order may differ from source order (GFM footnotes). Unreferenced
    // footnote definitions emit nothing. Scan once and retain only visible
    // slots instead of assuming all parser nodes survive in source order.
    let mut output = String::with_capacity(html.len());
    let mut cursor = 0;
    let mut visible = vec![false; replacements.len()];
    for (start, _) in html.match_indices(&prefix) {
        let index_start = start + prefix.len();
        let Some(length) = html[index_start..].find(':') else {
            continue;
        };
        let Ok(index) = html[index_start..index_start + length].parse::<usize>() else {
            continue;
        };
        let Some(replacement) = replacements.get(index) else {
            continue;
        };
        output.push_str(&html[cursor..start]);
        output.push_str(replacement);
        cursor = index_start + length + 1;
        visible[index] = true;
    }
    output.push_str(&html[cursor..]);
    root["html"] = output.into();
    let inlines = inlines
        .into_iter()
        .zip(visible)
        .filter_map(|(inline, visible)| visible.then_some(inline))
        .collect::<Vec<_>>();
    Ok(root.with("inlines", inlines))
}

// Markdown-rs permits whitespace next to math delimiters. Rejecting its first
// "$100 then $" node afterwards loses the following "$x$" opener. Protect
// nonmatching delimiters before that parse, with byte-for-byte offsets intact.
// The ordinary GFM tree supplies text positions: code, image attributes, HTML,
// link destinations and escaped dollars cannot open a native inline slot.
fn guarded_source(source: &str) -> Result<Option<Cow<'_, str>>> {
    // Escaped/currency-only input needs no buffer. The common single complete
    // formula also needs no context tree: its delimiters bound the whole input.
    let mut backslashes = 0;
    let mut count = 0;
    for byte in source.bytes() {
        if byte == b'$' && backslashes % 2 == 0 {
            count += 1;
            if count == 2 {
                break;
            }
        }
        backslashes = if byte == b'\\' { backslashes + 1 } else { 0 };
    }
    if count < 2 {
        return Ok(None);
    }
    if let Some(body) = source.strip_prefix('$').and_then(|s| s.strip_suffix('$')) {
        if !body.is_empty()
            && !body.starts_with(char::is_whitespace)
            && !body.ends_with(char::is_whitespace)
            && !body.contains(['$', '\r', '\n'])
        {
            return Ok(Some(Cow::Borrowed(source)));
        }
    }
    let root = markdown::to_mdast(source, &markdown::ParseOptions::gfm())
        .map_err(|error| anyhow!("{error}"))?;
    let mut dollars = Vec::new();
    collect_text_dollars(&root, source, &mut dollars);
    drop(root);
    let bytes = source.as_bytes();
    let mut pairs = Vec::new();
    let mut opener = None;
    for (offset, can_open, can_close) in dollars {
        if let Some(start) = opener.take() {
            if can_close && !source[start + 1..offset].contains(['\r', '\n']) {
                pairs.push(start);
                pairs.push(offset);
                continue;
            }
        }
        // A fresh opener after whitespace supersedes a currency opener. The
        // actual math syntax, nesting and final spans still belong to mdast.
        opener = can_open.then_some(offset);
    }
    if pairs.is_empty() {
        return Ok(None);
    }
    let mut rewritten = None;
    let mut accepted = pairs.into_iter().peekable();
    for (offset, byte) in bytes.iter().enumerate() {
        if *byte != b'$' {
            continue;
        }
        if accepted.peek() == Some(&offset) {
            accepted.next();
        } else {
            // '%' has the same ASCII punctuation class, no Markdown construct
            // of its own, and the same UTF-8 length. It is never user output.
            rewritten.get_or_insert_with(|| bytes.to_vec())[offset] = b'%';
        }
    }
    Ok(Some(match rewritten {
        Some(bytes) => {
            Cow::Owned(String::from_utf8(bytes).expect("ASCII substitutions keep UTF-8"))
        }
        None => Cow::Borrowed(source),
    }))
}

fn collect_text_dollars(node: &Node, source: &str, dollars: &mut Vec<(usize, bool, bool)>) {
    if matches!(node, Node::Image(_) | Node::ImageReference(_)) {
        return;
    }
    if let Node::Text(text) = node {
        if let Some(position) = &text.position {
            let start = position.start.offset;
            let raw = &source[start..position.end.offset];
            let mut backslashes = 0;
            for (offset, byte) in raw.bytes().enumerate() {
                if byte == b'$' && backslashes % 2 == 0 {
                    let before = raw[..offset].chars().next_back();
                    let after = raw[offset + 1..].chars().next();
                    let single = before != Some('$') && after != Some('$');
                    dollars.push((
                        start + offset,
                        single && after.is_some_and(|c| !c.is_whitespace()),
                        single
                            && before.is_some_and(|c| !c.is_whitespace())
                            && !after.is_some_and(|c| c.is_ascii_digit()),
                    ));
                }
                backslashes = if byte == b'\\' { backslashes + 1 } else { 0 };
            }
        }
        return;
    }
    if let Some(children) = node.children() {
        for child in children {
            collect_text_dollars(child, source, dollars);
        }
    }
}

fn token_prefix(source: &str) -> String {
    const MARKER: &str = ":PROTEUSINLINE";
    // Collect authored numeric namespaces in one source scan. Appending X's
    // until a prefix disappears would be quadratic for a long authored X run,
    // and could multiply that run into every generated inline placeholder.
    let occupied = source
        .match_indices(MARKER)
        .filter_map(|(offset, _)| {
            let tail = &source[offset + MARKER.len()..];
            let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
            (tail.as_bytes().get(digits) == Some(&b'X'))
                .then(|| tail[..digits].parse::<usize>().ok())
                .flatten()
        })
        .collect::<HashSet<_>>();
    let nonce = (0..).find(|nonce| !occupied.contains(nonce)).unwrap();
    format!("{MARKER}{nonce}X")
}

fn compile(id: &str, source: &str) -> Result<Value> {
    let html = markdown::to_html_with_options(source, &markdown::Options::gfm())
        .map_err(|error| anyhow!("{error}"))?;
    Ok(ui::rich_text(id, html))
}

fn collect(node: &Node, source: &str, maths: &mut Vec<Math>) {
    // Alt text is an attribute, not a native text run. Code has no children.
    if matches!(node, Node::Image(_) | Node::ImageReference(_)) {
        return;
    }
    if let Node::InlineMath(math) = node {
        if maths.len() >= MAX_INLINES || math.value.len() > MAX_SOURCE {
            return;
        }
        if let Some(position) = &math.position {
            let range = position.start.offset..position.end.offset;
            if let Some(raw) = source.get(range.clone()) {
                // Single-dollar inline syntax only; display blocks are routed
                // separately. Trim guards avoid swallowing "$100 and $20".
                let body = raw.strip_prefix('$').and_then(|s| s.strip_suffix('$'));
                if let Some(body) = body.filter(|body| {
                    !body.is_empty()
                        && body.len() <= MAX_SOURCE
                        && !body.starts_with('$')
                        && !body.ends_with('$')
                        && !body.starts_with(char::is_whitespace)
                        && !body.ends_with(char::is_whitespace)
                        && !body.contains(['\r', '\n'])
                        && !source[range.end..].starts_with(|c: char| c.is_ascii_digit())
                }) {
                    maths.push(Math {
                        range,
                        source: body.to_owned(),
                    });
                }
            }
        }
        return;
    }
    if let Some(children) = node.children() {
        for child in children {
            collect(child, source, maths);
        }
    }
}

fn escape(source: &str) -> String {
    source
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn math(source: &str) -> Value {
        render("prose-0", source, &["latex".into()]).unwrap()
    }

    #[test]
    fn math_inside_emphasis_lists_tables_and_unicode_preserves_surrounding_markup() {
        let root = math(
            "Энергия **$E=mc^2$** и *$α+β$*.\n\n- $x_1$\n\n| А | Б |\n|---|---|\n| $\\frac{1}{2}$ | текст |\n",
        );
        let html = root["html"].as_str().unwrap();
        assert!(
            html.contains("<strong><span data-content=\"inline-0\">$E=mc^2$</span></strong>"),
            "{html}"
        );
        assert!(
            html.contains("<em><span data-content=\"inline-1\">$α+β$</span></em>"),
            "{html}"
        );
        assert!(html.contains("<table>"));
        assert_eq!(root["inlines"].as_array().unwrap().len(), 4);
        assert_eq!(root["inlines"][3]["source"], r"\frac{1}{2}");
    }

    #[test]
    fn code_escapes_currency_unclosed_and_image_alt_are_literal() {
        for source in [
            "`$x$`",
            "```md\n$x$\n```",
            r"\$x\$",
            "$x",
            "$ x$",
            "$x $",
            "$100 and $20",
            "$1$20",
            "![image $x$](https://example.com/a.png)",
        ] {
            let root = math(source);
            assert!(root.get("inlines").is_none(), "{source}: {root}");
            assert!(!root["html"].as_str().unwrap().contains("data-content"));
        }
    }

    #[test]
    fn currency_openers_never_consume_later_formulas() {
        for (source, expected) in [
            ("Price $100 and formula $x$.", vec!["x"]),
            ("Price $100 then $x$ and $y$.", vec!["x", "y"]),
            ("Price $100, `$literal$`, then $x$.", vec!["x"]),
            (r"Price $100, \$escaped\$, then $x$.", vec!["x"]),
            ("Цена $100 и **$x$**, ещё *$y$*.", vec!["x", "y"]),
            ("Values $1$ and $100$; price $20.", vec!["1", "100"]),
        ] {
            let root = math(source);
            let actual = root["inlines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|slot| slot["source"].as_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{source}: {root}");
            assert!(root["html"].as_str().unwrap().contains("$100"));
            assert!(!root["html"].as_str().unwrap().contains("%100"));
        }
    }

    #[test]
    fn delimiter_guard_fast_paths_keep_source_borrowed_or_skip_math() {
        for source in ["", "No math", "$100", r"\$x\$"] {
            assert!(guarded_source(source).unwrap().is_none(), "{source}");
        }
        assert!(matches!(
            guarded_source("$x$").unwrap(),
            Some(Cow::Borrowed("$x$"))
        ));
    }

    #[test]
    fn disabled_provider_and_tex_registration_keep_generic_policy() {
        let fallback = render("a", "Энергия $E=mc^2$", &[]).unwrap();
        assert_eq!(fallback["html"], "<p>Энергия $E=mc^2$</p>");
        assert!(fallback.get("inlines").is_none());
        let tex = render("a", "$x$", &["tex".into()]).unwrap();
        assert_eq!(tex["inlines"][0]["language"], "tex");
    }

    #[test]
    fn authored_html_stays_disabled_and_fallback_is_escaped() {
        let root = math(
            "<script>alert(1)</script>\n\n$x<y & z$\n\n<span data-content=\"inline-0\">forged</span>",
        );
        let html = root["html"].as_str().unwrap();
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("$x&lt;y &amp; z$"), "{html}");
        assert_eq!(html.matches("<span data-content=").count(), 1, "{html}");
    }

    #[test]
    fn slots_are_bounded_local_and_collision_free() {
        let source = format!(":PROTEUSINLINE0X0: {}", "$x$ ".repeat(258));
        let root = math(&source);
        assert_eq!(root["inlines"].as_array().unwrap().len(), 256);
        assert!(
            root["html"]
                .as_str()
                .unwrap()
                .contains(":PROTEUSINLINE0X0:")
        );
        assert_eq!(
            root["html"]
                .as_str()
                .unwrap()
                .matches("data-content=")
                .count(),
            256
        );
        assert!(root["html"].as_str().unwrap().ends_with("$x$ $x$</p>"));
    }

    #[test]
    fn reordered_and_unreferenced_footnotes_never_break_visible_slots() {
        let root =
            math("[^n]: Footnote $y$\n\n[^unused]: Hidden $z$\n\nBody $x$ and footnote[^n].");
        let html = root["html"].as_str().unwrap();
        assert!(!html.contains("PROTEUSINLINE"), "{html}");
        assert_eq!(root["inlines"].as_array().unwrap().len(), 2, "{root}");
        assert!(html.contains("$x$"));
        assert!(html.contains("$y$"));
        assert!(!html.contains("$z$"));
    }

    #[test]
    fn links_and_punctuation_preserve_math_as_text_content() {
        let root = math("[_$x$_](https://example.com) and ~~$y$~~.");
        let html = root["html"].as_str().unwrap();
        assert!(
            html.contains("<a href=\"https://example.com\"><em><span"),
            "{html}"
        );
        assert!(html.contains("<del><span"), "{html}");
        assert_eq!(root["inlines"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn adversarial_authored_prefix_does_not_inflate_inline_tokens() {
        let source = format!(":PROTEUSINLINE{} $x$", "X".repeat(100_000));
        let prefix = token_prefix(&source);
        assert!(prefix.len() < 32);
        assert!(!source.contains(&prefix));
    }
}
