use thiserror::Error;

/// Explicit replacement declaration in a Markdown body. Code fences and
/// indented code do not declare relationships; ordinary wikilinks never do.
pub fn superseded_by(body: &str) -> Result<Option<String>, SupersessionError> {
    let mut target = None;
    let mut fence: Option<(u8, usize)> = None;
    let mut html_comment = false;
    for line in body.lines() {
        let spaces = line.bytes().take_while(|byte| *byte == b' ').count();
        if !html_comment && (spaces > 3 || line.starts_with('\t')) {
            continue;
        }
        let line = line.trim();
        let byte = line.as_bytes().first().copied().unwrap_or_default();
        let count = line.bytes().take_while(|value| *value == byte).count();
        if let Some((opening, width)) = fence {
            if byte == opening && count >= width && line[count..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if !html_comment
            && matches!(byte, b'`' | b'~')
            && count >= 3
            && (byte != b'`' || !line[count..].contains('`'))
        {
            fence = Some((byte, count));
            continue;
        }
        let visible = without_html_comments(line, &mut html_comment);
        let line = visible.trim();
        let Some(value) = line.strip_prefix("**Superseded by:**") else {
            continue;
        };
        let slug = value
            .trim()
            .strip_prefix("[[")
            .and_then(|v| v.strip_suffix("]]"))
            .filter(|slug| valid_replacement_slug(slug))
            .ok_or(SupersessionError::Malformed)?;
        if target.replace(slug.to_owned()).is_some() {
            return Err(SupersessionError::Duplicate);
        }
    }
    Ok(target)
}

// Ignore HTML comment text, while preserving paired inline code spans so an
// example such as `<!--` cannot hide later declarations. This parser otherwise
// recognizes physical declaration lines, not the complete Markdown grammar.
fn without_html_comments(line: &str, in_comment: &mut bool) -> String {
    let mut rest = line;
    let mut visible = String::new();
    while !rest.is_empty() {
        if *in_comment {
            let Some(end) = rest.find("-->") else {
                break;
            };
            rest = &rest[end + 3..];
            *in_comment = false;
        } else if let Some(after) = rest.strip_prefix("<!--") {
            // An unterminated mid-prose opener is literal text, not an HTML
            // block. Inline comments are recognized when closed on this line.
            if visible.trim().is_empty() || after.contains("-->") {
                *in_comment = true;
            } else {
                visible.push_str("<!--");
            }
            rest = after;
        } else if rest.starts_with('`') {
            let width = rest.bytes().take_while(|byte| *byte == b'`').count();
            let marker = &rest[..width];
            let after = &rest[width..];
            let close = after
                .match_indices(marker)
                .find(|(index, _)| {
                    (*index == 0 || after.as_bytes()[index - 1] != b'`')
                        && after.as_bytes().get(index + width) != Some(&b'`')
                })
                .map(|(index, _)| width + index + width);
            let end = close.unwrap_or(width);
            visible.push_str(&rest[..end]);
            rest = &rest[end..];
        } else {
            let ch = rest.chars().next().expect("rest is nonempty");
            visible.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    visible
}

pub fn valid_replacement_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 64
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SupersessionError {
    #[error("expected **Superseded by:** [[replacement-slug]] on its own line")]
    Malformed,
    #[error("a page may have only one Superseded by declaration")]
    Duplicate,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_inline_backticks_do_not_create_or_hide_relationships() {
        let body = "<!--\n**Superseded by:** [[commented]]\n    -->\n```inline```\nAn inline example `<!--` is not a comment.\n**Superseded by:** [[actual]]\n";
        assert_eq!(superseded_by(body), Ok(Some("actual".into())));
        assert_eq!(
            superseded_by("Prose mentions <!-- without closing.\n\n**Superseded by:** [[actual]]"),
            Ok(Some("actual".into()))
        );
        assert_eq!(
            superseded_by("<!-- **Superseded by:** [[hidden]] -->"),
            Ok(None)
        );
    }

    #[test]
    fn declarations_are_explicit_and_skip_code_examples() {
        let body = "[[ordinary-link]]\n`**Superseded by:** [[inline]]`\n    **Superseded by:** [[indent]]\n```md\n**Superseded by:** [[fenced]]\n```\n~~~\n**Superseded by:** [[tilde]]\n~~~\n**Superseded by:** [[corrected-fact]]\n";
        assert_eq!(superseded_by(body), Ok(Some("corrected-fact".into())));
        assert_eq!(superseded_by("[[new-fact]]"), Ok(None));
        assert!(superseded_by("**Superseded by:** [[../escape]]").is_err());
        assert!(superseded_by("**Superseded by:** [[one]]\n**Superseded by:** [[two]]").is_err());
    }
}
