//! Convert common LLM/Markdown markup into Telegram HTML.
//!
//! OpenAI-style replies use `**bold**`, while Telegram expects HTML or MarkdownV2.
//! HTML is more forgiving after a small conversion pass.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseMode {
    #[default]
    Plain,
    /// CommonMark-ish (`**bold**`, `*italic*`, `` `code` ``) → Telegram HTML.
    Markdown,
    /// Already Telegram HTML (`<b>`, `<i>`, `<code>`, `<pre>`).
    Html,
}

impl ParseMode {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("markdown") | Some("md") => Self::Markdown,
            Some("html") => Self::Html,
            Some("plain") | Some("text") | Some("none") | Some("") | None => Self::Plain,
            _ => Self::Markdown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Markdown => "markdown",
            Self::Html => "html",
        }
    }
}

/// Escape text for Telegram HTML body (outside tags).
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Convert a subset of CommonMark into Telegram-safe HTML.
pub fn markdown_to_telegram_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Fenced code block ```
        if bytes[i..].starts_with(b"```") {
            i += 3;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1; // skip language tag
            }
            if i < bytes.len() && bytes[i] == b'\n' {
                i += 1;
            }
            let start = i;
            while i + 2 < bytes.len() && !(bytes[i] == b'`' && bytes[i + 1] == b'`' && bytes[i + 2] == b'`')
            {
                i += 1;
            }
            let code = &input[start..i];
            out.push_str("<pre>");
            out.push_str(&escape_html(code.trim_end_matches('\n')));
            out.push_str("</pre>");
            if i + 2 < bytes.len() {
                i += 3;
            } else {
                i = bytes.len();
            }
            continue;
        }

        // Inline code `...`
        if bytes[i] == b'`' {
            if let Some(end) = input[i + 1..].find('`') {
                let code = &input[i + 1..i + 1 + end];
                if !code.contains('\n') {
                    out.push_str("<code>");
                    out.push_str(&escape_html(code));
                    out.push_str("</code>");
                    i += 2 + end;
                    continue;
                }
            }
        }

        // Bold **...** or __...__
        if let Some((marker, len)) = match_pair(bytes, i, b"**").or_else(|| match_pair(bytes, i, b"__"))
        {
            out.push_str("<b>");
            out.push_str(&inline_markdown_to_html(marker));
            out.push_str("</b>");
            i += len;
            continue;
        }

        // Italic *...* or _..._ (single, not part of **)
        if (bytes[i] == b'*' || bytes[i] == b'_')
            && (i + 1 >= bytes.len() || bytes[i + 1] != bytes[i])
        {
            let delim = bytes[i];
            if let Some(end) = find_closing(bytes, i + 1, delim) {
                let inner = &input[i + 1..end];
                if !inner.is_empty() && !inner.contains('\n') {
                    out.push_str("<i>");
                    out.push_str(&escape_html(inner));
                    out.push_str("</i>");
                    i = end + 1;
                    continue;
                }
            }
        }

        // Plain char (escape HTML)
        let ch = input[i..].chars().next().unwrap();
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
        i += ch.len_utf8();
    }
    out
}

fn inline_markdown_to_html(text: &str) -> String {
    // Nested italics inside bold — keep simple: escape only.
    escape_html(text)
}

fn match_pair<'a>(bytes: &'a [u8], i: usize, marker: &[u8]) -> Option<(&'a str, usize)> {
    if !bytes[i..].starts_with(marker) {
        return None;
    }
    let start = i + marker.len();
    let mut j = start;
    while j + marker.len() <= bytes.len() {
        if bytes[j..].starts_with(marker) {
            let inner = std::str::from_utf8(&bytes[start..j]).ok()?;
            if inner.is_empty() || inner.contains('\n') {
                return None;
            }
            return Some((inner, marker.len() * 2 + inner.len()));
        }
        j += 1;
    }
    None
}

fn find_closing(bytes: &[u8], from: usize, delim: u8) -> Option<usize> {
    let mut j = from;
    while j < bytes.len() {
        if bytes[j] == delim {
            return Some(j);
        }
        if bytes[j] == b'\n' {
            return None;
        }
        j += 1;
    }
    None
}

/// Prepare text for Telegram: returns (body, bot_api_parse_mode).
/// `bot_api_parse_mode` is `Some("HTML")` when formatting is applied.
pub fn prepare_telegram_text(text: &str, mode: ParseMode) -> (String, Option<&'static str>) {
    match mode {
        ParseMode::Plain => (text.to_string(), None),
        ParseMode::Html => (text.to_string(), Some("HTML")),
        ParseMode::Markdown => (markdown_to_telegram_html(text), Some("HTML")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_bold_italic_code() {
        let html = markdown_to_telegram_html(
            "В России живёт примерно **146 миллионов** человек и *примерно* `9`-е место.",
        );
        assert!(html.contains("<b>146 миллионов</b>"));
        assert!(html.contains("<i>примерно</i>"));
        assert!(html.contains("<code>9</code>"));
        assert!(!html.contains("**"));
    }

    #[test]
    fn converts_code_fence() {
        let html = markdown_to_telegram_html("before\n```rust\nlet x = 1;\n```\nafter");
        assert!(html.contains("<pre>let x = 1;</pre>"));
        assert!(html.contains("before"));
        assert!(html.contains("after"));
    }

    #[test]
    fn escapes_html_in_plain() {
        assert_eq!(
            markdown_to_telegram_html("a <b> & c"),
            "a &lt;b&gt; &amp; c"
        );
    }

    #[test]
    fn prepare_modes() {
        let (t, m) = prepare_telegram_text("**hi**", ParseMode::Markdown);
        assert_eq!(t, "<b>hi</b>");
        assert_eq!(m, Some("HTML"));
        let (t, m) = prepare_telegram_text("**hi**", ParseMode::Plain);
        assert_eq!(t, "**hi**");
        assert!(m.is_none());
    }
}
