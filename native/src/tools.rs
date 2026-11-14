//! Built-in text tools (Menu > Tools and the command palette).

use crate::app::{Dialog, InkApp};
use crate::buffer::{Edit, EditKind};
use crate::data;
use crate::doc::Kind;

/// Local date and time as `(year, month, day, hour, minute)`.
pub fn now_local() -> (i64, u32, u32, u32, u32) {
    #[cfg(windows)]
    {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        (t.wYear as i64, t.wMonth as u32, t.wDay as u32, t.wHour as u32, t.wMinute as u32)
    }
    #[cfg(not(windows))]
    {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let (y, m, d) = civil(secs.div_euclid(86400));
        let rem = secs.rem_euclid(86400);
        (y, m, d, (rem / 3600) as u32, (rem % 3600 / 60) as u32)
    }
}

/// Days since 1970-01-01 to a calendar date.
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn uuid4() -> String {
    let mut b = [0u8; 16];
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = getrandom::fill(&mut b);
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        use std::hash::{BuildHasher, Hasher};
        for chunk in b.chunks_mut(8) {
            let v = std::collections::hash_map::RandomState::new().build_hasher().finish().to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Re-pad every Markdown table so its pipes line up.
pub fn align_tables(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    let is_row = |l: &str| l.trim_start().starts_with('|') && l.trim_end().ends_with('|') && l.trim().len() > 1;
    let mut in_code = false;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("```") {
            in_code = !in_code;
        }
        if in_code || !is_row(lines[i]) {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        }
        let start = i;
        while i < lines.len() && is_row(lines[i]) {
            i += 1;
        }
        let rows: Vec<Vec<String>> = lines[start..i]
            .iter()
            .map(|l| {
                let t = l.trim();
                let inner = &t[1..t.len() - 1];
                let mut cells = Vec::new();
                let mut cur = String::new();
                let mut esc = false;
                for c in inner.chars() {
                    if c == '|' && !esc {
                        cells.push(cur.trim().to_string());
                        cur.clear();
                    } else {
                        cur.push(c);
                    }
                    esc = c == '\\';
                }
                cells.push(cur.trim().to_string());
                cells
            })
            .collect();
        let is_sep = |r: &Vec<String>| r.iter().all(|c| !c.is_empty() && c.chars().all(|x| x == '-' || x == ':'));
        if rows.len() < 2 || !is_sep(&rows[1]) {
            out.extend(lines[start..i].iter().map(|s| s.to_string()));
            continue;
        }
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut widths = vec![3usize; cols];
        for (ri, r) in rows.iter().enumerate() {
            if ri == 1 {
                continue;
            }
            for (ci, c) in r.iter().enumerate() {
                widths[ci] = widths[ci].max(c.chars().count());
            }
        }
        let indent: String = lines[start].chars().take_while(|c| c.is_whitespace()).collect();
        for (ri, r) in rows.iter().enumerate() {
            let mut line = format!("{indent}|");
            for ci in 0..cols {
                let cell = r.get(ci).map(String::as_str).unwrap_or("");
                if ri == 1 {
                    let (l, rr) = (cell.starts_with(':'), cell.ends_with(':') && cell.len() > 1);
                    let dashes = "-".repeat(widths[ci] - usize::from(l) - usize::from(rr));
                    line.push_str(&format!(" {}{}{} |", if l { ":" } else { "" }, dashes, if rr { ":" } else { "" }));
                } else {
                    let pad = widths[ci] - cell.chars().count();
                    line.push_str(&format!(" {cell}{} |", " ".repeat(pad)));
                }
            }
            out.push(line);
        }
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    s
}

pub fn renumber_lists(text: &str) -> String {
    let re = regex::Regex::new(r"^(\s*)(\d+)([.)])(\s+)(.*)$").unwrap();
    let mut counters: Vec<(usize, usize)> = Vec::new(); // (indent, next number)
    let mut out = Vec::new();
    let mut in_code = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
        }
        if in_code {
            out.push(line.to_string());
            continue;
        }
        match re.captures(line) {
            Some(c) => {
                let indent = c[1].len();
                while counters.last().is_some_and(|x| x.0 > indent) {
                    counters.pop();
                }
                if counters.last().map(|x| x.0) != Some(indent) {
                    counters.push((indent, 1));
                }
                let n = counters.last_mut().unwrap();
                out.push(format!("{}{}{}{}{}", &c[1], n.1, &c[3], &c[4], &c[5]));
                n.1 += 1;
            }
            None => {
                let indent = line.len() - line.trim_start().len();
                if line.trim().is_empty() {
                    // a blank line keeps the list going only if the next item continues it
                } else if counters.last().is_some_and(|x| indent <= x.0) {
                    counters.clear();
                }
                out.push(line.to_string());
            }
        }
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    s
}

pub fn toc(text: &str) -> String {
    let tops = crate::md::parse(text);
    let heads = crate::md::outline(&tops);
    let min = heads.iter().map(|h| h.0).filter(|l| *l > 0).min().unwrap_or(1);
    heads.iter().map(|(l, t, id, _)| format!("{}- [{t}](#{id})", "  ".repeat((*l - min) as usize))).collect::<Vec<_>>().join("\n")
}

pub fn csv_to_markdown(text: &str, delim: char) -> String {
    let (rows, _) = crate::preview::parse_csv(text, delim, 5000);
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut out = String::new();
    for (i, r) in rows.iter().enumerate() {
        let cells: Vec<String> = (0..cols).map(|c| r.get(c).map(|s| s.replace('|', "\\|").replace('\n', " ")).unwrap_or_default()).collect();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
        if i == 0 {
            out.push_str(&format!("|{}\n", " --- |".repeat(cols)));
        }
    }
    align_tables(&out)
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn map_selection(app: &mut InkApp, now: f64, f: impl Fn(&str) -> Result<String, String>) {
    let Some(d) = app.docs.get_mut(app.active) else { return };
    let s = d.buf.primary();
    if s.is_empty() {
        app.toast("Select some text first");
        return;
    }
    match f(&d.buf.slice(s.start(), s.end())) {
        Ok(new) => {
            let n = new.chars().count();
            d.buf.edit(vec![Edit::new(s.start(), s.end(), new)], EditKind::Other);
            d.buf.set_sel(s.start(), s.start() + n);
            d.buf.commit_sels();
            d.edited_at = Some(now);
        }
        Err(e) => app.toast(e),
    }
}

fn replace_all(app: &mut InkApp, now: f64, f: impl Fn(&str) -> String) {
    let Some(d) = app.docs.get_mut(app.active) else { return };
    let text = d.buf.text();
    let new = f(&text);
    if new != text {
        d.buf.set_text(&new);
        d.edited_at = Some(now);
    } else {
        app.toast("Nothing to change");
    }
}

fn insert(app: &mut InkApp, now: f64, text: &str) {
    if let Some(d) = app.docs.get_mut(app.active) {
        crate::edit::exec(&mut d.buf, crate::edit::Cmd::Paste(text.to_string()), &d.cfg);
        d.edited_at = Some(now);
        if let Some(r) = &mut d.view.rich {
            r.resync = true;
        }
    }
}

pub fn command(app: &mut InkApp, ctx: &egui::Context, name: &str) {
    let now = ctx.input(|i| i.time);
    match name {
        "tool_word_count" => {
            let Some(d) = app.docs.get(app.active) else { return };
            let s = d.buf.primary();
            let (text, scope) = if s.is_empty() { (d.buf.text(), "Document") } else { (d.buf.slice(s.start(), s.end()), "Selection") };
            let words = text.split_whitespace().count();
            let chars = text.chars().count();
            let no_space = text.chars().filter(|c| !c.is_whitespace()).count();
            let paragraphs = text.split("\n\n").filter(|p| !p.trim().is_empty()).count();
            let sentences = text.split(['.', '!', '?']).filter(|p| p.trim().len() > 1).count();
            app.dialog = Some(Dialog::Message(
                format!("Word Count: {scope}"),
                format!("{words} words\n{chars} characters ({no_space} without spaces)\n{} lines\n{paragraphs} paragraphs\n{sentences} sentences\nAbout {} min to read", text.lines().count(), (words / 220).max(1)),
            ));
        }
        "tool_format_tables" => replace_all(app, now, align_tables),
        "tool_renumber" => replace_all(app, now, renumber_lists),
        "tool_smart_quotes" => replace_all(app, now, |t| t.replace(['\u{201c}', '\u{201d}'], "\"").replace(['\u{2018}', '\u{2019}'], "'").replace('\u{2026}', "...").replace('\u{2013}', "-").replace('\u{2014}', "--")),
        "tool_toc" => {
            let Some(d) = app.docs.get(app.active) else { return };
            let list = toc(&d.buf.text());
            if list.is_empty() {
                app.toast("No headings to list");
            } else {
                insert(app, now, &format!("{list}\n"));
            }
        }
        "tool_json_format" | "tool_to_json" | "tool_to_yaml" | "tool_to_toml" => {
            let Some(d) = app.docs.get(app.active) else { return };
            let Kind::Data(from) = d.kind else {
                app.toast("Open a JSON, YAML or TOML file first");
                return;
            };
            let to = match name {
                "tool_to_json" => "json",
                "tool_to_yaml" => "yaml",
                "tool_to_toml" => "toml",
                _ => from,
            };
            let result = data::parse(from, &d.buf.text()).map_err(|e| e.message).and_then(|v| data::emit(to, &v, false));
            match result {
                Ok(text) if to == from => {
                    if let Some(d) = app.doc() {
                        d.buf.set_text(&text);
                        d.edited_at = Some(now);
                    }
                }
                Ok(text) => {
                    let stem = d.name.rsplit_once('.').map(|x| x.0.to_string()).unwrap_or_else(|| d.name.clone());
                    app.new_file(Some(format!("{stem}.{to}")), &text);
                }
                Err(e) => app.toast(e),
            }
        }
        "tool_csv_to_table" => {
            let Some(d) = app.docs.get(app.active) else { return };
            let Kind::Table(delim) = d.kind else {
                app.toast("Open a CSV or TSV file first");
                return;
            };
            let md = csv_to_markdown(&d.buf.text(), delim);
            let stem = d.name.rsplit_once('.').map(|x| x.0.to_string()).unwrap_or_else(|| d.name.clone());
            app.new_file(Some(format!("{stem}.md")), &md);
        }
        "tool_base64_encode" => map_selection(app, now, |s| {
            use base64::Engine;
            Ok(base64::engine::general_purpose::STANDARD.encode(s))
        }),
        "tool_base64_decode" => map_selection(app, now, |s| {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD.decode(s.trim()).map_err(|_| "Selection is not valid Base64".to_string())?;
            String::from_utf8(bytes).map_err(|_| "Decoded data is not text".to_string())
        }),
        "tool_url_encode" => map_selection(app, now, |s| Ok(url_encode(s))),
        "tool_url_decode" => map_selection(app, now, |s| Ok(url_decode(s))),
        "tool_insert_date" => {
            let (y, m, d, h, mi) = now_local();
            let _ = (h, mi);
            insert(app, now, &format!("{y:04}-{m:02}-{d:02}"));
        }
        "tool_insert_uuid" => insert(app, now, &uuid4()),
        "tool_set_default" => crate::install::open_default_apps_settings_or_explain(app),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_tools() {
        assert_eq!(align_tables("| a | bbb |\n|---|:-:|\n| long cell | x |\n"), "| a         | bbb |\n| --------- | :-: |\n| long cell | x   |\n");
        assert_eq!(renumber_lists("3. a\n7. b\n   1. x\n   9. y\n2. c\n\ntext\n\n5. new\n"), "1. a\n2. b\n   1. x\n   2. y\n3. c\n\ntext\n\n1. new\n");
        assert_eq!(toc("# A\n\n## B c\n"), "- [A](#a)\n  - [B c](#b-c)");
        assert_eq!(csv_to_markdown("x,y\n1,2\n", ','), "| x   | y   |\n| --- | --- |\n| 1   | 2   |\n");
        assert_eq!(url_decode(&url_encode("a b/ü")), "a b/ü");
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_771), (2026, 11, 14));
        let u = uuid4();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
    }
}
