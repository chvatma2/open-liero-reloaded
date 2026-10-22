//! A release's notes (GitHub's Markdown) as readable plain text for the
//! update screen: headings, emphasis, links and code marks become text,
//! list items bullets, HTML comments and tags go. Line structure is
//! kept; nothing is lost but markup. [`changes`]: the notes of every
//! release a player skips, each under its heading, within caps.

use crate::release::Release;

/// The most characters of notes shown (one release's, and all of
/// [`changes`]' together).
pub const MAX_NOTES_CHARS: usize = 20_000;

/// The most releases whose notes [`changes`] shows.
pub const MAX_RELEASES: usize = 10;

/// One release's notes under its heading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseNotes {
    /// Its title and tag: `0.2.0 development build 318 (v0.2.0-dev.318)`;
    /// the tag alone for a release titled by it.
    pub heading: String,
    /// [`plain_text`] of its notes (empty: none).
    pub text: String,
}

/// The notes [`changes`] shows, and how many releases it leaves out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub shown: Vec<ReleaseNotes>,
    /// The older releases not shown (over the caps).
    pub left_out: usize,
}

/// The notes of `releases` (newest first, as `release::since` lists
/// them) in that order: at most [`MAX_RELEASES`], and the heading and
/// text of all together at most [`MAX_NOTES_CHARS`] characters, the
/// newest always shown (its text capped alone); the rest are left out.
pub fn changes(releases: &[&Release]) -> Changes {
    let mut out = Changes::default();
    let mut chars = 0;
    for (i, r) in releases.iter().enumerate() {
        let heading = if r.title == r.tag { r.tag.clone() } else { format!("{} ({})", r.title, r.tag) };
        let text = plain_text(&r.notes);
        let len = heading.chars().count() + text.chars().count();
        if i >= MAX_RELEASES || (i > 0 && chars + len > MAX_NOTES_CHARS) {
            out.left_out = releases.len() - i;
            break;
        }
        chars += len;
        out.shown.push(ReleaseNotes { heading, text });
    }
    out
}

/// `markdown` as plain text.
pub fn plain_text(markdown: &str) -> String {
    let text = markdown.replace("\r\n", "\n").replace('\r', "\n");
    let text = strip_comments(&text);
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            out.push(format!("    {line}"));
            continue;
        }
        let indent = &line[..line.len() - trimmed.len()];
        let converted = if let Some(h) = heading(trimmed) {
            inline(h)
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            let item = item
                .strip_prefix("[ ] ")
                .map(|r| format!("☐ {r}"))
                .or_else(|| item.strip_prefix("[x] ").map(|r| format!("☑ {r}")))
                .unwrap_or_else(|| item.to_string());
            format!("{indent}• {}", inline(&item))
        } else if let Some(q) = trimmed.strip_prefix('>') {
            format!("{indent}│ {}", inline(q.trim_start()))
        } else if is_rule(trimmed) {
            "────────".to_string()
        } else {
            format!("{indent}{}", inline(trimmed))
        };
        out.push(converted.trim_end().to_string());
    }
    // At most one blank line in a row, none at the ends.
    let mut lines: Vec<String> = Vec::new();
    for l in out {
        if l.is_empty() && lines.last().is_none_or(|p| p.is_empty()) {
            continue;
        }
        lines.push(l);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut text = lines.join("\n");
    if text.chars().count() > MAX_NOTES_CHARS {
        text = text.chars().take(MAX_NOTES_CHARS).collect::<String>() + "\n…";
    }
    text
}

fn heading(line: &str) -> Option<&str> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        Some(line[hashes..].trim().trim_end_matches('#').trim_end())
    } else {
        None
    }
}

fn is_rule(line: &str) -> bool {
    let l: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    l.len() >= 3
        && (l.chars().all(|c| c == '-') || l.chars().all(|c| c == '*') || l.chars().all(|c| c == '_'))
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        match rest[i..].find("-->") {
            Some(j) => rest = &rest[i + j + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// One line's inline markup removed: `**`, `__`, `*`, `_` around words,
/// `` ` ``, `[text](url)` → `text (url)`, `![alt](url)` → `alt`, tags.
fn inline(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() && chars[i + 1].is_ascii_punctuation() => {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            '!' | '[' => {
                let image = c == '!';
                let open = if image { i + 1 } else { i };
                if chars.get(open) == Some(&'[')
                    && let Some((text, url, end)) = link(&chars, open)
                {
                    if image || text == url || url.is_empty() {
                        out.push_str(&inline(&text));
                    } else {
                        out.push_str(&format!("{} ({url})", inline(&text)));
                    }
                    i = end;
                    continue;
                }
            }
            '`' => {
                i += 1;
                continue;
            }
            '*' | '_' => {
                // Emphasis marks: at a word's edge; `snake_case` keeps its `_`.
                let prev = i.checked_sub(1).map(|p| chars[p]);
                let mut j = i;
                while j < chars.len() && chars[j] == c {
                    j += 1;
                }
                let next = chars.get(j).copied();
                let word = |x: Option<char>| x.is_some_and(|x| x.is_alphanumeric());
                if !(word(prev) && word(next)) {
                    i = j;
                    continue;
                }
            }
            '<' => {
                if let Some(end) = chars[i..].iter().position(|&x| x == '>') {
                    let tag: String = chars[i + 1..i + end].iter().collect();
                    if tag.starts_with("http://") || tag.starts_with("https://") {
                        out.push_str(&tag);
                        i += end + 1;
                        continue;
                    }
                    let name = tag.trim_start_matches('/');
                    if name.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                        if name.starts_with("br") {
                            out.push(' ');
                        }
                        i += end + 1;
                        continue;
                    }
                }
            }
            _ => {}
        }
        out.push(c);
        i += 1;
    }
    out
}

/// `[text](url)` starting at `chars[open] == '['`: the text, the url and
/// the index after it.
fn link(chars: &[char], open: usize) -> Option<(String, String, usize)> {
    let mut depth = 0;
    let mut close = None;
    for (k, &c) in chars.iter().enumerate().skip(open) {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(k);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = chars[close + 2..].iter().position(|&c| c == ')')? + close + 2;
    let text: String = chars[open + 1..close].iter().collect();
    let url: String = chars[close + 2..end].iter().collect();
    let url = url.split_whitespace().next().unwrap_or("").to_string();
    Some((text, url, end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_reads_as_text() {
        let md = "## What's new\r\n\r\n- **Update** from the [main menu](https://x.example/m)\n\
                  - `cargo xtask publish` and snake_case_names\n  * nested _item_\n\n\n\
                  <!-- hidden -->\n> quoted\n\n---\n```\nflatpak update\n```\n![shot](a.png) <b>bold</b>\n# End #";
        assert_eq!(
            plain_text(md),
            "What's new\n\n• Update from the main menu (https://x.example/m)\n\
             • cargo xtask publish and snake_case_names\n  • nested item\n\n│ quoted\n\n────────\n\
             \x20   flatpak update\nshot bold\nEnd"
        );
    }

    #[test]
    fn odd_markdown_never_breaks() {
        for s in ["[", "[a](", "<", "<!-- open", "**", "\\", "```", "# ", "[a]", "![", "_*_*", "<<>>"] {
            let _ = plain_text(s);
        }
        assert_eq!(plain_text("a\\*b\\*"), "a*b*");
        assert_eq!(plain_text("<https://x.example>"), "https://x.example");
        let long = "x".repeat(MAX_NOTES_CHARS + 10);
        assert!(plain_text(&long).ends_with('…'));
    }
}
