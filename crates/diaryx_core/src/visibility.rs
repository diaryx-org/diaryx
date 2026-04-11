//! Audience visibility filtering for inline and block markdown directives.
//!
//! Supported syntax:
//! - Inline: `:vis[text]{audience1 audience2}`
//! - Block:
//!   ```text
//!   :::vis{audience1 audience2}
//!   content
//!   :::
//!   ```
//!
//! These helpers operate purely on markdown text. They do not parse frontmatter
//! or render markdown to HTML.

/// Fast-path check for visibility directives.
pub fn has_visibility_directives(body: &str) -> bool {
    body.contains(":vis[") || body.contains(":::vis{")
}

/// Filter a markdown body to content visible to `target_audience`.
///
/// Equivalent to [`filter_body_for_audiences`] with a single-element slice.
pub fn filter_body_for_audience(body: &str, target_audience: &str) -> String {
    filter_body_for_audiences(body, &[target_audience])
}

/// Filter a markdown body to content visible to any of `target_audiences`.
///
/// A `:vis[...]` or `:::vis{...}` directive is included when its attribute list
/// shares at least one audience tag with `target_audiences` (case-insensitive,
/// whitespace-trimmed). An empty `target_audiences` slice strips every directive.
pub fn filter_body_for_audiences(body: &str, target_audiences: &[&str]) -> String {
    let (filtered, _, _) = filter_segment(body, 0, Some(target_audiences), false);
    filtered
}

/// Remove visibility directive markers while preserving their inner content.
pub fn strip_visibility_directives(body: &str) -> String {
    let (filtered, _, _) = filter_segment(body, 0, None, false);
    filtered
}

fn filter_segment(
    input: &str,
    mut index: usize,
    target_audiences: Option<&[&str]>,
    stop_at_block_close: bool,
) -> (String, usize, bool) {
    let mut out = String::new();

    while index < input.len() {
        if is_line_start(input, index) {
            if stop_at_block_close && let Some(next_index) = try_parse_vis_block_close(input, index)
            {
                return (out, next_index, true);
            }

            if let Some((attrs, content_start)) = try_parse_vis_block_open(input, index) {
                let (inner, next_index, closed) =
                    filter_segment(input, content_start, target_audiences, true);
                if closed {
                    if should_include_visibility_content(attrs, target_audiences) {
                        out.push_str(&inner);
                    }
                    index = next_index;
                    continue;
                }
            }
        }

        if let Some((content, attrs, next_index)) = try_parse_vis_inline(input, index) {
            if should_include_visibility_content(attrs, target_audiences) {
                let (inner, _, _) = filter_segment(content, 0, target_audiences, false);
                out.push_str(&inner);
            }
            index = next_index;
            continue;
        }

        let ch = input[index..]
            .chars()
            .next()
            .expect("index should always point at a char boundary");
        out.push(ch);
        index += ch.len_utf8();
    }

    (out, index, false)
}

fn is_line_start(input: &str, index: usize) -> bool {
    index == 0 || input.as_bytes().get(index.saturating_sub(1)) == Some(&b'\n')
}

fn try_parse_vis_inline(input: &str, index: usize) -> Option<(&str, &str, usize)> {
    const PREFIX: &str = ":vis[";
    if !input[index..].starts_with(PREFIX) {
        return None;
    }

    let mut cursor = index + PREFIX.len();
    let mut depth = 1usize;
    let bytes = input.as_bytes();
    let mut content_end = None;

    while cursor < input.len() {
        match bytes[cursor] {
            b'\\' => {
                cursor = (cursor + 2).min(input.len());
            }
            b'[' => {
                depth += 1;
                cursor += 1;
            }
            b']' => {
                depth -= 1;
                if depth == 0 {
                    content_end = Some(cursor);
                    cursor += 1;
                    break;
                }
                cursor += 1;
            }
            _ => cursor += 1,
        }
    }

    let content_end = content_end?;
    if bytes.get(cursor) != Some(&b'{') {
        return None;
    }

    let attrs_start = cursor + 1;
    let attrs_end = input[attrs_start..].find('}')? + attrs_start;

    Some((
        &input[index + PREFIX.len()..content_end],
        &input[attrs_start..attrs_end],
        attrs_end + 1,
    ))
}

fn try_parse_vis_block_open(input: &str, index: usize) -> Option<(&str, usize)> {
    const PREFIX: &str = ":::vis{";
    if !input[index..].starts_with(PREFIX) {
        return None;
    }

    let attrs_start = index + PREFIX.len();
    let attrs_end = input[attrs_start..].find('}')? + attrs_start;
    let mut next_index = attrs_end + 1;

    while let Some(byte) = input.as_bytes().get(next_index) {
        if *byte == b' ' || *byte == b'\t' {
            next_index += 1;
        } else {
            break;
        }
    }

    if input.as_bytes().get(next_index) == Some(&b'\r') {
        next_index += 1;
    }
    if input.as_bytes().get(next_index) == Some(&b'\n') {
        next_index += 1;
    }

    Some((&input[attrs_start..attrs_end], next_index))
}

fn try_parse_vis_block_close(input: &str, index: usize) -> Option<usize> {
    if !input[index..].starts_with(":::") {
        return None;
    }

    if let Some(next) = input[index + 3..].chars().next()
        && (next == '_' || next.is_alphanumeric())
    {
        return None;
    }

    let mut next_index = index + 3;
    while let Some(byte) = input.as_bytes().get(next_index) {
        if *byte == b' ' || *byte == b'\t' {
            next_index += 1;
        } else {
            break;
        }
    }

    if input.as_bytes().get(next_index) == Some(&b'\r') {
        next_index += 1;
    }
    if input.as_bytes().get(next_index) == Some(&b'\n') {
        next_index += 1;
    }

    Some(next_index)
}

fn should_include_visibility_content(attrs: &str, target_audiences: Option<&[&str]>) -> bool {
    match target_audiences {
        Some(targets) => attrs.split_whitespace().any(|audience| {
            targets
                .iter()
                .any(|target| audience.eq_ignore_ascii_case(target.trim()))
        }),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_visibility_directives() {
        assert!(has_visibility_directives(":vis[hello]{public}"));
        assert!(has_visibility_directives(":::vis{public}\nhello\n:::\n"));
        assert!(!has_visibility_directives("hello"));
    }

    #[test]
    fn filters_inline_directive() {
        let body = "Before :vis[hello]{public} after";
        assert_eq!(
            filter_body_for_audience(body, "public"),
            "Before hello after"
        );
        assert_eq!(filter_body_for_audience(body, "friends"), "Before  after");
    }

    #[test]
    fn filters_block_directive() {
        let body = "Intro\n:::vis{public}\nhello\n:::\nOutro";
        assert_eq!(
            filter_body_for_audience(body, "public"),
            "Intro\nhello\nOutro"
        );
        assert_eq!(filter_body_for_audience(body, "friends"), "Intro\nOutro");
    }

    #[test]
    fn strips_directive_markers_without_filtering() {
        let body = "Before :vis[hello]{public} after";
        assert_eq!(strip_visibility_directives(body), "Before hello after");
    }

    #[test]
    fn supports_nested_directives() {
        let body = "A :vis[outer :vis[inner]{public} end]{public}";
        assert_eq!(
            filter_body_for_audience(body, "public"),
            "A outer inner end"
        );
    }

    #[test]
    fn multi_audience_includes_when_any_member_matches() {
        let body = "Before :vis[hello]{family} after";
        assert_eq!(
            filter_body_for_audiences(body, &["public", "family"]),
            "Before hello after"
        );
        assert_eq!(
            filter_body_for_audiences(body, &["public", "internal"]),
            "Before  after"
        );
    }

    #[test]
    fn multi_audience_block_directive() {
        let body = "Intro\n:::vis{family friends}\nhello\n:::\nOutro";
        assert_eq!(
            filter_body_for_audiences(body, &["friends"]),
            "Intro\nhello\nOutro"
        );
        assert_eq!(
            filter_body_for_audiences(body, &["public", "family"]),
            "Intro\nhello\nOutro"
        );
        assert_eq!(filter_body_for_audiences(body, &["public"]), "Intro\nOutro");
    }

    #[test]
    fn empty_audience_slice_strips_all_directives() {
        let body = "Before :vis[hello]{public} after";
        assert_eq!(filter_body_for_audiences(body, &[]), "Before  after");
    }
}
