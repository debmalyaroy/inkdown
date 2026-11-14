//! Standalone HTML from the Markdown block model: used for HTML export, PDF, printing
//! and the rich-text clipboard. Diagrams, charts and math are embedded as images.

use std::path::Path;

use base64::Engine;

use crate::assets;
use crate::md::{AlertKind, Block, CellAlign, DiagramKind, Inl, Style, Top};
use crate::theme::Palette;

pub struct HtmlOpts<'a> {
    pub base: Option<&'a Path>,
    /// Rasterise vector graphics to PNG (Word and Google Docs do not paste SVG).
    pub png: bool,
    pub pal: &'a Palette,
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn png_bytes(r: &assets::Raster) -> Vec<u8> {
    // un-premultiply for the file
    let mut data = r.rgba.clone();
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new(&mut out);
    let _ = image::ImageEncoder::write_image(enc, &data, r.w as u32, r.h as u32, image::ExtendedColorType::Rgba8);
    out
}

/// `<img>` for an SVG, either inline as SVG or rasterised.
fn svg_img(svg: &str, opts: &HtmlOpts<'_>, alt: &str, style: &str) -> String {
    match assets::raster_svg(svg, if opts.png { 2.0 } else { 0.1 }, 100_000.0, opts.base) {
        Ok(r) => {
            let (w, h) = (r.size.x, r.size.y);
            let src = if opts.png { format!("data:image/png;base64,{}", b64(&png_bytes(&r))) } else { format!("data:image/svg+xml;base64,{}", b64(svg.as_bytes())) };
            format!(r#"<img src="{src}" alt="{}" width="{w:.0}" height="{h:.0}" style="max-width:100%;height:auto;{style}">"#, esc(alt))
        }
        Err(e) => format!("<pre>{}</pre>", esc(&e)),
    }
}

fn math_img(tex: &str, display: bool, opts: &HtmlOpts<'_>) -> String {
    let c = opts.pal.doc_text;
    match assets::math_svg(tex, display, if display { 19.0 } else { 17.0 }, c) {
        Ok((svg, w, h, depth)) => {
            let src = if opts.png {
                match assets::svg_tree(&svg, None) {
                    Ok(tree) => {
                        let scale = 3.0 * w / tree.size().width().max(0.01);
                        let (pw, ph) = (((w * 3.0).ceil() as u32).max(1), ((h * 3.0).ceil() as u32).max(1));
                        match resvg::tiny_skia::Pixmap::new(pw, ph) {
                            Some(mut pm) => {
                                resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pm.as_mut());
                                let r = assets::Raster { rgba: pm.take(), w: pw as usize, h: ph as usize, size: egui::vec2(w, h), depth, svg: None };
                                format!("data:image/png;base64,{}", b64(&png_bytes(&r)))
                            }
                            None => String::new(),
                        }
                    }
                    Err(_) => String::new(),
                }
            } else {
                format!("data:image/svg+xml;base64,{}", b64(svg.as_bytes()))
            };
            if display {
                format!(r#"<p style="text-align:center"><img src="{src}" alt="{}" width="{w:.1}" height="{h:.1}"></p>"#, esc(tex))
            } else {
                format!(r#"<img src="{src}" alt="{}" width="{w:.1}" height="{h:.1}" style="vertical-align:-{depth:.1}px">"#, esc(tex))
            }
        }
        Err(_) => format!("<code>{}</code>", esc(tex)),
    }
}

fn image_src(url: &str, opts: &HtmlOpts<'_>) -> String {
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("data:") {
        return url.to_string();
    }
    // local files are embedded so the HTML stands alone
    match assets::resolve_path(url, opts.base).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
        Some((p, bytes)) if bytes.len() < 12 << 20 => {
            let mime = match p.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
                Some("png") => "image/png",
                Some("jpg" | "jpeg") => "image/jpeg",
                Some("gif") => "image/gif",
                Some("webp") => "image/webp",
                Some("svg") => "image/svg+xml",
                _ => "application/octet-stream",
            };
            format!("data:{mime};base64,{}", b64(&bytes))
        }
        _ => url.to_string(),
    }
}

fn inlines(inl: &[Inl], opts: &HtmlOpts<'_>, out: &mut String) {
    for i in inl {
        match i {
            Inl::Text(t, st) => styled(t, st, out),
            Inl::Break => out.push_str("<br>\n"),
            Inl::Math(tex) => out.push_str(&math_img(tex, false, opts)),
            Inl::FootRef(_, ix) => out.push_str(&format!("<sup><a href=\"#fn-{ix}\">[{ix}]</a></sup>")),
            Inl::Image { url, alt, title, link, width } => {
                let img = format!(r#"<img src="{}" alt="{}"{}{} style="max-width:100%">"#, esc(&image_src(url, opts)), esc(alt), if title.is_empty() { String::new() } else { format!(" title=\"{}\"", esc(title)) }, width.map(|w| format!(" width=\"{w}\"")).unwrap_or_default());
                match link {
                    Some(l) => out.push_str(&format!("<a href=\"{}\">{img}</a>", esc(l))),
                    None => out.push_str(&img),
                }
            }
        }
    }
}

fn styled(t: &str, st: &Style, out: &mut String) {
    let tags: [(bool, &str); 9] = [(st.bold, "strong"), (st.italic, "em"), (st.strike, "del"), (st.underline, "u"), (st.mark, "mark"), (st.sup, "sup"), (st.sub, "sub"), (st.kbd, "kbd"), (st.code, "code")];
    if let Some(l) = &st.link {
        out.push_str(&format!("<a href=\"{}\">", esc(l)));
    }
    for (on, tag) in tags {
        if on {
            out.push_str(&format!("<{tag}>"));
        }
    }
    out.push_str(&esc(t));
    for (on, tag) in tags.iter().rev() {
        if *on {
            out.push_str(&format!("</{tag}>"));
        }
    }
    if st.link.is_some() {
        out.push_str("</a>");
    }
}

fn code_html(lang: &str, text: &str, pal: &Palette) -> String {
    let mut out = String::from("<pre><code>");
    let spans = crate::syntax::highlight_snippet(lang, text);
    for (line, sp) in text.split_inclusive('\n').zip(spans.iter()) {
        let mut last = 0usize;
        for (end, tok) in sp.iter() {
            let end = (*end as usize).min(line.len());
            if end > last && line.is_char_boundary(last) && line.is_char_boundary(end) {
                let c = pal.token(*tok);
                if *tok == 0 {
                    out.push_str(&esc(&line[last..end]));
                } else {
                    out.push_str(&format!("<span style=\"color:#{:02x}{:02x}{:02x}\">{}</span>", c.r(), c.g(), c.b(), esc(&line[last..end])));
                }
                last = end;
            }
        }
    }
    out.push_str("</code></pre>\n");
    out
}

pub fn block(b: &Block, opts: &HtmlOpts<'_>, out: &mut String) {
    match b {
        Block::Heading { level, inl, id } => {
            out.push_str(&format!("<h{level} id=\"{}\">", esc(id)));
            inlines(inl, opts, out);
            out.push_str(&format!("</h{level}>\n"));
        }
        Block::Para(inl) => {
            out.push_str("<p>");
            inlines(inl, opts, out);
            out.push_str("</p>\n");
        }
        Block::Code { lang, text } => out.push_str(&code_html(lang, text, opts.pal)),
        Block::Quote(body) => {
            out.push_str("<blockquote>\n");
            blocks(body, opts, out);
            out.push_str("</blockquote>\n");
        }
        Block::Alert { kind, title, body } => {
            let (class, name) = match kind {
                AlertKind::Note => ("note", "Note"),
                AlertKind::Tip => ("tip", "Tip"),
                AlertKind::Important => ("important", "Important"),
                AlertKind::Warning => ("warning", "Warning"),
                AlertKind::Caution => ("caution", "Caution"),
            };
            out.push_str(&format!("<div class=\"markdown-alert markdown-alert-{class}\"><p class=\"markdown-alert-title\">{}</p>\n", esc(title.as_deref().unwrap_or(name))));
            blocks(body, opts, out);
            out.push_str("</div>\n");
        }
        Block::List { ordered, start, items, .. } => {
            let tag = if *ordered { "ol" } else { "ul" };
            let tasks = items.iter().any(|i| i.task.is_some());
            out.push_str(&format!("<{tag}{}{}>\n", if *ordered && *start != 1 { format!(" start=\"{start}\"") } else { String::new() }, if tasks { " class=\"contains-task-list\"" } else { "" }));
            for item in items {
                match item.task {
                    Some(done) => out.push_str(&format!("<li class=\"task-list-item\"><input type=\"checkbox\" disabled{}> ", if done { " checked" } else { "" })),
                    None => out.push_str("<li>"),
                }
                // tight lists keep a lone paragraph inline
                match item.blocks.as_slice() {
                    [Block::Para(inl)] => inlines(inl, opts, out),
                    [Block::Para(inl), rest @ ..] => {
                        inlines(inl, opts, out);
                        out.push('\n');
                        blocks(rest, opts, out);
                    }
                    other => blocks(other, opts, out),
                }
                out.push_str("</li>\n");
            }
            out.push_str(&format!("</{tag}>\n"));
        }
        Block::Table { aligns, rows } => {
            out.push_str("<table>\n");
            for (ri, row) in rows.iter().enumerate() {
                out.push_str(if ri == 0 { "<thead><tr>" } else if ri == 1 { "<tbody><tr>" } else { "<tr>" });
                for (ci, cell) in row.iter().enumerate() {
                    let tag = if ri == 0 { "th" } else { "td" };
                    let align = match aligns.get(ci) {
                        Some(CellAlign::Center) => " style=\"text-align:center\"",
                        Some(CellAlign::Right) => " style=\"text-align:right\"",
                        _ => "",
                    };
                    out.push_str(&format!("<{tag}{align}>"));
                    inlines(cell, opts, out);
                    out.push_str(&format!("</{tag}>"));
                }
                out.push_str(if ri == 0 { "</tr></thead>\n" } else { "</tr>\n" });
            }
            out.push_str(if rows.len() > 1 { "</tbody></table>\n" } else { "</table>\n" });
        }
        Block::Rule => out.push_str("<hr>\n"),
        Block::Math(tex) => out.push_str(&math_img(tex, true, opts)),
        Block::Diagram { kind, src } => {
            let svg = match kind {
                DiagramKind::Mermaid => assets::mermaid_svg(src, false),
                DiagramKind::Dot => assets::dot_svg(src),
                DiagramKind::Svg => Ok(src.clone()),
                DiagramKind::Chart => crate::chart::chart_svg(src, false, 720.0),
            };
            match svg {
                Ok(svg) => out.push_str(&format!("<p style=\"text-align:center\">{}</p>\n", svg_img(&svg, opts, "diagram", ""))),
                Err(e) => out.push_str(&format!("<pre>{}</pre>\n", esc(&format!("{e}\n\n{src}")))),
            }
        }
        Block::Details { summary, body } => {
            out.push_str("<details open><summary>");
            inlines(summary, opts, out);
            out.push_str("</summary>\n");
            blocks(body, opts, out);
            out.push_str("</details>\n");
        }
        Block::Footnote { index, body, .. } => {
            out.push_str(&format!("<div class=\"footnote\" id=\"fn-{index}\"><sup>{index}</sup> "));
            match body.as_slice() {
                [Block::Para(inl)] => inlines(inl, opts, out),
                other => blocks(other, opts, out),
            }
            out.push_str("</div>\n");
        }
        Block::DefList(items) => {
            out.push_str("<dl>\n");
            for (term, defs) in items {
                out.push_str("<dt>");
                inlines(term, opts, out);
                out.push_str("</dt>\n");
                for d in defs {
                    out.push_str("<dd>");
                    match d.as_slice() {
                        [Block::Para(inl)] => inlines(inl, opts, out),
                        other => blocks(other, opts, out),
                    }
                    out.push_str("</dd>\n");
                }
            }
            out.push_str("</dl>\n");
        }
        Block::FrontMatter(_) => {}
        Block::Center(body) => {
            out.push_str("<div style=\"text-align:center\">\n");
            blocks(body, opts, out);
            out.push_str("</div>\n");
        }
        Block::Image { url, alt, title, link, width } => {
            let img = format!(r#"<img src="{}" alt="{}"{} style="max-width:100%">"#, esc(&image_src(url, opts)), esc(alt), width.map(|w| format!(" width=\"{w}\"")).unwrap_or_default());
            let img = match link {
                Some(l) => format!("<a href=\"{}\">{img}</a>", esc(l)),
                None => img,
            };
            if title.is_empty() {
                out.push_str(&format!("<p>{img}</p>\n"));
            } else {
                out.push_str(&format!("<figure>{img}<figcaption>{}</figcaption></figure>\n", esc(title)));
            }
        }
    }
}

pub fn blocks(list: &[Block], opts: &HtmlOpts<'_>, out: &mut String) {
    for b in list {
        block(b, opts, out);
    }
}

pub fn body(tops: &[Top], opts: &HtmlOpts<'_>) -> String {
    let mut out = String::new();
    for t in tops {
        block(&t.block, opts, &mut out);
    }
    out
}

pub fn title_of(tops: &[Top], fallback: &str) -> String {
    for t in tops {
        match &t.block {
            Block::FrontMatter(rows) => {
                if let Some((_, v)) = rows.iter().find(|r| r.0.eq_ignore_ascii_case("title")) {
                    return v.clone();
                }
            }
            Block::Heading { level: 1, inl, .. } => return crate::md::plain(inl),
            _ => {}
        }
    }
    fallback.to_string()
}

const CSS: &str = r#"
body{margin:0;background:#fff;color:#1f2328;font:16px/1.7 "Segoe UI Variable Text","Segoe UI",system-ui,-apple-system,"Helvetica Neue",Arial,sans-serif}
main{max-width:820px;margin:0 auto;padding:48px 32px 96px}
h1,h2,h3,h4,h5,h6{line-height:1.3;margin:1.6em 0 .6em;font-weight:650}
h1{font-size:2.05em;letter-spacing:-.02em;padding-bottom:.3em;border-bottom:1px solid #eaecf0}
h2{font-size:1.55em;padding-bottom:.25em;border-bottom:1px solid #eaecf0}
h3{font-size:1.27em}h4{font-size:1.1em}h6{font-size:.88em;color:#656d76;text-transform:uppercase;letter-spacing:.04em}
p,ul,ol,table,pre,blockquote,dl,figure{margin:0 0 1em}
a{color:#5b5fe9;text-decoration:none}a:hover{text-decoration:underline}
code,pre,kbd{font-family:"Cascadia Mono","Cascadia Code",Consolas,"JetBrains Mono",ui-monospace,monospace}
code{font-size:.88em;background:#f6f8fa;padding:.15em .35em;border-radius:5px}
pre{background:#f6f8fa;border:1px solid #e6e9ee;border-radius:8px;padding:14px 16px;overflow:auto;line-height:1.55}
pre code{background:none;padding:0;font-size:.86em}
kbd{font-size:.8em;font-weight:600;padding:.18em .5em;border-radius:6px;background:#f6f7f9;border:1px solid #d9dde3;border-bottom-width:2px}
mark{background:#fff3b0;padding:.05em .25em;border-radius:4px}
blockquote{margin-left:0;padding:.2em 1.1em;color:#656d76;border-left:3px solid #d9dde3}
hr{border:0;height:1px;background:#d9dde3;margin:2.2em 0}
table{border-collapse:collapse;display:block;overflow:auto}
th,td{border:1px solid #d9dde3;padding:.42em .8em;text-align:left}
th{background:#f6f7f9;font-weight:650}tr:nth-child(even) td{background:#fafbfc}
img{max-width:100%}figure{text-align:center}figcaption{color:#656d76;font-size:.88em}
ul.contains-task-list{list-style:none;padding-left:1.4em}
li.task-list-item input{margin:0 .5em 0 -1.4em}
.markdown-alert{border-left:3px solid #2f6feb;background:#f3f7ff;border-radius:8px;padding:.7em 1em;margin:0 0 1em}
.markdown-alert p:last-child{margin-bottom:0}
.markdown-alert-title{font-weight:650;color:#2f6feb;margin-bottom:.3em}
.markdown-alert-tip{border-color:#1a7f37;background:#f1f9f3}.markdown-alert-tip .markdown-alert-title{color:#1a7f37}
.markdown-alert-important{border-color:#8250df;background:#f7f3ff}.markdown-alert-important .markdown-alert-title{color:#8250df}
.markdown-alert-warning{border-color:#9a6700;background:#fff8e8}.markdown-alert-warning .markdown-alert-title{color:#9a6700}
.markdown-alert-caution{border-color:#cf222e;background:#fff3f3}.markdown-alert-caution .markdown-alert-title{color:#cf222e}
.footnote{font-size:.9em;color:#656d76;margin:.3em 0}
dt{font-weight:650}dd{margin:.2em 0 .6em 1.5em;color:#656d76}
@media print{main{padding:0;max-width:none}pre,table,img,.markdown-alert{break-inside:avoid}h1,h2,h3{break-after:avoid}a{color:inherit}}
@page{margin:18mm 16mm}
"#;

/// A complete, self-contained HTML page.
pub fn document(tops: &[Top], title: &str, opts: &HtmlOpts<'_>) -> String {
    format!("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"Inkdown\">\n<title>{}</title>\n<style>{CSS}</style>\n</head>\n<body>\n<main class=\"markdown-body\">\n{}</main>\n</body>\n</html>\n", esc(title), body(tops, opts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_html() {
        let pal = crate::theme::palette("light", false);
        let tops = crate::md::parse("---\ntitle: My Doc\n---\n\n# Head\n\nSome **bold** & <kbd>K</kbd> and $x^2$.\n\n- [x] a\n- [ ] b\n\n| A | B |\n|---|--:|\n| 1 | 2 |\n\n> [!WARNING]\n> Hot\n\n```mermaid\ngraph LR; A-->B\n```\n\n```rust\nfn main() {}\n```\n");
        let html = document(&tops, &title_of(&tops, "x"), &HtmlOpts { base: None, png: false, pal: &pal });
        assert!(html.contains("<title>My Doc</title>"));
        assert!(html.contains("<h1 id=\"head\">Head</h1>"));
        assert!(html.contains("<strong>bold</strong> &amp; <kbd>K</kbd>"));
        assert!(html.contains("data:image/svg+xml;base64,"), "math and diagrams are embedded");
        assert!(html.contains("<li class=\"task-list-item\"><input type=\"checkbox\" disabled checked> a</li>"));
        assert!(html.contains("<td style=\"text-align:right\">2</td>"));
        assert!(html.contains("markdown-alert-warning"));
        assert!(html.contains("<pre><code><span style=\"color:#"));
        let png = body(&tops, &HtmlOpts { base: None, png: true, pal: &pal });
        assert!(png.contains("data:image/png;base64,") && !png.contains("svg+xml"));
    }
}
