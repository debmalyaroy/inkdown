//! Markdown -> HTML rendering (CommonMark + GitHub Flavored Markdown + extras) via comrak.

use comrak::nodes::NodeValue;
use comrak::{format_html, parse_document, Arena, Options};

pub struct Rendered {
    pub html: String,
    /// Raw front matter block (including delimiters), if the document has one.
    pub front_matter: Option<String>,
}

pub fn options() -> Options<'static> {
    let mut o = Options::default();

    let e = &mut o.extension;
    e.strikethrough = true;
    e.table = true;
    e.autolink = true;
    e.tasklist = true;
    e.superscript = true;
    e.subscript = true;
    e.header_id_prefix = Some(String::new());
    e.footnotes = true;
    e.inline_footnotes = true;
    e.description_lists = true;
    e.front_matter_delimiter = Some("---".to_string());
    e.multiline_block_quotes = true;
    e.alerts = true;
    e.math_dollars = true;
    e.math_code = true;
    e.shortcodes = true;
    e.wikilinks_title_after_pipe = true;
    e.highlight = true;
    e.insert = true;

    let p = &mut o.parse;
    p.relaxed_tasklist_matching = true;
    p.relaxed_autolinks = true;
    p.tasklist_in_table = true;

    let r = &mut o.render;
    // Raw HTML is passed through here and sanitised in the webview (DOMPurify)
    // before it ever touches the DOM.
    r.r#unsafe = true;
    r.sourcepos = true;
    r.tasklist_classes = true;
    r.figure_with_caption = true;

    o
}

pub fn render(md: &str) -> Rendered {
    let opts = options();
    let arena = Arena::new();
    let root = parse_document(&arena, md, &opts);

    let mut front_matter = None;
    for child in root.children() {
        if let NodeValue::FrontMatter(ref fm) = child.data.borrow().value {
            front_matter = Some(fm.clone());
            break;
        }
    }

    let mut html = String::with_capacity(md.len() * 2);
    let _ = format_html(root, &opts, &mut html);
    Rendered { html, front_matter }
}

/// Inline tags the rich editor understands natively; anything else is kept
/// verbatim in an opaque node so that it survives a round trip.
const RICH_INLINE_TAGS: &[&str] = &["br", "kbd", "u", "ins", "sub", "sup", "mark", "del", "s", "b", "strong", "i", "em"];

fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// HTML for the WYSIWYG editor: no source positions or generated ids, and
/// constructs the editor cannot model (footnotes, wikilinks, raw HTML, ...) are
/// left as literal text or wrapped in `data-raw-html` carriers.
pub fn render_rich(md: &str) -> Rendered {
    let mut opts = options();
    opts.extension.header_id_prefix = None;
    opts.extension.footnotes = false;
    opts.extension.inline_footnotes = false;
    opts.extension.shortcodes = false;
    opts.extension.description_lists = false;
    opts.extension.wikilinks_title_after_pipe = false;
    opts.render.sourcepos = false;
    opts.render.figure_with_caption = false;

    let arena = Arena::new();
    let root = parse_document(&arena, md, &opts);
    let mut front_matter = None;
    for node in root.descendants() {
        let mut data = node.data.borrow_mut();
        if matches!(data.value, NodeValue::SoftBreak) {
            // Keep the author's line breaks so that editing does not reflow paragraphs.
            data.value = NodeValue::HtmlInline("<span data-soft-break></span>".to_string());
            continue;
        }
        match &mut data.value {
            NodeValue::FrontMatter(fm) => front_matter = Some(fm.clone()),
            NodeValue::HtmlBlock(block) => {
                block.literal = format!("<div data-raw-html=\"{}\"></div>\n", attr_escape(block.literal.trim_end()));
            }
            NodeValue::HtmlInline(tag) => {
                let name: String = tag
                    .trim_start_matches("</")
                    .trim_start_matches('<')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect::<String>()
                    .to_ascii_lowercase();
                let plain = !tag[1..].contains('<') && !tag.contains('=');
                if !(plain && RICH_INLINE_TAGS.contains(&name.as_str())) {
                    *tag = format!("<span data-raw-html=\"{}\"></span>", attr_escape(tag));
                }
            }
            _ => {}
        }
    }
    let mut html = String::with_capacity(md.len() * 2);
    let _ = format_html(root, &opts, &mut html);
    Rendered { html, front_matter }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_gfm_and_extras() {
        let r = render(
            "---\ntitle: T\n---\n\n# Hi\n\n- [x] done\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n$x^2$\n\n> [!NOTE]\n> hello\n\n```mermaid\ngraph TD;A-->B\n```\n",
        );
        assert!(r.front_matter.unwrap().contains("title: T"));
        assert!(r.html.contains("<table"));
        assert!(r.html.contains("data-sourcepos"));
        assert!(r.html.contains("checkbox"));
        assert!(r.html.contains("data-math-style"));
        assert!(r.html.contains("markdown-alert"));
        assert!(r.html.contains("language-mermaid"));
        assert!(r.html.contains("id=\"hi\""));
    }
}
