//! Tiny markdown -> styled, width-wrapped lines renderer for the TUI.
//!
//! Supports a practical subset: headings, code fences, inline `code`, **bold**,
//! *italic*, and bullet lists. Wrapping is display-width aware (CJK-safe) and
//! preserves inline styles across wrap boundaries.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Render markdown `text` to styled lines wrapped to `width` columns.
pub fn render(text: &str, width: usize, base: Style) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut out: Vec<Line> = Vec::new();
    let mut in_code = false;

    for raw in text.split('\n') {
        let trimmed = raw.trim_start();

        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            let style = Style::default().fg(Color::Rgb(180, 220, 140));
            for piece in hard_split(raw, width.saturating_sub(1)) {
                out.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(piece, style),
                ]));
            }
            continue;
        }

        // Headings
        if let Some((_, htext)) = heading(trimmed) {
            let style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
            let segs = parse_inline(htext, style);
            wrap_segments(&mut out, &segs, width, "");
            continue;
        }

        // Bullet list
        if let Some(rest) = bullet(trimmed) {
            let segs = parse_inline(rest, base);
            wrap_segments(&mut out, &segs, width, "• ");
            continue;
        }

        // Plain paragraph (preserve leading indentation roughly)
        let segs = parse_inline(raw, base);
        wrap_segments(&mut out, &segs, width, "");
    }
    out
}

fn heading(line: &str) -> Option<(usize, &str)> {
    if !line.starts_with('#') {
        return None;
    }
    let level = line.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = line[level..].trim_start();
    Some((level, rest))
}

fn bullet(line: &str) -> Option<&str> {
    for p in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(p) {
            return Some(rest);
        }
    }
    None
}

/// Parse inline markdown into styled segments.
fn parse_inline(text: &str, base: Style) -> Vec<(String, Style)> {
    let mut segs: Vec<(String, Style)> = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let bold = base.add_modifier(Modifier::BOLD);
    let italic = base.add_modifier(Modifier::ITALIC);
    let code = Style::default().fg(Color::Rgb(180, 220, 140)).bg(Color::Rgb(40, 40, 40));

    let flush = |cur: &mut String, segs: &mut Vec<(String, Style)>| {
        if !cur.is_empty() {
            segs.push((std::mem::take(cur), base));
        }
    };

    while i < chars.len() {
        // `inline code`
        if chars[i] == '`' {
            if let Some(end) = find(&chars, i + 1, '`') {
                flush(&mut cur, &mut segs);
                let s: String = chars[i + 1..end].iter().collect();
                segs.push((s, code));
                i = end + 1;
                continue;
            }
        }
        // **bold**
        if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            if let Some(end) = find2(&chars, i + 2) {
                flush(&mut cur, &mut segs);
                let s: String = chars[i + 2..end].iter().collect();
                segs.push((s, bold));
                i = end + 2;
                continue;
            }
        }
        // *italic*
        if chars[i] == '*' {
            if let Some(end) = find(&chars, i + 1, '*') {
                flush(&mut cur, &mut segs);
                let s: String = chars[i + 1..end].iter().collect();
                segs.push((s, italic));
                i = end + 1;
                continue;
            }
        }
        cur.push(chars[i]);
        i += 1;
    }
    flush(&mut cur, &mut segs);
    if segs.is_empty() {
        segs.push((String::new(), base));
    }
    segs
}

fn find(chars: &[char], from: usize, target: char) -> Option<usize> {
    (from..chars.len()).find(|&i| chars[i] == target)
}

/// Find a closing `**` starting at `from`.
fn find2(chars: &[char], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < chars.len() {
        if chars[i] == '*' && chars[i + 1] == '*' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Greedy, display-width-aware wrap of styled segments.
pub fn wrap_segments(
    out: &mut Vec<Line<'static>>,
    segs: &[(String, Style)],
    width: usize,
    prefix: &str,
) {
    let width = width.max(1);
    let indent: String = " ".repeat(UnicodeWidthStr::width(prefix));
    let mut cur: Vec<Span<'static>> = Vec::new();
    let mut cur_w = 0usize;
    let mut first = true;

    let mut flush = |cur: &mut Vec<Span<'static>>, first: &mut bool| {
        let mut spans: Vec<Span> = Vec::new();
        if *first {
            if !prefix.is_empty() {
                spans.push(Span::styled(
                    prefix.to_string(),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            *first = false;
        } else {
            spans.push(Span::raw(indent.clone()));
        }
        spans.append(cur);
        out.push(Line::from(spans));
    };

    for (text, style) in segs {
        for word in split_words(text) {
            let ww = UnicodeWidthStr::width(word.as_str());
            if cur_w + ww > width && cur_w > 0 {
                flush(&mut cur, &mut first);
                cur_w = 0;
            }
            if ww > width {
                for piece in hard_split(&word, width) {
                    let pw = UnicodeWidthStr::width(piece.as_str());
                    if cur_w + pw > width && cur_w > 0 {
                        flush(&mut cur, &mut first);
                        cur_w = 0;
                    }
                    cur.push(Span::styled(piece, *style));
                    cur_w += pw;
                }
            } else {
                cur.push(Span::styled(word, *style));
                cur_w += ww;
            }
        }
    }
    if !cur.is_empty() || first {
        flush(&mut cur, &mut first);
    }
}

/// Split into words keeping trailing spaces attached (so wrapping eats them).
fn split_words(text: &str) -> Vec<String> {
    text.split_inclusive(' ').map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    fn line_width(l: &Line) -> usize {
        l.spans
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum()
    }

    #[test]
    fn wraps_within_width_including_cjk() {
        let text = "**Bold** and `code` plus 一些中文字符混排测试，应该正确换行不溢出边界。";
        let lines = render(text, 20, Style::default());
        assert!(!lines.is_empty());
        for l in &lines {
            assert!(line_width(l) <= 20, "line too wide: {}", line_width(l));
        }
    }

    #[test]
    fn renders_headings_bullets_and_code_fence() {
        let md = "# Title\n- item one\n- item two\n```\nlet x = 1;\n```\ndone";
        let lines = render(md, 40, Style::default());
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(joined.contains("Title"));
        assert!(joined.contains("• item one"));
        assert!(joined.contains("let x = 1;"));
    }
}

/// Hard-split a token to at most `width` display columns per piece.
pub fn hard_split(s: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if cur_w + cw > width && cur_w > 0 {
            out.push(std::mem::take(&mut cur));
            cur_w = 0;
        }
        cur.push(c);
        cur_w += cw;
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}
