//! Markdown formatting on the source text: the toolbar and shortcuts used in Code and Split modes.

use egui::{vec2, Ui};

use crate::app::InkApp;
use crate::buffer::{Buffer, Edit, EditKind, Sel};
use crate::doc::Kind;
use crate::icons;

/// Wrap each selection in `open`/`close`, or unwrap it if it already is.
pub fn toggle_wrap(b: &mut Buffer, open: &str, close: &str) {
    let (on, cn) = (open.chars().count(), close.chars().count());
    let mut edits = Vec::new();
    let mut sels = Vec::new();
    let mut delta: isize = 0;
    let mut sorted = b.sels.clone();
    sorted.sort_by_key(|s| s.start());
    for s in sorted {
        let (mut a, mut z) = (s.start(), s.end());
        if a == z {
            let (wa, wz) = crate::edit::word_at(b, a);
            if wa < wz && b.char_at(wa).is_some_and(crate::buffer::is_word) {
                a = wa;
                z = wz;
            }
        }
        let inner = b.slice(a, z);
        let before = b.slice(a.saturating_sub(on), a);
        let after = b.slice(z, z + cn);
        let base = (a as isize + delta) as usize;
        if a >= on && before == open && after == close {
            // markers sit just outside the selection
            edits.push(Edit::new(a - on, a, ""));
            edits.push(Edit::new(z, z + cn, ""));
            sels.push(Sel::new(base - on, base - on + (z - a)));
            delta -= (on + cn) as isize;
        } else if inner.starts_with(open) && inner.ends_with(close) && inner.chars().count() >= on + cn {
            let body: String = inner.chars().skip(on).take(inner.chars().count() - on - cn).collect();
            let n = body.chars().count();
            edits.push(Edit::new(a, z, body));
            sels.push(Sel::new(base, base + n));
            delta -= (on + cn) as isize;
        } else {
            edits.push(Edit::new(a, z, format!("{open}{inner}{close}")));
            sels.push(Sel::new(base + on, base + on + (z - a)));
            delta += (on + cn) as isize;
        }
    }
    if b.edit(edits, EditKind::Other) {
        b.sels = sels;
        b.commit_sels();
    }
}

fn line_range(b: &Buffer) -> Vec<usize> {
    let mut lines = Vec::new();
    for s in &b.sels {
        let (l0, mut l1) = (b.line_of(s.start()), b.line_of(s.end()));
        if l1 > l0 && s.end() == b.line_start(l1) {
            l1 -= 1;
        }
        lines.extend(l0..=l1);
    }
    lines.sort();
    lines.dedup();
    lines
}

fn strip_block_prefix(t: &str) -> (usize, &str) {
    // returns (indent chars, rest without list/quote/heading marker)
    let indent = t.len() - t.trim_start().len();
    let rest = &t[indent..];
    let after = if let Some(r) = rest.strip_prefix("- [ ] ").or(rest.strip_prefix("- [x] ")).or(rest.strip_prefix("- [X] ")) {
        r
    } else if let Some(r) = rest.strip_prefix("- ").or(rest.strip_prefix("* ")).or(rest.strip_prefix("+ ")).or(rest.strip_prefix("> ")) {
        r
    } else if rest.starts_with('#') {
        rest.trim_start_matches('#').trim_start()
    } else {
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && (rest[digits..].starts_with(". ") || rest[digits..].starts_with(") ")) { &rest[digits + 2..] } else { rest }
    };
    (indent, after)
}

/// Give the selected lines a block prefix (`# `, `- `, `1. `, `> `, `- [ ] `), or remove it if all have it.
pub fn toggle_prefix(b: &mut Buffer, prefix: &str) {
    let lines = line_range(b);
    let numbered = prefix == "1. ";
    let has = |t: &str| {
        let r = t.trim_start();
        if numbered {
            let d = r.chars().take_while(|c| c.is_ascii_digit()).count();
            d > 0 && r[d..].starts_with(". ")
        } else if prefix == "- " {
            r.starts_with("- ") && !r.starts_with("- [")
        } else {
            r.starts_with(prefix)
        }
    };
    let texts: Vec<String> = lines.iter().map(|l| b.line_text(*l)).collect();
    let all = texts.iter().filter(|t| !t.trim().is_empty() || texts.len() == 1).all(|t| has(t));
    let mut edits = Vec::new();
    let mut n = 0;
    for (l, t) in lines.iter().zip(&texts) {
        if t.trim().is_empty() && texts.len() > 1 {
            continue;
        }
        let (indent, rest) = strip_block_prefix(t);
        let ind: String = t.chars().take(indent).collect();
        n += 1;
        let new = if all { format!("{ind}{rest}") } else if numbered { format!("{ind}{n}. {rest}") } else { format!("{ind}{prefix}{rest}") };
        if &new != t {
            edits.push(Edit::new(b.line_start(*l), b.line_end(*l), new));
        }
    }
    let last = *lines.last().unwrap_or(&0);
    if b.edit(edits, EditKind::Other) {
        let e = b.line_end(last);
        if lines.len() == 1 {
            b.set_caret(e);
        } else {
            b.set_sel(b.line_start(lines[0]), e);
        }
        b.commit_sels();
    }
}

/// Insert a block on its own lines at the caret; `$0` marks where the caret ends up.
pub fn insert_block(b: &mut Buffer, body: &str) {
    let s = b.primary();
    let line = b.line_of(s.end());
    let at = b.line_end(line);
    let blank_here = b.line_text(line).trim().is_empty();
    let next_blank = line + 1 >= b.lines() || b.line_text(line + 1).trim().is_empty();
    let caret_off = body.find("$0").map(|i| body[..i].chars().count());
    let clean = body.replace("$0", "");
    let lead = if blank_here { "" } else { "\n\n" };
    let tail = if next_blank { "\n" } else { "\n\n" };
    let text = format!("{lead}{clean}{tail}");
    let start = if blank_here { b.line_start(line) } else { at };
    b.edit(vec![Edit::new(start, at, text)], EditKind::Other);
    let base = start + lead.chars().count();
    b.set_caret(base + caret_off.unwrap_or(clean.chars().count()));
    b.commit_sels();
}

/// Enter at the end of a list item starts the next item; on an empty item it ends the list.
pub fn smart_newline(b: &mut Buffer) -> bool {
    if b.sels.len() != 1 || !b.primary().is_empty() {
        return false;
    }
    let h = b.primary().head;
    let line = b.line_of(h);
    if h != b.line_end(line) {
        return false;
    }
    let t = b.line_text(line);
    let re = regex::Regex::new(r"^(\s*(?:>\s?)*)([-*+]|\d+[.)])(\s+)(\[[ xX]\]\s+)?(.*)$").unwrap();
    let Some(c) = re.captures(&t) else { return false };
    let (lead, marker, gap, task, body) = (&c[1], &c[2], &c[3], c.get(4).map(|m| m.as_str()), &c[5]);
    if body.trim().is_empty() {
        // empty item: leave the list
        b.edit(vec![Edit::new(b.line_start(line), h, lead.trim_end())], EditKind::Other);
        b.set_caret(b.line_end(line));
        b.commit_sels();
        return true;
    }
    let next = match marker.trim_end_matches(['.', ')']).parse::<usize>() {
        Ok(n) => format!("{}{}", n + 1, &marker[marker.len() - 1..]),
        Err(_) => marker.to_string(),
    };
    let text = format!("\n{lead}{next}{gap}{}", if task.is_some() { "[ ] " } else { "" });
    let n = text.chars().count();
    b.edit(vec![Edit::new(h, h, text)], EditKind::Other);
    b.set_caret(h + n);
    b.commit_sels();
    true
}

pub fn command(app: &mut InkApp, name: &str, now: f64) -> bool {
    let Some(d) = app.docs.get_mut(app.active) else { return false };
    // Ctrl+B / Ctrl+I mean bold / italic in Markdown source
    let name = match (name, d.kind) {
        ("build", Kind::Markdown) => "md_bold",
        (n, _) => n,
    };
    if !name.starts_with("md_") {
        return false;
    }
    if d.kind != Kind::Markdown || d.read_only {
        return true;
    }
    let b = &mut d.buf;
    b.now = now;
    match name {
        "md_bold" => toggle_wrap(b, "**", "**"),
        "md_italic" => toggle_wrap(b, "*", "*"),
        "md_strike" => toggle_wrap(b, "~~", "~~"),
        "md_code" => toggle_wrap(b, "`", "`"),
        "md_highlight" => toggle_wrap(b, "==", "=="),
        "md_kbd" => toggle_wrap(b, "<kbd>", "</kbd>"),
        "md_paragraph" => toggle_prefix(b, ""),
        "md_h1" => toggle_prefix(b, "# "),
        "md_h2" => toggle_prefix(b, "## "),
        "md_h3" => toggle_prefix(b, "### "),
        "md_ul" => toggle_prefix(b, "- "),
        "md_ol" => toggle_prefix(b, "1. "),
        "md_task" => toggle_prefix(b, "- [ ] "),
        "md_quote" => toggle_prefix(b, "> "),
        "md_link" => {
            let s = b.primary();
            let text = b.slice(s.start(), s.end());
            let label = if text.is_empty() { "text".to_string() } else { text };
            let n = label.chars().count();
            b.edit(vec![Edit::new(s.start(), s.end(), format!("[{label}](url)"))], EditKind::Other);
            b.set_sel(s.start() + n + 3, s.start() + n + 6);
            b.commit_sels();
        }
        "md_image" => {
            let s = b.primary();
            b.edit(vec![Edit::new(s.start(), s.end(), "![alt](path)")], EditKind::Other);
            b.set_sel(s.start() + 7, s.start() + 11);
            b.commit_sels();
        }
        "md_codeblock" => {
            let s = b.primary();
            if s.is_empty() {
                insert_block(b, "```\n$0\n```");
            } else {
                let (l0, l1) = (b.line_of(s.start()), b.line_of(s.end()));
                let (a, z) = (b.line_start(l0), b.line_end(l1));
                let body = b.slice(a, z);
                b.edit(vec![Edit::new(a, z, format!("```\n{body}\n```"))], EditKind::Other);
                b.set_caret(a + 3);
                b.commit_sels();
            }
        }
        "md_table" => insert_block(b, "| $0Column | Column | Column |\n| --- | --- | --- |\n|  |  |  |\n|  |  |  |"),
        "md_hr" => insert_block(b, "---\n$0"),
        "md_math" => insert_block(b, "$$\n$0\n$$"),
        "md_mermaid" => insert_block(b, "```mermaid\nflowchart TD\n    A[Start] --> B{Decision}\n    B -->|Yes| C[Result]$0\n```"),
        "md_chart" => insert_block(b, "```chart\n{\n  \"title\": { \"text\": \"Title\" },\n  \"xAxis\": { \"type\": \"category\", \"data\": [\"A\", \"B\", \"C\"] },\n  \"yAxis\": {},\n  \"series\": [{ \"type\": \"bar\", \"data\": [3, 7, 5] }]$0\n}\n```"),
        "md_note" => insert_block(b, "> [!NOTE]\n> $0"),
        "md_footnote" => {
            let s = b.primary();
            let n = b.text().matches("[^").count() / 2 + 1;
            b.edit(vec![Edit::new(s.end(), s.end(), format!("[^{n}]"))], EditKind::Other);
            let end = b.len();
            let sep = if b.text().ends_with('\n') { "\n" } else { "\n\n" };
            b.edit(vec![Edit::new(end, end, format!("{sep}[^{n}]: "))], EditKind::Other);
            b.set_caret(b.len());
            b.commit_sels();
        }
        _ => return false,
    }
    d.edited_at = Some(now);
    d.ed.reveal = true;
    d.ed.focus = true;
    true
}

pub fn toolbar(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        let avail = ui.available_width();
        let groups: &[&[(&str, &str, &str)]] = &[
            &[(icons::BOLD, "Bold (Ctrl+B)", "md_bold"), (icons::ITALIC, "Italic", "md_italic"), (icons::STRIKETHROUGH, "Strikethrough", "md_strike"), (icons::CODE, "Inline code", "md_code"), (icons::HIGHLIGHTER, "Highlight", "md_highlight")],
            &[(icons::HEADING_1, "Heading 1", "md_h1"), (icons::HEADING_2, "Heading 2", "md_h2"), (icons::HEADING_3, "Heading 3", "md_h3")],
            &[(icons::LIST, "Bullet list", "md_ul"), (icons::LIST_ORDERED, "Numbered list", "md_ol"), (icons::LIST_CHECKS, "Task list", "md_task"), (icons::QUOTE, "Quote", "md_quote")],
            &[(icons::LINK, "Link", "md_link"), (icons::IMAGE, "Image", "md_image"), (icons::TABLE, "Table", "md_table"), (icons::SQUARE_CODE, "Code block", "md_codeblock"), (icons::MINUS, "Divider", "md_hr")],
            &[(icons::SIGMA, "Math block", "md_math"), (icons::WORKFLOW, "Mermaid diagram", "md_mermaid"), (icons::CHART_COLUMN, "Chart", "md_chart"), (icons::INFO, "Callout", "md_note")],
        ];
        for (gi, group) in groups.iter().enumerate() {
            // show only the groups that fit
            let needed: f32 = groups[..=gi].iter().map(|g| g.len() as f32 * 31.0 + 16.0).sum();
            if needed > avail && gi > 0 {
                break;
            }
            if gi > 0 {
                ui.add_space(5.0);
                let (r, _) = ui.allocate_exact_size(vec2(1.0, 18.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 0, pal.border_soft);
                ui.add_space(5.0);
            }
            for (icon, tip, cmd) in group.iter() {
                let b = egui::Button::new(egui::RichText::new(*icon).size(14.5).color(pal.muted)).frame_when_inactive(false).min_size(vec2(28.0, 26.0)).corner_radius(6);
                if ui.add(b).on_hover_text(*tip).clicked() {
                    app.pending.push(cmd.to_string());
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_prefixes() {
        let mut b = Buffer::new("hello world");
        b.set_sel(0, 5);
        toggle_wrap(&mut b, "**", "**");
        assert_eq!(b.text(), "**hello** world");
        assert_eq!((b.primary().start(), b.primary().end()), (2, 7));
        toggle_wrap(&mut b, "**", "**");
        assert_eq!(b.text(), "hello world");
        b.set_caret(8);
        toggle_wrap(&mut b, "`", "`");
        assert_eq!(b.text(), "hello `world`");

        let mut b = Buffer::new("one\ntwo");
        b.set_sel(0, 7);
        toggle_prefix(&mut b, "1. ");
        assert_eq!(b.text(), "1. one\n2. two");
        b.set_sel(0, b.len());
        toggle_prefix(&mut b, "- [ ] ");
        assert_eq!(b.text(), "- [ ] one\n- [ ] two");
        b.set_sel(0, b.len());
        toggle_prefix(&mut b, "- [ ] ");
        assert_eq!(b.text(), "one\ntwo");
        b.set_caret(1);
        toggle_prefix(&mut b, "## ");
        assert_eq!(b.text(), "## one\ntwo");

        let mut b = Buffer::new("para");
        b.set_caret(2);
        insert_block(&mut b, "---\n$0");
        assert_eq!(b.text(), "para\n\n---\n\n");
    }
}
