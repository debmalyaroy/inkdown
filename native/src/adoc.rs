//! AsciiDoc support: the common subset is translated to Markdown and rendered by the
//! Markdown engine (titles, lists, tables, admonitions, source blocks, images, links, math).

use std::sync::OnceLock;

use regex::Regex;

struct Rx {
    bold: Regex,
    bold2: Regex,
    italic: Regex,
    mark: Regex,
    link: Regex,
    link_macro: Regex,
    xref: Regex,
    xref2: Regex,
    image: Regex,
    stem: Regex,
    kbd: Regex,
    attr: Regex,
}

fn rx() -> &'static Rx {
    static R: OnceLock<Rx> = OnceLock::new();
    R.get_or_init(|| Rx {
        bold2: Regex::new(r"\*\*([^*\n]+?)\*\*").unwrap(),
        bold: Regex::new(r"(^|[\s(\[{>])\*([^\s*][^*\n]*?)\*($|[\s.,;:!?)\]}<])").unwrap(),
        italic: Regex::new(r"(^|[\s(\[{>*])_([^\s_][^_\n]*?)_($|[\s.,;:!?)\]}<*])").unwrap(),
        mark: Regex::new(r"(^|[\s(])#([^\s#][^#\n]*?)#($|[\s.,;:!?)])").unwrap(),
        link: Regex::new(r"(https?://[^\s\[\]]+)\[([^\]]*)\]").unwrap(),
        link_macro: Regex::new(r"link:([^\s\[\]]+)\[([^\]]*)\]").unwrap(),
        xref: Regex::new(r"<<([^,>]+)(?:,\s*([^>]+))?>>").unwrap(),
        xref2: Regex::new(r"xref:([^\s\[\]]+)\[([^\]]*)\]").unwrap(),
        image: Regex::new(r"image::?([^\s\[\]]+)\[([^\]]*)\]").unwrap(),
        stem: Regex::new(r"(?:stem|latexmath):\[([^\]]+)\]").unwrap(),
        kbd: Regex::new(r"kbd:\[([^\]]+)\]").unwrap(),
        attr: Regex::new(r"\{([a-zA-Z0-9_-]+)\}").unwrap(),
    })
}

fn inline(line: &str, attrs: &[(String, String)]) -> String {
    let r = rx();
    // protect inline code spans
    let mut parts: Vec<String> = Vec::new();
    for (i, seg) in line.split('`').enumerate() {
        if i % 2 == 1 {
            parts.push(format!("`{seg}`"));
            continue;
        }
        let mut s = seg.to_string();
        if !attrs.is_empty() && s.contains('{') {
            s = r.attr.replace_all(&s, |c: &regex::Captures| attrs.iter().find(|a| a.0 == c[1]).map(|a| a.1.clone()).unwrap_or_else(|| c[0].to_string())).into_owned();
        }
        s = r.image.replace_all(&s, |c: &regex::Captures| format!("![{}]({})", c[2].split(',').next().unwrap_or(""), &c[1])).into_owned();
        s = r.link.replace_all(&s, |c: &regex::Captures| if c[2].is_empty() { c[1].to_string() } else { format!("[{}]({})", c[2].trim_end_matches('^'), &c[1]) }).into_owned();
        s = r.link_macro.replace_all(&s, "[$2]($1)").into_owned();
        s = r.xref2.replace_all(&s, |c: &regex::Captures| format!("[{}](#{})", &c[2], c[1].trim_start_matches('#'))).into_owned();
        s = r.xref.replace_all(&s, |c: &regex::Captures| format!("[{}](#{})", c.get(2).map(|m| m.as_str()).unwrap_or(&c[1]), crate::md::slug(&c[1]))).into_owned();
        s = r.stem.replace_all(&s, "$$$1$$").into_owned();
        s = r.kbd.replace_all(&s, |c: &regex::Captures| c[1].split('+').map(|k| format!("<kbd>{}</kbd>", k.trim())).collect::<Vec<_>>().join("+")).into_owned();
        s = r.mark.replace_all(&s, "$1==$2==$3").into_owned();
        // constrained bold first (single stars), using a placeholder so italics can run after
        s = r.bold2.replace_all(&s, "\u{1}$1\u{1}").into_owned();
        s = r.bold.replace_all(&s, "$1\u{1}$2\u{1}$3").into_owned();
        s = r.italic.replace_all(&s, "$1*$2*$3").into_owned();
        s = s.replace('\u{1}', "**");
        parts.push(s);
    }
    let mut out = parts.concat();
    if out.ends_with(" +") {
        out.truncate(out.len() - 2);
        out.push_str("  ");
    }
    out
}

pub fn to_markdown(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = String::with_capacity(src.len() + 256);
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    let mut pending_lang: Option<String> = None;
    let mut pending_admon: Option<String> = None;
    let mut pending_math = false;
    let mut first_title = true;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_end();
        let trimmed = t.trim_start();
        // comments
        if trimmed.starts_with("////") {
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("////") {
                i += 1;
            }
            i += 1;
            continue;
        }
        if trimmed.starts_with("//") {
            i += 1;
            continue;
        }
        // attribute entries
        if let Some(rest) = t.strip_prefix(':') {
            if let Some((k, v)) = rest.split_once(':') {
                if !k.contains(' ') {
                    attrs.push((k.trim_start_matches('!').to_string(), v.trim().to_string()));
                    i += 1;
                    continue;
                }
            }
        }
        // block attribute lines
        if t.starts_with('[') && t.ends_with(']') && !t.starts_with("[[") {
            let inner = &t[1..t.len() - 1];
            let lower = inner.to_lowercase();
            let mut it = inner.split(',').map(str::trim);
            match it.next().unwrap_or("").to_lowercase().as_str() {
                "source" | "listing" => pending_lang = Some(it.next().unwrap_or("").to_string()),
                "mermaid" | "graphviz" | "plantuml" | "dot" => pending_lang = Some(if lower.starts_with("graphviz") { "dot".into() } else { lower.split(',').next().unwrap_or("").to_string() }),
                "stem" | "latexmath" | "asciimath" => pending_math = true,
                a @ ("note" | "tip" | "important" | "warning" | "caution") => pending_admon = Some(a.to_uppercase()),
                _ => {}
            }
            i += 1;
            continue;
        }
        if t.starts_with("[[") && t.ends_with("]]") {
            i += 1;
            continue;
        }
        // delimited blocks
        let delim = |d: &str| t.len() >= 4 && t.chars().all(|c| c == d.chars().next().unwrap()) && t.starts_with(d);
        if delim("----") || delim("....") || delim("++++") {
            let close = t.to_string();
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && lines[i].trim_end() != close {
                body.push(lines[i]);
                i += 1;
            }
            i += 1;
            if close.starts_with('+') {
                if pending_math {
                    out.push_str(&format!("$$\n{}\n$$\n\n", body.join("\n")));
                } else {
                    out.push_str(&body.join("\n"));
                    out.push_str("\n\n");
                }
            } else {
                out.push_str(&format!("```{}\n{}\n```\n\n", pending_lang.take().unwrap_or_default(), body.join("\n")));
            }
            pending_math = false;
            pending_lang = None;
            continue;
        }
        if delim("====") || delim("____") || delim("****") {
            let close = t.to_string();
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && lines[i].trim_end() != close {
                body.push(lines[i]);
                i += 1;
            }
            i += 1;
            let inner = to_markdown(&body.join("\n"));
            if let Some(a) = pending_admon.take() {
                out.push_str(&format!("> [!{a}]\n"));
            }
            for l in inner.trim_end().lines() {
                out.push_str("> ");
                out.push_str(l);
                out.push('\n');
            }
            out.push('\n');
            continue;
        }
        if t.starts_with("|===") {
            let mut cells: Vec<Vec<String>> = Vec::new();
            i += 1;
            let mut row: Vec<String> = Vec::new();
            let mut cols = 0usize;
            while i < lines.len() && !lines[i].trim_end().starts_with("|===") {
                let l = lines[i].trim();
                if l.is_empty() {
                    if !row.is_empty() && cols == 0 {
                        cols = row.len();
                    }
                    if !row.is_empty() {
                        cells.push(std::mem::take(&mut row));
                    }
                } else if let Some(rest) = l.strip_prefix('|') {
                    let parts: Vec<String> = rest.split('|').map(|c| inline(c.trim(), &attrs)).collect();
                    let multi = parts.len() > 1;
                    row.extend(parts);
                    if multi || (cols > 0 && row.len() >= cols) {
                        if cols == 0 {
                            cols = row.len();
                        }
                        cells.push(std::mem::take(&mut row));
                    }
                }
                i += 1;
            }
            if !row.is_empty() {
                cells.push(row);
            }
            i += 1;
            let width = cells.iter().map(Vec::len).max().unwrap_or(0);
            for (ri, r) in cells.iter().enumerate() {
                let mut padded = r.clone();
                padded.resize(width, String::new());
                out.push_str(&format!("| {} |\n", padded.join(" | ")));
                if ri == 0 {
                    out.push_str(&format!("|{}\n", " --- |".repeat(width)));
                }
            }
            out.push('\n');
            continue;
        }
        if t == "'''" || t == "---" || t == "***" {
            out.push_str("---\n\n");
            i += 1;
            continue;
        }
        // titles
        let eq = t.chars().take_while(|c| *c == '=').count();
        if (1..=6).contains(&eq) && t[eq..].starts_with(' ') {
            out.push_str(&format!("{} {}\n\n", "#".repeat(eq), inline(t[eq..].trim(), &attrs)));
            i += 1;
            // author / revision lines directly under the document title
            if first_title && eq == 1 {
                let mut meta = Vec::new();
                while i < lines.len() && !lines[i].trim().is_empty() && !lines[i].starts_with(':') && !lines[i].starts_with('=') && meta.len() < 2 {
                    meta.push(lines[i].trim());
                    i += 1;
                }
                if !meta.is_empty() {
                    out.push_str(&format!("*{}*\n\n", meta.join(" · ")));
                }
            }
            first_title = false;
            continue;
        }
        // block title
        if t.starts_with('.') && t.len() > 1 && !t.starts_with("..") && !t[1..].starts_with(' ') {
            out.push_str(&format!("**{}**\n\n", inline(&t[1..], &attrs)));
            i += 1;
            continue;
        }
        // admonition paragraph
        if let Some((kind, rest)) = t.split_once(": ") {
            if matches!(kind, "NOTE" | "TIP" | "IMPORTANT" | "WARNING" | "CAUTION") {
                out.push_str(&format!("> [!{kind}]\n> {}\n", inline(rest, &attrs)));
                i += 1;
                while i < lines.len() && !lines[i].trim().is_empty() {
                    out.push_str(&format!("> {}\n", inline(lines[i].trim(), &attrs)));
                    i += 1;
                }
                out.push('\n');
                continue;
            }
        }
        // lists
        let stars = trimmed.chars().take_while(|c| *c == '*').count();
        let dots = trimmed.chars().take_while(|c| *c == '.').count();
        if stars >= 1 && trimmed[stars..].starts_with(' ') {
            let body = trimmed[stars..].trim_start();
            out.push_str(&format!("{}- {}\n", "  ".repeat(stars - 1), inline(body, &attrs)));
            i += 1;
            if i >= lines.len() || lines[i].trim().is_empty() {
                out.push('\n');
            }
            continue;
        }
        if dots >= 1 && trimmed[dots..].starts_with(' ') {
            out.push_str(&format!("{}1. {}\n", "   ".repeat(dots - 1), inline(trimmed[dots..].trim_start(), &attrs)));
            i += 1;
            if i >= lines.len() || lines[i].trim().is_empty() {
                out.push('\n');
            }
            continue;
        }
        // description list: `term:: text`
        if let Some((term, def)) = t.split_once(":: ") {
            if !term.contains("  ") && !term.is_empty() {
                out.push_str(&format!("**{}**: {}\n\n", inline(term.trim(), &attrs), inline(def.trim(), &attrs)));
                i += 1;
                continue;
            }
        }
        if t.is_empty() {
            out.push('\n');
        } else {
            out.push_str(&inline(t, &attrs));
            out.push('\n');
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_common_asciidoc() {
        let md = to_markdown("= Title\nJane Doe\n:toc:\n\n== Section\n\nSome *bold* and _italic_ and `code *x*` text.\nSee https://example.com[the site] and <<Section>>.\n\n* one\n** nested\n. first\n\nNOTE: Careful now.\n\n[source,rust]\n----\nfn main() {}\n----\n\n|===\n|A |B\n\n|1 |2\n|===\n\nimage::pic.png[A picture]\n");
        assert!(md.contains("# Title\n"), "{md}");
        assert!(md.contains("*Jane Doe*"));
        assert!(md.contains("## Section"));
        assert!(md.contains("Some **bold** and *italic* and `code *x*` text."), "{md}");
        assert!(md.contains("[the site](https://example.com)") && md.contains("[Section](#section)"));
        assert!(md.contains("- one\n  - nested\n1. first"), "{md}");
        assert!(md.contains("> [!NOTE]\n> Careful now."));
        assert!(md.contains("```rust\nfn main() {}\n```"));
        assert!(md.contains("| A | B |\n| --- | --- |\n| 1 | 2 |"), "{md}");
        assert!(md.contains("![A picture](pic.png)"));
    }
}
