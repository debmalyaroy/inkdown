//! Colour palettes, fonts and the egui style built from them.

use std::sync::Arc;

use egui::epaint::text::{FontData, FontDefinitions, FontTweak, VariationCoords};
use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextFormat};

pub const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub fn with_alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

/// Linear blend of two opaque colours (t = 0 gives `a`).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

#[derive(Clone, Copy, PartialEq)]
pub struct Syn {
    pub keyword: Color32,
    pub string: Color32,
    pub comment: Color32,
    pub number: Color32,
    pub func: Color32,
    pub typ: Color32,
    pub tag: Color32,
    pub attr: Color32,
    pub constant: Color32,
    pub punct: Color32,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Palette {
    pub id: &'static str,
    pub name: &'static str,
    pub dark: bool,
    pub bg: Color32,
    pub chrome: Color32,
    pub chrome2: Color32,
    pub elev: Color32,
    pub border: Color32,
    pub border_soft: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent2: Color32,
    pub danger: Color32,
    pub desk: Color32,
    pub doc_bg: Color32,
    pub doc_text: Color32,
    pub doc_subtle: Color32,
    pub doc_code: Color32,
    pub doc_border: Color32,
    pub mark: Color32,
    pub note: Color32,
    pub tip: Color32,
    pub important: Color32,
    pub warning: Color32,
    pub caution: Color32,
    pub syn: Syn,
}

impl Palette {
    pub fn accent_soft(&self) -> Color32 {
        mix(self.bg, self.accent, if self.dark { 0.2 } else { 0.13 })
    }
    pub fn hover(&self) -> Color32 {
        mix(self.chrome, self.text, if self.dark { 0.09 } else { 0.07 })
    }
    pub fn active(&self) -> Color32 {
        mix(self.chrome, self.text, if self.dark { 0.15 } else { 0.12 })
    }
    pub fn selection(&self) -> Color32 {
        mix(self.bg, self.accent, if self.dark { 0.34 } else { 0.24 })
    }
    pub fn line_hl(&self) -> Color32 {
        mix(self.bg, self.text, if self.dark { 0.045 } else { 0.035 })
    }
    pub fn on_accent(&self) -> Color32 {
        if self.dark { rgb(0x14151a) } else { Color32::WHITE }
    }
}

const SYN_LIGHT: Syn = Syn {
    keyword: rgb(0x8250df),
    string: rgb(0x1a7f37),
    comment: rgb(0x8c959f),
    number: rgb(0x9a6700),
    func: rgb(0x2f6feb),
    typ: rgb(0x0e7c86),
    tag: rgb(0xcf4a22),
    attr: rgb(0x0e7c86),
    constant: rgb(0x9a6700),
    punct: rgb(0x57606a),
};
const SYN_DARK: Syn = Syn {
    keyword: rgb(0xd49bf5),
    string: rgb(0x7ee2a8),
    comment: rgb(0x7d8794),
    number: rgb(0xe8b964),
    func: rgb(0x7fb4ff),
    typ: rgb(0x67d4e8),
    tag: rgb(0xff9e7a),
    attr: rgb(0x67d4e8),
    constant: rgb(0xe8b964),
    punct: rgb(0xa9b2be),
};

const LIGHT: Palette = Palette {
    id: "light",
    name: "Light",
    dark: false,
    bg: rgb(0xffffff),
    chrome: rgb(0xf4f5f8),
    chrome2: rgb(0xeceef2),
    elev: rgb(0xffffff),
    border: rgb(0xdfe2e8),
    border_soft: rgb(0xe9ebf0),
    text: rgb(0x1f2328),
    muted: rgb(0x68707c),
    faint: rgb(0x9aa1ac),
    accent: rgb(0x5b5fe9),
    accent2: rgb(0xa855f7),
    danger: rgb(0xcf222e),
    desk: rgb(0xeceef2),
    doc_bg: rgb(0xffffff),
    doc_text: rgb(0x1f2328),
    doc_subtle: rgb(0xf6f7f9),
    doc_code: rgb(0xf6f8fa),
    doc_border: rgb(0xd9dde3),
    mark: rgb(0xfff3b0),
    note: rgb(0x2f6feb),
    tip: rgb(0x1a7f37),
    important: rgb(0x8250df),
    warning: rgb(0x9a6700),
    caution: rgb(0xcf222e),
    syn: SYN_LIGHT,
};

const DARK: Palette = Palette {
    id: "dark",
    name: "Dark",
    dark: true,
    bg: rgb(0x16181d),
    chrome: rgb(0x1b1e24),
    chrome2: rgb(0x22262e),
    elev: rgb(0x242830),
    border: rgb(0x30353f),
    border_soft: rgb(0x272c35),
    text: rgb(0xe3e6eb),
    muted: rgb(0x9aa3af),
    faint: rgb(0x6b7380),
    accent: rgb(0x9698fb),
    accent2: rgb(0xc084fc),
    danger: rgb(0xf85149),
    desk: rgb(0x0f1115),
    doc_bg: rgb(0x16181d),
    doc_text: rgb(0xe3e6eb),
    doc_subtle: rgb(0x1c1f26),
    doc_code: rgb(0x1e222a),
    doc_border: rgb(0x333944),
    mark: rgb(0x5c4b00),
    note: rgb(0x58a6ff),
    tip: rgb(0x3fb950),
    important: rgb(0xa371f7),
    warning: rgb(0xd29922),
    caution: rgb(0xf85149),
    syn: SYN_DARK,
};

pub const PALETTES: &[Palette] = &[
    LIGHT,
    Palette {
        id: "sepia",
        name: "Sepia",
        bg: rgb(0xfbf5e9),
        chrome: rgb(0xf3ead8),
        chrome2: rgb(0xeadfc9),
        elev: rgb(0xfffaf0),
        border: rgb(0xe0d3b8),
        border_soft: rgb(0xeadfc9),
        text: rgb(0x3b3024),
        muted: rgb(0x7a6a55),
        faint: rgb(0xa89a84),
        accent: rgb(0xb5622a),
        accent2: rgb(0xc98a2b),
        desk: rgb(0xefe5d0),
        doc_bg: rgb(0xfbf5e9),
        doc_text: rgb(0x3b3024),
        doc_subtle: rgb(0xf3ead8),
        doc_code: rgb(0xf3ead8),
        doc_border: rgb(0xdccfb4),
        mark: rgb(0xf6dc8a),
        syn: Syn { keyword: rgb(0x9c4a8b), string: rgb(0x4f7a28), comment: rgb(0xa89a84), number: rgb(0xa8641a), func: rgb(0x2f6aa8), typ: rgb(0x1f7a78), tag: rgb(0xb5622a), attr: rgb(0x1f7a78), constant: rgb(0xa8641a), punct: rgb(0x6b5c48) },
        ..LIGHT
    },
    Palette {
        id: "solarized",
        name: "Solarized Light",
        bg: rgb(0xfdf6e3),
        chrome: rgb(0xeee8d5),
        chrome2: rgb(0xe6dfc8),
        elev: rgb(0xfffbee),
        border: rgb(0xddd6c1),
        border_soft: rgb(0xe8e1cd),
        text: rgb(0x47585e),
        muted: rgb(0x6c7f84),
        faint: rgb(0x93a1a1),
        accent: rgb(0x268bd2),
        accent2: rgb(0x2aa198),
        desk: rgb(0xeee8d5),
        doc_bg: rgb(0xfdf6e3),
        doc_text: rgb(0x47585e),
        doc_subtle: rgb(0xeee8d5),
        doc_code: rgb(0xeee8d5),
        doc_border: rgb(0xd8d1bb),
        mark: rgb(0xf3e3a3),
        syn: Syn { keyword: rgb(0x859900), string: rgb(0x2aa198), comment: rgb(0x93a1a1), number: rgb(0xd33682), func: rgb(0x268bd2), typ: rgb(0xb58900), tag: rgb(0xcb4b16), attr: rgb(0xb58900), constant: rgb(0xd33682), punct: rgb(0x657b83) },
        ..LIGHT
    },
    Palette {
        id: "mint",
        name: "Mint",
        bg: rgb(0xf6fbf8),
        chrome: rgb(0xe8f3ed),
        chrome2: rgb(0xdcebe2),
        elev: rgb(0xffffff),
        border: rgb(0xcfe2d7),
        border_soft: rgb(0xdfece4),
        text: rgb(0x1c2b24),
        muted: rgb(0x56695f),
        faint: rgb(0x8ba095),
        accent: rgb(0x1f8a5b),
        accent2: rgb(0x0e9aa7),
        desk: rgb(0xe3efe8),
        doc_bg: rgb(0xf9fdfb),
        doc_text: rgb(0x1c2b24),
        doc_subtle: rgb(0xedf6f1),
        doc_code: rgb(0xedf6f1),
        doc_border: rgb(0xcfe2d7),
        ..LIGHT
    },
    DARK,
    Palette {
        id: "nord",
        name: "Nord",
        bg: rgb(0x2e3440),
        chrome: rgb(0x292e39),
        chrome2: rgb(0x3b4252),
        elev: rgb(0x3b4252),
        border: rgb(0x434c5e),
        border_soft: rgb(0x3b4252),
        text: rgb(0xeceff4),
        muted: rgb(0xa7b1c2),
        faint: rgb(0x7b8698),
        accent: rgb(0x88c0d0),
        accent2: rgb(0xb48ead),
        desk: rgb(0x262b35),
        doc_bg: rgb(0x2e3440),
        doc_text: rgb(0xe5e9f0),
        doc_subtle: rgb(0x353c4a),
        doc_code: rgb(0x3b4252),
        doc_border: rgb(0x4c566a),
        syn: Syn { keyword: rgb(0x81a1c1), string: rgb(0xa3be8c), comment: rgb(0x7b88a1), number: rgb(0xb48ead), func: rgb(0x88c0d0), typ: rgb(0x8fbcbb), tag: rgb(0x81a1c1), attr: rgb(0x8fbcbb), constant: rgb(0xd08770), punct: rgb(0xc2c9d6) },
        ..DARK
    },
    Palette {
        id: "dracula",
        name: "Dracula",
        bg: rgb(0x282a36),
        chrome: rgb(0x21222c),
        chrome2: rgb(0x343746),
        elev: rgb(0x343746),
        border: rgb(0x44475a),
        border_soft: rgb(0x383a4a),
        text: rgb(0xf8f8f2),
        muted: rgb(0xb4b7c9),
        faint: rgb(0x7f8399),
        accent: rgb(0xbd93f9),
        accent2: rgb(0xff79c6),
        desk: rgb(0x1e1f29),
        doc_bg: rgb(0x282a36),
        doc_text: rgb(0xf8f8f2),
        doc_subtle: rgb(0x2f3140),
        doc_code: rgb(0x21222c),
        doc_border: rgb(0x4d5066),
        syn: Syn { keyword: rgb(0xff79c6), string: rgb(0xf1fa8c), comment: rgb(0x6272a4), number: rgb(0xbd93f9), func: rgb(0x50fa7b), typ: rgb(0x8be9fd), tag: rgb(0xff79c6), attr: rgb(0x50fa7b), constant: rgb(0xbd93f9), punct: rgb(0xd6d6cf) },
        ..DARK
    },
    Palette {
        id: "solarized-dark",
        name: "Solarized Dark",
        bg: rgb(0x002b36),
        chrome: rgb(0x00252f),
        chrome2: rgb(0x073642),
        elev: rgb(0x073642),
        border: rgb(0x0f4958),
        border_soft: rgb(0x073642),
        text: rgb(0xd6dccf),
        muted: rgb(0x93a1a1),
        faint: rgb(0x657b83),
        accent: rgb(0x2aa198),
        accent2: rgb(0x268bd2),
        desk: rgb(0x00212b),
        doc_bg: rgb(0x002b36),
        doc_text: rgb(0xd6dccf),
        doc_subtle: rgb(0x073642),
        doc_code: rgb(0x073642),
        doc_border: rgb(0x175565),
        syn: Syn { keyword: rgb(0x859900), string: rgb(0x2aa198), comment: rgb(0x657b83), number: rgb(0xd33682), func: rgb(0x268bd2), typ: rgb(0xb58900), tag: rgb(0xcb4b16), attr: rgb(0xb58900), constant: rgb(0xd33682), punct: rgb(0x93a1a1) },
        ..DARK
    },
    Palette {
        id: "black",
        name: "Black",
        bg: rgb(0x000000),
        chrome: rgb(0x0a0a0a),
        chrome2: rgb(0x1a1a1a),
        elev: rgb(0x141414),
        border: rgb(0x3a3a3a),
        border_soft: rgb(0x262626),
        text: rgb(0xffffff),
        muted: rgb(0xc8c8c8),
        faint: rgb(0x8c8c8c),
        accent: rgb(0x4cc2ff),
        accent2: rgb(0xffd866),
        desk: rgb(0x000000),
        doc_bg: rgb(0x000000),
        doc_text: rgb(0xffffff),
        doc_subtle: rgb(0x111111),
        doc_code: rgb(0x121212),
        doc_border: rgb(0x4a4a4a),
        ..DARK
    },
];

pub fn palette(id: &str, system_dark: bool) -> Palette {
    PALETTES.iter().copied().find(|p| p.id == id).unwrap_or(if system_dark { DARK } else { LIGHT })
}

// ------------------------------------------------------------------ fonts

pub const ITALIC: &str = "italic";

pub fn italic_family() -> FontFamily {
    FontFamily::Name(ITALIC.into())
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mono_tweak = FontTweak { tab_size: 4.0, ..Default::default() };
    defs.font_data.insert("Inter".into(), Arc::new(FontData::from_static(include_bytes!("../fonts/Inter.ttf"))));
    defs.font_data.insert("Inter-Italic".into(), Arc::new(FontData::from_static(include_bytes!("../fonts/Inter-Italic.ttf"))));
    defs.font_data.insert("JetBrainsMono".into(), Arc::new(FontData::from_static(include_bytes!("../fonts/JetBrainsMono.ttf")).tweak(mono_tweak)));
    defs.font_data.insert("Lucide".into(), Arc::new(FontData::from_static(include_bytes!("../fonts/icons.ttf")).tweak(FontTweak { y_offset_factor: 0.06, ..Default::default() })));
    let mut fallback: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fallback.retain(|f| f != "Ubuntu-Light" && f != "Hack");
    fallback.insert(0, "Lucide".into());
    let chain = |first: &[&str]| -> Vec<String> { first.iter().map(|s| s.to_string()).chain(fallback.iter().cloned()).collect() };
    defs.families.insert(FontFamily::Proportional, chain(&["Inter"]));
    defs.families.insert(FontFamily::Monospace, chain(&["JetBrainsMono", "Inter"]));
    defs.families.insert(italic_family(), chain(&["Inter-Italic", "Inter"]));
    ctx.set_fonts(defs);
}

pub fn weight(w: f32) -> VariationCoords {
    VariationCoords::new([("wght", w)])
}

/// A text format in the UI font at the given weight.
pub fn fmt(size: f32, color: Color32, w: f32) -> TextFormat {
    TextFormat { font_id: FontId::proportional(size), color, coords: weight(w), ..Default::default() }
}

pub fn mono(size: f32, color: Color32) -> TextFormat {
    TextFormat { font_id: FontId::monospace(size), color, ..Default::default() }
}

/// Lay out a single run of UI text.
pub fn text(ui: &egui::Ui, s: &str, size: f32, color: Color32, w: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.append(s, 0.0, fmt(size, color, w));
    job.wrap.max_width = f32::INFINITY;
    ui.fonts_mut(|f| f.layout_job(job))
}

// ------------------------------------------------------------------ style

/// Re-apply the style only when the palette changed.
pub fn apply_if_changed(ctx: &egui::Context, p: &Palette, epoch: u64) {
    static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if LAST.swap(epoch, std::sync::atomic::Ordering::Relaxed) != epoch {
        apply(ctx, p);
    }
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.override_text_color = Some(p.text);
    v.panel_fill = p.chrome;
    v.window_fill = p.elev;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.chrome2;
    v.code_bg_color = p.doc_code;
    v.hyperlink_color = p.accent;
    v.error_fg_color = p.danger;
    v.warn_fg_color = p.warning;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = egui::Shadow { offset: [0, 10], blur: 34, spread: 0, color: Color32::from_black_alpha(if p.dark { 130 } else { 46 }) };
    v.popup_shadow = egui::Shadow { offset: [0, 6], blur: 22, spread: 0, color: Color32::from_black_alpha(if p.dark { 120 } else { 38 }) };
    v.selection.bg_fill = p.selection();
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.text_cursor.stroke = Stroke::new(1.6, p.accent);
    let r = CornerRadius::same(6);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.chrome;
    w.noninteractive.weak_bg_fill = p.chrome;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border_soft);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = p.chrome2;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = p.hover();
    w.hovered.weak_bg_fill = p.hover();
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = p.active();
    w.active.weak_bg_fill = p.active();
    w.active.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;
    w.open.bg_fill = p.active();
    w.open.weak_bg_fill = p.active();
    w.open.bg_stroke = Stroke::NONE;
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.corner_radius = r;
    ctx.set_visuals(v);

    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 5.0);
        s.spacing.button_padding = egui::vec2(9.0, 5.0);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.window_margin = egui::Margin::same(14);
        s.spacing.scroll = egui::style::ScrollStyle { bar_width: 9.0, floating: true, ..egui::style::ScrollStyle::floating() };
        s.interaction.selectable_labels = false;
        s.animation_time = 0.12;
        use egui::TextStyle::*;
        s.text_styles = [
            (Heading, FontId::proportional(20.0)),
            (Body, FontId::proportional(13.5)),
            (Button, FontId::proportional(13.5)),
            (Small, FontId::proportional(11.5)),
            (Monospace, FontId::monospace(13.0)),
        ]
        .into();
    });
}

// ------------------------------------------------------ syntax highlighting

/// Map a syntect scope stack to a palette colour (plus bold / italic flags).
pub struct ScopeColors {
    rules: Vec<(syntect::highlighting::ScopeSelector, u8)>,
}

pub const TOK_TEXT: u8 = 0;
pub const TOK_KEYWORD: u8 = 1;
pub const TOK_STRING: u8 = 2;
pub const TOK_COMMENT: u8 = 3;
pub const TOK_NUMBER: u8 = 4;
pub const TOK_FUNC: u8 = 5;
pub const TOK_TYPE: u8 = 6;
pub const TOK_TAG: u8 = 7;
pub const TOK_ATTR: u8 = 8;
pub const TOK_CONST: u8 = 9;
pub const TOK_PUNCT: u8 = 10;
pub const TOK_HEADING: u8 = 11;
pub const TOK_BOLD: u8 = 12;
pub const TOK_ITALIC: u8 = 13;
pub const TOK_LINK: u8 = 14;
pub const TOK_INSERTED: u8 = 15;
pub const TOK_DELETED: u8 = 16;

impl ScopeColors {
    pub fn new() -> Self {
        // later rules win; keep the most specific at the bottom
        let table: &[(&str, u8)] = &[
            ("punctuation", TOK_PUNCT),
            ("keyword.operator", TOK_PUNCT),
            ("variable.parameter", TOK_TEXT),
            ("variable.language", TOK_KEYWORD),
            ("variable.other.constant", TOK_CONST),
            ("constant", TOK_CONST),
            ("constant.numeric", TOK_NUMBER),
            ("constant.character.escape", TOK_CONST),
            ("string", TOK_STRING),
            ("string.regexp", TOK_CONST),
            ("keyword", TOK_KEYWORD),
            ("keyword.operator.word", TOK_KEYWORD),
            ("storage", TOK_KEYWORD),
            ("storage.type", TOK_KEYWORD),
            ("support.function", TOK_FUNC),
            ("support.type", TOK_TYPE),
            ("support.class", TOK_TYPE),
            ("support.constant", TOK_CONST),
            ("entity.name.function", TOK_FUNC),
            ("variable.function", TOK_FUNC),
            ("meta.function-call.generic", TOK_FUNC),
            ("entity.name.type", TOK_TYPE),
            ("entity.name.class", TOK_TYPE),
            ("entity.name.struct", TOK_TYPE),
            ("entity.name.enum", TOK_TYPE),
            ("entity.name.trait", TOK_TYPE),
            ("entity.name.namespace", TOK_TYPE),
            ("entity.other.inherited-class", TOK_TYPE),
            ("entity.name.tag", TOK_TAG),
            ("entity.other.attribute-name", TOK_ATTR),
            ("meta.mapping.key string", TOK_ATTR),
            ("meta.mapping.key", TOK_ATTR),
            ("entity.name.tag.yaml", TOK_ATTR),
            ("meta.structure.dictionary.key string", TOK_ATTR),
            ("meta.object-literal.key", TOK_ATTR),
            ("entity.name.section", TOK_HEADING),
            ("markup.heading", TOK_HEADING),
            ("markup.bold", TOK_BOLD),
            ("markup.italic", TOK_ITALIC),
            ("markup.underline.link", TOK_LINK),
            ("markup.raw", TOK_STRING),
            ("markup.quote", TOK_COMMENT),
            ("markup.inserted", TOK_INSERTED),
            ("markup.deleted", TOK_DELETED),
            ("meta.diff.header", TOK_KEYWORD),
            ("meta.diff.range", TOK_FUNC),
            ("comment", TOK_COMMENT),
        ];
        let rules = table.iter().filter_map(|(s, t)| s.parse().ok().map(|sel| (sel, *t))).collect();
        Self { rules }
    }

    pub fn classify(&self, stack: &[syntect::parsing::Scope]) -> u8 {
        let mut best = (0.0f64, TOK_TEXT);
        for (sel, tok) in &self.rules {
            if let Some(m) = sel.does_match(stack) {
                let score = m.0;
                if score >= best.0 {
                    best = (score, *tok);
                }
            }
        }
        best.1
    }
}

impl Palette {
    pub fn token(&self, tok: u8) -> Color32 {
        let s = &self.syn;
        match tok {
            TOK_KEYWORD => s.keyword,
            TOK_STRING => s.string,
            TOK_COMMENT => s.comment,
            TOK_NUMBER => s.number,
            TOK_FUNC => s.func,
            TOK_TYPE => s.typ,
            TOK_TAG => s.tag,
            TOK_ATTR => s.attr,
            TOK_CONST => s.constant,
            TOK_PUNCT => s.punct,
            TOK_HEADING => s.func,
            TOK_BOLD => self.text,
            TOK_ITALIC => self.text,
            TOK_LINK => self.accent,
            TOK_INSERTED => self.tip,
            TOK_DELETED => self.caution,
            _ => self.text,
        }
    }
}
