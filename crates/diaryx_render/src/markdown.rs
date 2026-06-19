//! Markdown → HTML conversion and custom-syntax preprocessing.
//!
//! Two stages run in order:
//! 1. [`preprocess_custom_syntax`] rewrites Diaryx-specific syntax (highlights,
//!    spoilers, HTML embeds) into raw HTML, skipping fenced/inline code.
//! 2. [`markdown_to_html`] runs comrak over the preprocessed markdown with
//!    `unsafe` rendering on, so the injected raw HTML passes through.

/// Convert preprocessed markdown to HTML via comrak.
///
/// Call [`preprocess_custom_syntax`] first to expand Diaryx custom syntax.
pub fn markdown_to_html(preprocessed_markdown: &str) -> String {
    use comrak::{Options, markdown_to_html as comrak_to_html};

    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.render.r#unsafe = true; // Allow raw HTML

    comrak_to_html(preprocessed_markdown, &options)
}

/// Pre-process custom markdown syntax (highlights, spoilers, HTML embeds) into
/// raw HTML before passing to comrak. Skips fenced code blocks and inline code.
pub fn preprocess_custom_syntax(markdown: &str) -> String {
    let bytes = markdown.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;

    while i < len {
        // Skip fenced code blocks (``` ... ```)
        if i + 2 < len && bytes[i] == b'`' && bytes[i + 1] == b'`' && bytes[i + 2] == b'`' {
            let fence_start = i;
            i += 3;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            loop {
                if i >= len {
                    out.push_str(&markdown[fence_start..]);
                    return out;
                }
                if bytes[i] == b'\n'
                    && i + 3 < len
                    && bytes[i + 1] == b'`'
                    && bytes[i + 2] == b'`'
                    && bytes[i + 3] == b'`'
                {
                    i += 4;
                    while i < len && bytes[i] != b'\n' {
                        i += 1;
                    }
                    break;
                }
                i += 1;
            }
            out.push_str(&markdown[fence_start..i]);
            continue;
        }

        // Skip inline code (` ... `)
        if bytes[i] == b'`' {
            let start = i;
            i += 1;
            while i < len && bytes[i] != b'`' {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            out.push_str(&markdown[start..i]);
            continue;
        }

        // Try HTML embed: ![alt](path.html) or ![alt](path.htm)
        if bytes[i] == b'!' && i + 1 < len && bytes[i + 1] == b'[' {
            if let Some((html, consumed)) = try_parse_html_embed(&markdown[i..]) {
                out.push_str(&html);
                i += consumed;
                continue;
            }
        }

        // Try highlight: ==text== or =={color}text==
        if i + 1 < len && bytes[i] == b'=' && bytes[i + 1] == b'=' {
            if let Some((html, consumed)) = try_parse_highlight(&markdown[i..]) {
                out.push_str(&html);
                i += consumed;
                continue;
            }
        }

        // Try spoiler: ||text||
        if i + 1 < len && bytes[i] == b'|' && bytes[i + 1] == b'|' {
            if let Some((html, consumed)) = try_parse_spoiler(&markdown[i..]) {
                out.push_str(&html);
                i += consumed;
                continue;
            }
        }

        out.push(markdown[i..].chars().next().unwrap());
        i += markdown[i..].chars().next().unwrap().len_utf8();
    }

    out
}

/// Try to parse a highlight starting at `==`. Returns `(html, bytes_consumed)`.
fn try_parse_highlight(s: &str) -> Option<(String, usize)> {
    const VALID_COLORS: &[&str] = &[
        "red", "orange", "yellow", "green", "cyan", "blue", "violet", "pink", "brown", "grey",
    ];

    if !s.starts_with("==") {
        return None;
    }

    let after_open = &s[2..];
    if after_open.is_empty() || after_open.starts_with("==") {
        return None;
    }

    let (color, content_start) = if after_open.starts_with('{') {
        let close_brace = after_open.find('}')?;
        let color_name = &after_open[1..close_brace];
        if !VALID_COLORS.contains(&color_name) {
            return None;
        }
        (color_name, close_brace + 1)
    } else {
        ("yellow", 0)
    };

    let content_region = &after_open[content_start..];
    let close_pos = content_region.find("==")?;
    if close_pos == 0 {
        return None;
    }

    let content = &content_region[..close_pos];
    if content.contains('\n') {
        return None;
    }

    let total_consumed = 2 + content_start + close_pos + 2;
    let html = format!(
        r#"<mark data-highlight-color="{color}" class="highlight-mark highlight-{color}">{content}</mark>"#,
        color = color,
        content = html_escape(content),
    );

    Some((html, total_consumed))
}

/// Try to parse a spoiler starting at `||`. Returns `(html, bytes_consumed)`.
fn try_parse_spoiler(s: &str) -> Option<(String, usize)> {
    if !s.starts_with("||") {
        return None;
    }

    let after_open = &s[2..];
    if after_open.is_empty() || after_open.starts_with("||") {
        return None;
    }

    let close_pos = after_open.find("||")?;
    if close_pos == 0 {
        return None;
    }

    let content = &after_open[..close_pos];
    if content.contains('|') || content.contains('\n') {
        return None;
    }

    let total_consumed = 2 + close_pos + 2;
    let html = format!(
        r#"<span data-spoiler="" class="spoiler-mark spoiler-hidden">{content}</span>"#,
        content = html_escape(content),
    );

    Some((html, total_consumed))
}

/// Try to parse an HTML embed starting at `![`. Returns `(html, bytes_consumed)`.
///
/// Matches `![alt](path.html)` or `![alt](path.htm)` and converts to a
/// sandboxed `<iframe>` tag. This runs before Comrak so the raw HTML is
/// passed through unchanged (with `unsafe = true`).
fn try_parse_html_embed(s: &str) -> Option<(String, usize)> {
    if !s.starts_with("![") {
        return None;
    }

    let after_bang = &s[2..];
    let close_bracket = after_bang.find(']')?;
    let alt = &after_bang[..close_bracket];

    let after_bracket = &after_bang[close_bracket + 1..];
    if !after_bracket.starts_with('(') {
        return None;
    }

    let after_paren = &after_bracket[1..];
    let close_paren = after_paren.find(')')?;
    let path = after_paren[..close_paren].trim();

    // Only match .html / .htm extensions
    let lower = path.to_lowercase();
    if !lower.ends_with(".html") && !lower.ends_with(".htm") {
        return None;
    }

    let total_consumed = 2 + close_bracket + 1 + 1 + close_paren + 1;
    let html = format!(
        r#"<iframe src="{}" title="{}" class="diaryx-island" sandbox="allow-scripts" loading="lazy" style="width:100%;min-height:200px;border:none;"></iframe>"#,
        html_escape(path),
        html_escape(alt),
    );

    Some((html, total_consumed))
}

/// Escape HTML special characters.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_default_color() {
        let out = preprocess_custom_syntax("a ==hi== b");
        assert_eq!(
            out,
            r#"a <mark data-highlight-color="yellow" class="highlight-mark highlight-yellow">hi</mark> b"#
        );
    }

    #[test]
    fn highlight_named_color() {
        let out = preprocess_custom_syntax("=={red}danger==");
        assert!(out.contains(r#"data-highlight-color="red""#));
        assert!(out.contains("highlight-red"));
        assert!(out.contains(">danger<"));
    }

    #[test]
    fn highlight_invalid_color_is_left_alone() {
        let out = preprocess_custom_syntax("=={mauve}x==");
        assert_eq!(out, "=={mauve}x==");
    }

    #[test]
    fn spoiler_basic() {
        let out = preprocess_custom_syntax("||secret||");
        assert_eq!(
            out,
            r#"<span data-spoiler="" class="spoiler-mark spoiler-hidden">secret</span>"#
        );
    }

    #[test]
    fn html_embed_becomes_iframe() {
        let out = preprocess_custom_syntax("![demo](island.html)");
        assert!(out.contains(r#"<iframe src="island.html""#));
        assert!(out.contains(r#"title="demo""#));
        assert!(out.contains(r#"class="diaryx-island""#));
    }

    #[test]
    fn inline_code_is_untouched() {
        let out = preprocess_custom_syntax("`==not a highlight==`");
        assert_eq!(out, "`==not a highlight==`");
    }

    #[test]
    fn fenced_code_is_untouched() {
        let input = "```\n==no==\n||no||\n```";
        let out = preprocess_custom_syntax(input);
        assert_eq!(out, input);
    }

    #[test]
    fn escapes_content() {
        let out = preprocess_custom_syntax("==<b>&\"==");
        assert!(out.contains("&lt;b&gt;&amp;&quot;"));
    }

    #[test]
    fn markdown_to_html_basics() {
        let html = markdown_to_html("# Title\n\n~~struck~~");
        assert!(html.contains("<h1>"));
        assert!(html.contains("<del>struck</del>"));
    }

    #[test]
    fn markdown_to_html_passes_through_raw_html() {
        // unsafe rendering keeps preprocessed raw HTML (e.g. <mark>) intact.
        let pre = preprocess_custom_syntax("==hi==");
        let html = markdown_to_html(&pre);
        assert!(html.contains("<mark"));
    }
}
