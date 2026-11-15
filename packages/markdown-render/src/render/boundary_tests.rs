use super::{Document, Value};

fn compare_after_edit(before: &str, after: &str, languages: &[String]) -> (Value, usize, usize) {
    let mut document = Document::default();
    document.render(before, languages).unwrap();
    let boundary = document.prefix.bytes;
    assert!(boundary > 0, "initial source must cache a closed line");
    let warm = document.render(after, languages).unwrap();
    let cold = Document::default().render(after, languages).unwrap();
    assert_eq!(warm, cold, "before={before:?}, after={after:?}");
    (warm, document.parsed_bytes, boundary)
}

#[test]
fn code_fence_line_edits_revalidate_claimed_unclaimed_and_literal_blocks() {
    let languages = vec!["mermaid".into(), "md".into()];
    for (opening, closing) in [("```mermaid", "```"), ("~~~rust", "~~~"), ("```md", "```")] {
        for ending in ["\n", "\r\n"] {
            let prefix = format!("{opening}{ending}А-->Б{ending}{closing}");
            let before = format!("{prefix}{ending}Tail");
            // Whitespace is still a valid closer, but must conservatively
            // reparse rather than reuse a boundary whose next byte changed.
            for inserted in [" invalid", " 🦀", "\t"] {
                let after = format!("{prefix}{inserted}{ending}Tail");
                let (_, parsed, _) = compare_after_edit(&before, &after, &languages);
                assert_eq!(parsed, after.len());
            }
        }
    }
}

#[test]
fn math_closing_line_edits_use_the_same_boundary_rule() {
    let languages = vec!["latex".into()];
    for marker in ["$$", "$$$"] {
        for ending in ["\n", "\r\n"] {
            let prefix = format!("{marker}{ending}x^2{ending}{marker}");
            let before = format!("{prefix}{ending}Tail");
            for inserted in [" invalid", "\t"] {
                let after = format!("{prefix}{inserted}{ending}Tail");
                let (_, parsed, _) = compare_after_edit(&before, &after, &languages);
                assert_eq!(parsed, after.len());
            }
        }
    }
}

#[test]
fn deleting_the_terminator_revalidates_and_restoring_it_reenables_prefix_reuse() {
    let languages = vec!["mermaid".into(), "latex".into()];
    for (opening, closing) in [("```mermaid", "```"), ("$$", "$$")] {
        for ending in ["\n", "\r\n"] {
            let eof = format!("{opening}\nA\n{closing}");
            let terminated = format!("{eof}{ending}");
            let mut document = Document::default();
            document.render(&terminated, &languages).unwrap();
            assert!(document.prefix.bytes > 0);

            let at_eof = document.render(&eof, &languages).unwrap();
            assert_eq!(
                at_eof,
                Document::default().render(&eof, &languages).unwrap()
            );
            assert_eq!(document.parsed_bytes, eof.len());
            assert_eq!(document.prefix.bytes, 0);

            let restored = document.render(&terminated, &languages).unwrap();
            assert_eq!(
                restored,
                Document::default().render(&terminated, &languages).unwrap()
            );
            assert_eq!(document.parsed_bytes, terminated.len());
            let boundary = document.prefix.bytes;
            assert!(boundary > 0);

            let appended = format!("{terminated}Tail 🦀");
            let warm = document.render(&appended, &languages).unwrap();
            assert_eq!(
                warm,
                Document::default().render(&appended, &languages).unwrap()
            );
            assert_eq!(document.parsed_bytes, appended.len() - boundary);
        }
    }
}

#[test]
fn retained_line_endings_keep_incremental_reuse_including_crlf_edits() {
    let languages = vec!["mermaid".into(), "latex".into()];
    for (opening, closing) in [("```mermaid", "```"), ("$$", "$$")] {
        let prefix = format!("{opening}\nA\n{closing}");
        for (old_tail, new_tail) in [
            ("\n", "\nTail"),
            ("\r\n", "\r\nTail"),
            ("\n", "\r\nTail"),
            ("\r\n", "\nTail"),
            // Removing/replacing only LF retains CR as a line terminator.
            // The newly parsed suffix, not the guard, determines its content.
            ("\r\n", "\rTail\n"),
        ] {
            let before = format!("{prefix}{old_tail}");
            let after = format!("{prefix}{new_tail}");
            let (_, parsed, boundary) = compare_after_edit(&before, &after, &languages);
            assert_eq!(parsed, after.len() - boundary);
        }
    }
}
