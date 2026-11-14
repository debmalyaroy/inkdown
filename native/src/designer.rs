//! Visual diagram designer (Design mode for `.svg` documents).
//!
//! The drawing is saved as a normal SVG that shows anywhere, with the editable model
//! embedded in an `<!--inkdown-diagram:BASE64-->` comment (same format as earlier versions).

use std::collections::HashSet;

use base64::Engine;
use egui::{pos2, vec2, Color32, CornerRadius, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use serde::{Deserialize, Serialize};

use crate::app::InkApp;
use crate::icons;
use crate::theme::{self, Palette};

const TAG: &str = "inkdown-diagram:";
const GRID: f32 = 10.0;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub fill: String,
    #[serde(default)]
    pub stroke: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub arrow: String,
    #[serde(default)]
    pub route: String,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Model {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// `(type, label, width, height, fill, stroke)`
pub const SHAPES: &[(&str, &str, f32, f32, &str, &str)] = &[
    ("rect", "Process", 140.0, 60.0, "", ""),
    ("round", "Rounded", 140.0, 60.0, "", ""),
    ("pill", "Start / end", 140.0, 50.0, "", ""),
    ("diamond", "Decision", 150.0, 90.0, "", ""),
    ("ellipse", "Ellipse", 140.0, 70.0, "", ""),
    ("para", "Input / output", 150.0, 60.0, "", ""),
    ("hex", "Preparation", 150.0, 60.0, "", ""),
    ("cylinder", "Database", 110.0, 80.0, "", ""),
    ("doc", "Document", 140.0, 76.0, "", ""),
    ("note", "Note", 140.0, 90.0, "#fff4d6", "#d99a00"),
    ("text", "Text", 120.0, 32.0, "none", "none"),
];
pub const COLORS: &[(&str, &str)] = &[("#eef0ff", "#6366f1"), ("#f5ebff", "#a855f7"), ("#e7f7ee", "#22a06b"), ("#fff4d6", "#d99a00"), ("#ffe9e7", "#e5484d"), ("#e6f4ff", "#2b8de3"), ("#f3f4f6", "#6b7280"), ("#ffffff", "#1f2328")];

fn n(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r.fract() == 0.0 { format!("{r:.0}") } else { format!("{r}") }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn color(s: &str) -> Color32 {
    let h = s.trim_start_matches('#');
    if s == "none" || h.len() != 6 {
        return Color32::TRANSPARENT;
    }
    u32::from_str_radix(h, 16).map(theme::rgb).unwrap_or(Color32::TRANSPARENT)
}

/// SVG path data for a shape of the given size.
pub fn shape_path(kind: &str, w: f32, h: f32) -> String {
    match kind {
        "round" => {
            let r = 14f32.min(w / 2.0).min(h / 2.0);
            format!("M{},0H{}Q{},0 {},{}V{}Q{},{} {},{}H{}Q0,{} 0,{}V{}Q0,0 {},0Z", n(r), n(w - r), n(w), n(w), n(r), n(h - r), n(w), n(h), n(w - r), n(h), n(r), n(h), n(h - r), n(r), n(r))
        }
        "pill" => {
            let r = (h / 2.0).min(w / 2.0);
            format!("M{},0H{}A{},{} 0 0 1 {},{}H{}A{},{} 0 0 1 {},0Z", n(r), n(w - r), n(r), n(r), n(w - r), n(h), n(r), n(r), n(r), n(r))
        }
        "diamond" => format!("M{},0L{},{}L{},{}L0,{}Z", n(w / 2.0), n(w), n(h / 2.0), n(w / 2.0), n(h), n(h / 2.0)),
        "ellipse" => format!("M0,{}A{},{} 0 1 0 {},{}A{},{} 0 1 0 0,{}Z", n(h / 2.0), n(w / 2.0), n(h / 2.0), n(w), n(h / 2.0), n(w / 2.0), n(h / 2.0), n(h / 2.0)),
        "para" => {
            let k = 20f32.min(w / 4.0);
            format!("M{},0H{}L{},{}H0Z", n(k), n(w), n(w - k), n(h))
        }
        "hex" => {
            let k = 22f32.min(w / 4.0);
            format!("M{},0H{}L{},{}L{},{}H{}L0,{}Z", n(k), n(w - k), n(w), n(h / 2.0), n(w - k), n(h), n(k), n(h / 2.0))
        }
        "cylinder" => {
            let e = 12f32.min(h / 4.0);
            format!("M0,{}A{},{} 0 0 1 {},{}V{}A{},{} 0 0 1 0,{}ZM0,{}A{},{} 0 0 0 {},{}", n(e), n(w / 2.0), n(e), n(w), n(e), n(h - e), n(w / 2.0), n(e), n(h - e), n(e), n(w / 2.0), n(e), n(w), n(e))
        }
        "doc" => format!("M0,0H{}V{}Q{},{} {},{}T0,{}Z", n(w), n(h - 10.0), n(w * 0.75), n(h - 24.0), n(w / 2.0), n(h - 10.0), n(h - 10.0)),
        "note" => format!("M0,0H{}L{},14V{}H0ZM{},0V14H{}", n(w - 14.0), n(w), n(h), n(w - 14.0), n(w)),
        _ => format!("M0,0H{}V{}H0Z", n(w), n(h)),
    }
}

fn center(nd: &Node) -> Pos2 {
    pos2(nd.x + nd.w / 2.0, nd.y + nd.h / 2.0)
}

fn border(nd: &Node, toward: Pos2) -> Pos2 {
    let c = center(nd);
    let (dx, dy) = (toward.x - c.x, toward.y - c.y);
    let (hw, hh) = (nd.w / 2.0, nd.h / 2.0);
    if dx == 0.0 && dy == 0.0 {
        return c;
    }
    let t = match nd.kind.as_str() {
        "ellipse" => 1.0 / ((dx * dx) / (hw * hw) + (dy * dy) / (hh * hh)).sqrt(),
        "diamond" => 1.0 / (dx.abs() / hw + dy.abs() / hh),
        _ => (hw / dx.abs().max(1e-9)).min(hh / dy.abs().max(1e-9)),
    };
    pos2(c.x + dx * t, c.y + dy * t)
}

fn edge_points(e: &Edge, m: &Model) -> Option<Vec<Pos2>> {
    let a = m.nodes.iter().find(|x| x.id == e.from)?;
    let b = m.nodes.iter().find(|x| x.id == e.to)?;
    let (ca, cb) = (center(a), center(b));
    if e.route == "elbow" {
        if (cb.x - ca.x).abs() >= (cb.y - ca.y).abs() {
            let sx = if cb.x > ca.x { a.x + a.w } else { a.x };
            let ex = if cb.x > ca.x { b.x } else { b.x + b.w };
            let mx = (sx + ex) / 2.0;
            return Some(vec![pos2(sx, ca.y), pos2(mx, ca.y), pos2(mx, cb.y), pos2(ex, cb.y)]);
        }
        let sy = if cb.y > ca.y { a.y + a.h } else { a.y };
        let ey = if cb.y > ca.y { b.y } else { b.y + b.h };
        let my = (sy + ey) / 2.0;
        return Some(vec![pos2(ca.x, sy), pos2(ca.x, my), pos2(cb.x, my), pos2(cb.x, ey)]);
    }
    Some(vec![border(a, cb), border(b, ca)])
}

pub fn bounds(m: &Model) -> Rect {
    if m.nodes.is_empty() {
        return Rect::from_min_size(pos2(0.0, 0.0), vec2(320.0, 120.0));
    }
    let mut r = Rect::NOTHING;
    for nd in &m.nodes {
        r = r.union(Rect::from_min_size(pos2(nd.x, nd.y), vec2(nd.w, nd.h)));
    }
    r.expand(16.0)
}

pub fn parse(svg: &str) -> Option<Model> {
    let i = svg.find(&format!("<!--{TAG}"))? + 4 + TAG.len();
    let end = svg[i..].find("-->")? + i;
    let bytes = base64::engine::general_purpose::STANDARD.decode(svg[i..end].trim()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Greedy word wrap using a width estimate; the same routine sizes text on canvas and in the SVG.
fn wrap(text: &str, max_w: f32, size: f32, measure: &dyn Fn(&str, f32) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let t = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if line.is_empty() || measure(&t, size) <= max_w {
                line = t;
            } else {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            }
        }
        lines.push(line);
    }
    lines
}

fn approx_width(t: &str, size: f32) -> f32 {
    t.chars().map(|c| if c.is_ascii_uppercase() || c == 'm' || c == 'w' { 0.66 } else if "iljtf.,;:'|! ".contains(c) { 0.3 } else if c.is_ascii() { 0.54 } else { 1.0 }).sum::<f32>() * size
}

fn svg_text(lines: &[String], cx: f32, cy: f32, size: f32, color: &str, bold: bool) -> String {
    let lh = size * 1.3;
    let mut s = format!(r#"<text x="{}" text-anchor="middle" font-family="Segoe UI, system-ui, sans-serif" font-size="{}" fill="{color}"{}>"#, n(cx), n(size), if bold { r#" font-weight="600""# } else { "" });
    for (i, line) in lines.iter().enumerate() {
        let y = cy - ((lines.len() - 1) as f32 * lh) / 2.0 + i as f32 * lh + size * 0.35;
        s.push_str(&format!(r#"<tspan x="{}" y="{}">{}</tspan>"#, n(cx), n(y), if line.is_empty() { " ".to_string() } else { esc(line) }));
    }
    s.push_str("</text>");
    s
}

pub fn to_svg(m: &Model) -> String {
    let b = bounds(m);
    let json = serde_json::to_string(m).unwrap_or_default();
    let mut s = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="{} {} {} {}">"#, b.width().round(), b.height().round(), n(b.min.x), n(b.min.y), n(b.width()), n(b.height()));
    s.push_str(&format!("<!--{TAG}{}-->", base64::engine::general_purpose::STANDARD.encode(json)));
    for e in &m.edges {
        let Some(pts) = edge_points(e, m) else { continue };
        let c = e.stroke.clone().unwrap_or_else(|| "#4b5563".into());
        let d: String = pts.iter().enumerate().map(|(i, p)| format!("{}{},{}", if i == 0 { "M" } else { "L" }, n(p.x), n(p.y))).collect();
        s.push_str(&format!(r#"<g><path d="{d}" fill="none" stroke="{c}" stroke-width="1.6"{} stroke-linejoin="round"/>"#, if e.dash == Some(true) { r#" stroke-dasharray="6 5""# } else { "" }));
        let head = |p: Pos2, q: Pos2| {
            let len = (q - p).length().max(1.0);
            let u = (q - p) / len;
            let bx = q - u * 11.0;
            format!(r#"<path d="M{},{}L{},{}L{},{}Z" fill="{c}"/>"#, n(q.x), n(q.y), n(bx.x - u.y * 4.5), n(bx.y + u.x * 4.5), n(bx.x + u.y * 4.5), n(bx.y - u.x * 4.5))
        };
        if e.arrow != "none" {
            s.push_str(&head(pts[pts.len() - 2], pts[pts.len() - 1]));
        }
        if e.arrow == "both" {
            s.push_str(&head(pts[1], pts[0]));
        }
        if !e.text.is_empty() {
            let i = (pts.len() - 1) / 2;
            let mid = pts[i] + (pts[i + 1] - pts[i]) / 2.0;
            let w = approx_width(&e.text, 12.5) + 10.0;
            s.push_str(&format!(r##"<rect x="{}" y="{}" width="{}" height="20" rx="4" fill="#ffffff"/>"##, n(mid.x - w / 2.0), n(mid.y - 10.0), n(w)));
            s.push_str(&svg_text(std::slice::from_ref(&e.text), mid.x, mid.y, 12.5, "#374151", false));
        }
        s.push_str("</g>");
    }
    for nd in &m.nodes {
        let none = nd.stroke == "none";
        s.push_str(&format!(r#"<g transform="translate({},{})"><path d="{}" fill="{}" stroke="{}" stroke-width="{}" stroke-linejoin="round" fill-rule="nonzero"/>"#, n(nd.x), n(nd.y), shape_path(&nd.kind, nd.w, nd.h), nd.fill, nd.stroke, if none { "0" } else { "1.6" }));
        if !nd.text.is_empty() {
            let size = nd.size.unwrap_or(14.0);
            let max_w = (nd.w - if nd.kind == "diamond" { nd.w * 0.4 } else { 18.0 }).max(20.0);
            let lines = wrap(&nd.text, max_w, size, &approx_width);
            s.push_str(&svg_text(&lines, nd.w / 2.0, nd.h / 2.0 + if nd.kind == "cylinder" { 5.0 } else { 0.0 }, size, nd.color.as_deref().unwrap_or("#1f2328"), nd.bold == Some(true)));
        }
        s.push_str("</g>");
    }
    s.push_str("</svg>");
    s
}

pub fn to_mermaid(m: &Model) -> String {
    let wrap_shape = |k: &str| match k {
        "round" => ("(", ")"),
        "pill" => ("([", "])"),
        "diamond" => ("{", "}"),
        "ellipse" => ("((", "))"),
        "para" => ("[/", "/]"),
        "hex" => ("{{", "}}"),
        "cylinder" => ("[(", ")]"),
        "note" => (">", "]"),
        _ => ("[", "]"),
    };
    let q = |t: &str| format!("\"{}\"", if t.is_empty() { " ".to_string() } else { t.replace('"', "#quot;").replace('\n', "<br/>") });
    let id = |x: &str| m.nodes.iter().position(|nd| nd.id == x).map(|i| format!("N{}", i + 1));
    let b = bounds(m);
    let mut out = vec![format!("flowchart {}", if b.width() >= b.height() { "LR" } else { "TD" })];
    for (i, nd) in m.nodes.iter().enumerate() {
        let (a, z) = wrap_shape(&nd.kind);
        out.push(format!("    N{}{a}{}{z}", i + 1, q(&nd.text)));
    }
    for e in &m.edges {
        let (Some(a), Some(z)) = (id(&e.from), id(&e.to)) else { continue };
        let dash = e.dash == Some(true);
        let link = match (e.arrow.as_str(), dash) {
            ("none", true) => "-.-",
            ("none", false) => "---",
            ("both", _) => "<-->",
            (_, true) => "-.->",
            _ => "-->",
        };
        out.push(format!("    {a} {link}{} {z}", if e.text.is_empty() { String::new() } else { format!("|{}|", q(&e.text)) }));
    }
    out.join("\n") + "\n"
}

// ----------------------------------------------------------- interaction

#[derive(Clone, PartialEq)]
enum Drag {
    None,
    Move(Pos2, Vec<(String, f32, f32)>),
    Marquee(Pos2),
    Resize(String, u8, Rect),
    Connect(String),
    Pan,
}

pub struct Designer {
    pub model: Model,
    loaded: Option<u64>,
    pub foreign: bool,
    sel: HashSet<String>,
    pan: Vec2,
    zoom: f32,
    drag: Drag,
    editing: Option<(String, String, bool)>,
    clip: Option<Model>,
    snap: bool,
    seq: u32,
    fitted: bool,
    dirty: bool,
}

impl Default for Designer {
    fn default() -> Self {
        Self { model: Model::default(), loaded: None, foreign: false, sel: HashSet::new(), pan: Vec2::ZERO, zoom: 1.0, drag: Drag::None, editing: None, clip: None, snap: true, seq: 1, fitted: false, dirty: false }
    }
}

fn snap(v: f32, on: bool) -> f32 {
    if on { (v / GRID).round() * GRID } else { v.round() }
}

impl Designer {
    fn next_id(&mut self, prefix: &str) -> String {
        loop {
            let id = format!("{prefix}{}", self.seq);
            self.seq += 1;
            if !self.model.nodes.iter().any(|x| x.id == id) && !self.model.edges.iter().any(|x| x.id == id) {
                return id;
            }
        }
    }

    fn add_node(&mut self, kind: &str, at: Pos2) -> String {
        let s = SHAPES.iter().find(|s| s.0 == kind).unwrap_or(&SHAPES[0]);
        let id = self.next_id("n");
        let (fill, stroke) = if s.4.is_empty() { COLORS[0] } else { (s.4, s.5) };
        self.model.nodes.push(Node { id: id.clone(), kind: kind.into(), x: snap(at.x - s.2 / 2.0, self.snap), y: snap(at.y - s.3 / 2.0, self.snap), w: s.2, h: s.3, text: if kind == "text" { "Text".into() } else { s.1.into() }, fill: fill.into(), stroke: stroke.into(), size: None, bold: None, color: None });
        self.sel.clear();
        self.sel.insert(id.clone());
        self.dirty = true;
        id
    }

    fn add_edge(&mut self, from: &str, to: &str) {
        if from == to || self.model.edges.iter().any(|e| e.from == from && e.to == to) {
            return;
        }
        let id = self.next_id("e");
        self.model.edges.push(Edge { id, from: from.into(), to: to.into(), arrow: "end".into(), route: "straight".into(), text: String::new(), dash: None, stroke: None });
        self.dirty = true;
    }

    fn delete_selected(&mut self) {
        let sel = self.sel.clone();
        self.model.nodes.retain(|x| !sel.contains(&x.id));
        self.model.edges.retain(|e| !sel.contains(&e.id) && !sel.contains(&e.from) && !sel.contains(&e.to));
        self.sel.clear();
        self.dirty = true;
    }

    fn node_at(&self, p: Pos2) -> Option<String> {
        self.model.nodes.iter().rev().find(|x| Rect::from_min_size(pos2(x.x, x.y), vec2(x.w, x.h)).contains(p)).map(|x| x.id.clone())
    }

    fn edge_at(&self, p: Pos2, tol: f32) -> Option<String> {
        for e in self.model.edges.iter().rev() {
            let Some(pts) = edge_points(e, &self.model) else { continue };
            for w in pts.windows(2) {
                let (a, b) = (w[0], w[1]);
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                if (a + ab * t - p).length() <= tol {
                    return Some(e.id.clone());
                }
            }
        }
        None
    }

    fn arrange(&mut self, how: &str) {
        let ids: Vec<String> = self.sel.iter().filter(|i| self.model.nodes.iter().any(|x| &x.id == *i)).cloned().collect();
        if ids.len() < 2 {
            return;
        }
        let nodes: Vec<&Node> = self.model.nodes.iter().filter(|x| ids.contains(&x.id)).collect();
        let (l, r) = (nodes.iter().map(|x| x.x).fold(f32::MAX, f32::min), nodes.iter().map(|x| x.x + x.w).fold(f32::MIN, f32::max));
        let (t, b) = (nodes.iter().map(|x| x.y).fold(f32::MAX, f32::min), nodes.iter().map(|x| x.y + x.h).fold(f32::MIN, f32::max));
        let count = nodes.len() as f32;
        let (sum_w, sum_h): (f32, f32) = (nodes.iter().map(|x| x.w).sum(), nodes.iter().map(|x| x.h).sum());
        let mut order_x: Vec<String> = ids.clone();
        order_x.sort_by(|a, b2| {
            let f = |i: &String| self.model.nodes.iter().find(|x| &x.id == i).map(|x| x.x).unwrap_or(0.0);
            f(a).partial_cmp(&f(b2)).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut order_y: Vec<String> = ids.clone();
        order_y.sort_by(|a, b2| {
            let f = |i: &String| self.model.nodes.iter().find(|x| &x.id == i).map(|x| x.y).unwrap_or(0.0);
            f(a).partial_cmp(&f(b2)).unwrap_or(std::cmp::Ordering::Equal)
        });
        let (gap_x, gap_y) = ((r - l - sum_w) / (count - 1.0), (b - t - sum_h) / (count - 1.0));
        let (mut cur_x, mut cur_y) = (l, t);
        match how {
            "dist-h" => {
                for id in &order_x {
                    if let Some(x) = self.model.nodes.iter_mut().find(|x| &x.id == id) {
                        x.x = cur_x.round();
                        cur_x += x.w + gap_x;
                    }
                }
            }
            "dist-v" => {
                for id in &order_y {
                    if let Some(x) = self.model.nodes.iter_mut().find(|x| &x.id == id) {
                        x.y = cur_y.round();
                        cur_y += x.h + gap_y;
                    }
                }
            }
            _ => {
                for x in self.model.nodes.iter_mut().filter(|x| ids.contains(&x.id)) {
                    match how {
                        "left" => x.x = l,
                        "right" => x.x = r - x.w,
                        "center" => x.x = ((l + r) / 2.0 - x.w / 2.0).round(),
                        "top" => x.y = t,
                        "bottom" => x.y = b - x.h,
                        "middle" => x.y = ((t + b) / 2.0 - x.h / 2.0).round(),
                        _ => {}
                    }
                }
            }
        }
        self.dirty = true;
    }
}

/// Outline of a shape as points (curves flattened), relative to the node origin.
fn outline(kind: &str, w: f32, h: f32) -> Vec<Pos2> {
    let arc = |cx: f32, cy: f32, rx: f32, ry: f32, a0: f32, a1: f32, steps: usize| -> Vec<Pos2> { (0..=steps).map(|i| { let a = a0 + (a1 - a0) * i as f32 / steps as f32; pos2(cx + rx * a.cos(), cy + ry * a.sin()) }).collect() };
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    match kind {
        "round" => {
            let r = 14f32.min(w / 2.0).min(h / 2.0);
            let mut p = arc(w - r, r, r, r, -FRAC_PI_2, 0.0, 6);
            p.extend(arc(w - r, h - r, r, r, 0.0, FRAC_PI_2, 6));
            p.extend(arc(r, h - r, r, r, FRAC_PI_2, PI, 6));
            p.extend(arc(r, r, r, r, PI, PI + FRAC_PI_2, 6));
            p
        }
        "pill" => {
            let r = (h / 2.0).min(w / 2.0);
            let mut p = arc(w - r, h / 2.0, r, h / 2.0, -FRAC_PI_2, FRAC_PI_2, 12);
            p.extend(arc(r, h / 2.0, r, h / 2.0, FRAC_PI_2, PI + FRAC_PI_2, 12));
            p
        }
        "diamond" => vec![pos2(w / 2.0, 0.0), pos2(w, h / 2.0), pos2(w / 2.0, h), pos2(0.0, h / 2.0)],
        "ellipse" => arc(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 0.0, TAU, 40),
        "para" => {
            let k = 20f32.min(w / 4.0);
            vec![pos2(k, 0.0), pos2(w, 0.0), pos2(w - k, h), pos2(0.0, h)]
        }
        "hex" => {
            let k = 22f32.min(w / 4.0);
            vec![pos2(k, 0.0), pos2(w - k, 0.0), pos2(w, h / 2.0), pos2(w - k, h), pos2(k, h), pos2(0.0, h / 2.0)]
        }
        "cylinder" => {
            let e = 12f32.min(h / 4.0);
            let mut p = arc(w / 2.0, e, w / 2.0, e, PI, TAU, 16);
            p.extend(arc(w / 2.0, h - e, w / 2.0, e, 0.0, PI, 16));
            p
        }
        "note" => vec![pos2(0.0, 0.0), pos2(w - 14.0, 0.0), pos2(w, 14.0), pos2(w, h), pos2(0.0, h)],
        "doc" => {
            // flat top, wavy bottom
            let mut p = vec![pos2(0.0, 0.0), pos2(w, 0.0)];
            for i in 0..=20 {
                let t = i as f32 / 20.0;
                let x = w * (1.0 - t);
                let y = h - 10.0 - (t * TAU).sin() * 7.0;
                p.push(pos2(x, y));
            }
            p
        }
        _ => vec![pos2(0.0, 0.0), pos2(w, 0.0), pos2(w, h), pos2(0.0, h)],
    }
}

fn paint_node(p: &egui::Painter, ui: &Ui, nd: &Node, to: &dyn Fn(Pos2) -> Pos2, zoom: f32) {
    let pts: Vec<Pos2> = outline(&nd.kind, nd.w, nd.h).into_iter().map(|q| to(pos2(nd.x + q.x, nd.y + q.y))).collect();
    let (fill, stroke) = (color(&nd.fill), color(&nd.stroke));
    let line = if stroke == Color32::TRANSPARENT { Stroke::NONE } else { Stroke::new(1.6 * zoom.max(0.5), stroke) };
    if nd.kind == "doc" {
        // the wave makes the outline concave: fill it as a fan of strips from the top edge
        let mut mesh = egui::Mesh::default();
        let n = pts.len();
        for q in &pts {
            mesh.colored_vertex(*q, fill);
        }
        for i in 2..n - 1 {
            mesh.add_triangle(0, i as u32, (i + 1) as u32);
            mesh.add_triangle(0, 1, i as u32);
        }
        if fill != Color32::TRANSPARENT {
            p.add(Shape::mesh(mesh));
        }
        p.add(Shape::closed_line(pts, line));
    } else {
        p.add(Shape::convex_polygon(pts, fill, line));
    }
    let o = to(pos2(nd.x, nd.y));
    match nd.kind.as_str() {
        "cylinder" => {
            let e = 12f32.min(nd.h / 4.0);
            let arc: Vec<Pos2> = (0..=16).map(|i| { let a = std::f32::consts::PI * i as f32 / 16.0; to(pos2(nd.x + nd.w / 2.0 + nd.w / 2.0 * a.cos(), nd.y + e + e * a.sin())) }).collect();
            p.add(Shape::line(arc, line));
        }
        "note" => {
            p.add(Shape::line(vec![to(pos2(nd.x + nd.w - 14.0, nd.y)), to(pos2(nd.x + nd.w - 14.0, nd.y + 14.0)), to(pos2(nd.x + nd.w, nd.y + 14.0))], line));
        }
        _ => {}
    }
    if !nd.text.is_empty() {
        let size = nd.size.unwrap_or(14.0);
        let max_w = (nd.w - if nd.kind == "diamond" { nd.w * 0.4 } else { 18.0 }).max(20.0);
        let lines = wrap(&nd.text, max_w, size, &approx_width);
        let lh = size * 1.3;
        let c = nd.color.as_deref().map(color).unwrap_or(theme::rgb(0x1f2328));
        for (i, l) in lines.iter().enumerate() {
            let g = theme::text(ui, l, size * zoom, c, if nd.bold == Some(true) { 650.0 } else { 420.0 });
            let cy = nd.h / 2.0 + if nd.kind == "cylinder" { 5.0 } else { 0.0 } - ((lines.len() - 1) as f32 * lh) / 2.0 + i as f32 * lh;
            p.galley(pos2(o.x + nd.w * zoom / 2.0 - g.size().x / 2.0, o.y + cy * zoom - g.size().y / 2.0), g, c);
        }
    }
}

fn tool_button(ui: &mut Ui, pal: &Palette, icon: &str, tip: &str, enabled: bool) -> bool {
    let b = egui::Button::new(egui::RichText::new(icon).size(15.0).color(if enabled { pal.muted } else { pal.faint })).frame_when_inactive(false).min_size(vec2(30.0, 28.0)).corner_radius(6);
    ui.add_enabled(enabled, b).on_hover_text(tip).clicked()
}

pub fn show(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    let now = ui.input(|i| i.time);
    let Some(d) = app.docs.get_mut(app.active) else { return };
    let dz = d.view.design.get_or_insert_with(Default::default);
    // (re)load when the text changed underneath us (open, undo, edits in Code mode)
    if dz.loaded != Some(d.buf.version) {
        let text = d.buf.text();
        match parse(&text) {
            Some(m) => {
                dz.model = m;
                dz.foreign = false;
            }
            None => {
                dz.foreign = !text.trim().is_empty();
                dz.model = Model::default();
            }
        }
        dz.loaded = Some(d.buf.version);
        dz.sel.retain(|id| dz.model.nodes.iter().any(|x| &x.id == id) || dz.model.edges.iter().any(|x| &x.id == id));
    }
    if dz.foreign {
        ui.painter().rect_filled(ui.available_rect_before_wrap(), 0, pal.doc_bg);
        let mut go = None;
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            ui.label(egui::RichText::new(icons::SHAPES).size(34.0).color(pal.faint));
            ui.add_space(8.0);
            ui.label(egui::RichText::new("This SVG was not made with the designer").size(16.0).strong());
            ui.label(egui::RichText::new("It can be viewed and edited as code. New drawings start from File > New from Template > Diagram.").color(pal.muted));
            ui.add_space(12.0);
            if ui.button("View").clicked() {
                go = Some("mode read");
            }
        });
        if let Some(c) = go {
            app.pending.push(c.into());
        }
        return;
    }

    let mut copy_mermaid = false;
    let mut export: Option<&'static str> = None;
    let mut history: Option<bool> = None;
    let sel_nodes = dz.model.nodes.iter().filter(|x| dz.sel.contains(&x.id)).count();
    let sel_edges = dz.model.edges.iter().filter(|x| dz.sel.contains(&x.id)).count();
    egui::Panel::top(("dz-toolbar", crate::editor::pane())).frame(egui::Frame::new().fill(pal.chrome).inner_margin(egui::Margin::symmetric(10, 5))).show_separator_line(false).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            let sep = |ui: &mut Ui| {
                ui.add_space(5.0);
                let (r, _) = ui.allocate_exact_size(vec2(1.0, 18.0), Sense::hover());
                ui.painter().rect_filled(r, 0, pal.border_soft);
                ui.add_space(5.0);
            };
            if tool_button(ui, &pal, icons::UNDO_2, "Undo (Ctrl+Z)", d.buf.can_undo()) {
                history = Some(true);
            }
            if tool_button(ui, &pal, icons::REDO_2, "Redo (Ctrl+Y)", d.buf.can_redo()) {
                history = Some(false);
            }
            if tool_button(ui, &pal, icons::TRASH_2, "Delete (Del)", !dz.sel.is_empty()) {
                dz.delete_selected();
            }
            sep(ui);
            for (fill, stroke) in COLORS {
                let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                ui.painter().circle(rect.center(), 8.0, color(fill), Stroke::new(1.6, color(stroke)));
                if resp.on_hover_text("Colour of the selection").clicked() {
                    let sel = dz.sel.clone();
                    for x in dz.model.nodes.iter_mut().filter(|x| sel.contains(&x.id)) {
                        if x.kind == "text" {
                            x.color = Some(stroke.to_string());
                        } else {
                            x.fill = fill.to_string();
                            x.stroke = stroke.to_string();
                        }
                    }
                    for e in dz.model.edges.iter_mut().filter(|e| sel.contains(&e.id)) {
                        e.stroke = Some(stroke.to_string());
                    }
                    dz.dirty = true;
                }
            }
            sep(ui);
            let r = ui.add(egui::Button::new(egui::RichText::new(format!("{}  Text  {}", icons::TYPE, icons::CHEVRON_DOWN)).size(12.5)).frame_when_inactive(false).min_size(vec2(0.0, 28.0)));
            egui::Popup::menu(&r).show(|ui| {
                ui.set_min_width(150.0);
                for (label, size) in [("Small", 12.0), ("Medium", 14.0), ("Large", 18.0), ("Title", 24.0)] {
                    if ui.button(label).clicked() {
                        let sel = dz.sel.clone();
                        for x in dz.model.nodes.iter_mut().filter(|x| sel.contains(&x.id)) {
                            x.size = Some(size);
                        }
                        dz.dirty = true;
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Bold").clicked() {
                    let sel = dz.sel.clone();
                    let all = dz.model.nodes.iter().filter(|x| sel.contains(&x.id)).all(|x| x.bold == Some(true));
                    for x in dz.model.nodes.iter_mut().filter(|x| sel.contains(&x.id)) {
                        x.bold = if all { None } else { Some(true) };
                    }
                    dz.dirty = true;
                    ui.close();
                }
            });
            let r = ui.add(egui::Button::new(egui::RichText::new(format!("{}  Line  {}", icons::SPLINE, icons::CHEVRON_DOWN)).size(12.5)).frame_when_inactive(false).min_size(vec2(0.0, 28.0)));
            egui::Popup::menu(&r).show(|ui| {
                ui.set_min_width(180.0);
                let sel = dz.sel.clone();
                let mut patch = |ui: &mut Ui, label: &str, f: &dyn Fn(&mut Edge)| {
                    if ui.button(label).clicked() {
                        // with nodes selected, apply to the lines attached to them
                        for e in dz.model.edges.iter_mut().filter(|e| sel.contains(&e.id) || (sel_edges == 0 && (sel.contains(&e.from) || sel.contains(&e.to)))) {
                            f(e);
                        }
                        dz.dirty = true;
                        ui.close();
                    }
                };
                patch(ui, "Solid", &|e| e.dash = None);
                patch(ui, "Dashed", &|e| e.dash = Some(true));
                ui.separator();
                patch(ui, "Arrow at end", &|e| e.arrow = "end".into());
                patch(ui, "Arrows at both ends", &|e| e.arrow = "both".into());
                patch(ui, "No arrows", &|e| e.arrow = "none".into());
                ui.separator();
                patch(ui, "Straight", &|e| e.route = "straight".into());
                patch(ui, "Right-angled", &|e| e.route = "elbow".into());
                patch(ui, "Reverse direction", &|e| std::mem::swap(&mut e.from, &mut e.to));
            });
            let r = ui.add_enabled(sel_nodes > 1, egui::Button::new(egui::RichText::new(format!("{}  Arrange  {}", icons::ALIGN_LEFT, icons::CHEVRON_DOWN)).size(12.5)).frame_when_inactive(false).min_size(vec2(0.0, 28.0)));
            egui::Popup::menu(&r).show(|ui| {
                ui.set_min_width(190.0);
                for (label, how) in [("Align left", "left"), ("Align centre", "center"), ("Align right", "right"), ("Align top", "top"), ("Align middle", "middle"), ("Align bottom", "bottom"), ("Distribute horizontally", "dist-h"), ("Distribute vertically", "dist-v")] {
                    if ui.button(label).clicked() {
                        dz.arrange(how);
                        ui.close();
                    }
                }
            });
            sep(ui);
            if tool_button(ui, &pal, icons::ZOOM_OUT, "Zoom out", true) {
                dz.zoom = (dz.zoom / 1.2).max(0.2);
            }
            ui.label(egui::RichText::new(format!("{:.0}%", dz.zoom * 100.0)).size(12.0).color(pal.muted));
            if tool_button(ui, &pal, icons::ZOOM_IN, "Zoom in", true) {
                dz.zoom = (dz.zoom * 1.2).min(4.0);
            }
            if tool_button(ui, &pal, icons::MAXIMIZE, "Fit drawing", true) {
                dz.fitted = false;
            }
            let snap_btn = egui::Button::new(egui::RichText::new(icons::GRID_3X3).size(15.0).color(if dz.snap { pal.accent } else { pal.muted })).fill(if dz.snap { pal.accent_soft() } else { Color32::TRANSPARENT }).min_size(vec2(30.0, 28.0));
            if ui.add(snap_btn).on_hover_text("Snap to grid").clicked() {
                dz.snap = !dz.snap;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.add(egui::Button::new(egui::RichText::new(format!("{}  Share  {}", icons::SHARE_2, icons::CHEVRON_DOWN)).size(12.5)).frame_when_inactive(false).min_size(vec2(0.0, 28.0)));
                egui::Popup::menu(&r).show(|ui| {
                    ui.set_min_width(190.0);
                    if ui.button("Copy as Mermaid").clicked() {
                        copy_mermaid = true;
                        ui.close();
                    }
                    if ui.button("Export PNG…").clicked() {
                        export = Some("export_png");
                        ui.close();
                    }
                    if ui.button("Export SVG…").clicked() {
                        export = Some("export_svg");
                        ui.close();
                    }
                });
            });
        });
    });

    // block palette
    let mut add: Option<&'static str> = None;
    egui::Panel::left(("dz-blocks", crate::editor::pane())).exact_size(132.0).frame(egui::Frame::new().fill(pal.chrome).inner_margin(egui::Margin::symmetric(8, 8))).show_separator_line(false).show(ui, |ui| {
        ui.label(egui::RichText::new("BLOCKS").size(10.5).color(pal.faint).strong());
        ui.add_space(4.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for s in SHAPES {
                let (rect, resp) = ui.allocate_exact_size(vec2(116.0, 52.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 8, pal.hover());
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let k = (30.0 / s.3).min(52.0 / s.2);
                let thumb = Node { kind: s.0.into(), w: s.2, h: s.3, fill: if s.4.is_empty() { COLORS[0].0.into() } else { s.4.into() }, stroke: if s.5.is_empty() { COLORS[0].1.into() } else { s.5.into() }, text: if s.0 == "text" { "T".into() } else { String::new() }, size: Some(22.0), color: Some(if pal.dark { "#e3e6eb".into() } else { "#1f2328".into() }), ..Default::default() };
                let o = pos2(rect.center().x - s.2 * k / 2.0, rect.min.y + 5.0);
                paint_node(ui.painter(), ui, &thumb, &|q| pos2(o.x + q.x * k, o.y + q.y * k), k);
                let g = theme::text(ui, s.1, 10.5, pal.muted, 450.0);
                ui.painter().galley(pos2(rect.center().x - g.size().x / 2.0, rect.max.y - 14.0), g, pal.muted);
                if resp.on_hover_text("Click to add").clicked() {
                    add = Some(s.0);
                }
            }
        });
    });

    egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
        let full = ui.available_rect_before_wrap();
        let id = Id::new(("designer", d.id));
        let resp = ui.interact(full, id, Sense::click_and_drag());
        ui.advance_cursor_after_rect(full);
        // the stage is always light: drawings are saved for a white page
        let p = ui.painter_at(full);
        p.rect_filled(full, CornerRadius::ZERO, theme::rgb(0xf6f7f9));
        if !dz.fitted {
            let b = bounds(&dz.model);
            dz.zoom = ((full.width() - 80.0) / b.width()).min((full.height() - 80.0) / b.height()).clamp(0.2, 1.0);
            dz.pan = full.center().to_vec2() - b.center().to_vec2() * dz.zoom - full.min.to_vec2();
            dz.fitted = true;
        }
        let (zoom, pan, origin) = (dz.zoom, dz.pan, full.min);
        let to = move |q: Pos2| pos2(origin.x + pan.x + q.x * zoom, origin.y + pan.y + q.y * zoom);
        let from = move |s: Pos2| pos2((s.x - origin.x - pan.x) / zoom, (s.y - origin.y - pan.y) / zoom);
        // grid dots
        if zoom > 0.45 {
            let step = 20.0 * zoom;
            let (mut x, y0) = (origin.x + pan.x.rem_euclid(step), origin.y + pan.y.rem_euclid(step));
            let dot = theme::rgb(0xd5d9e0);
            while x < full.max.x {
                let mut y = y0;
                while y < full.max.y {
                    p.circle_filled(pos2(x, y), 1.0, dot);
                    y += step;
                }
                x += step;
            }
        }
        if add.is_some() || resp.clicked() || resp.drag_started() {
            ui.memory_mut(|m| m.request_focus(id));
        }
        if let Some(kind) = add {
            // new blocks go under the last one, so a flow builds downwards
            let at = match dz.model.nodes.last() {
                Some(last) => pos2(last.x + last.w / 2.0, last.y + last.h + 70.0),
                None => from(full.center()),
            };
            dz.add_node(kind, at);
        }
        let focused = ui.memory(|m| m.has_focus(id)) && dz.editing.is_none();
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let mods = ui.input(|i| i.modifiers);
        let world = pointer.map(from);

        // zoom and pan
        if resp.hovered() {
            let (scroll, zd) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            if zd != 1.0 {
                if let Some(pt) = pointer {
                    let before = from(pt);
                    dz.zoom = (dz.zoom * zd).clamp(0.2, 4.0);
                    dz.pan = pt.to_vec2() - origin.to_vec2() - before.to_vec2() * dz.zoom;
                }
            } else if scroll != Vec2::ZERO {
                dz.pan += scroll;
            }
        }

        // connector handles of the hovered / single selected node
        let hover_node = world.and_then(|w| dz.node_at(w));
        let handle_of: Option<String> = match (&dz.drag, sel_nodes) {
            (Drag::None, _) => hover_node.clone().or_else(|| if sel_nodes == 1 { dz.model.nodes.iter().find(|x| dz.sel.contains(&x.id)).map(|x| x.id.clone()) } else { None }),
            _ => None,
        };
        let handles: Vec<(String, Pos2)> = handle_of
            .and_then(|hid| dz.model.nodes.iter().find(|x| x.id == hid))
            .map(|x| {
                let c = center(x);
                let g = 14.0 / zoom;
                vec![(x.id.clone(), pos2(x.x + x.w + g, c.y)), (x.id.clone(), pos2(x.x - g, c.y)), (x.id.clone(), pos2(c.x, x.y - g)), (x.id.clone(), pos2(c.x, x.y + x.h + g))]
            })
            .unwrap_or_default();
        let hot_handle = world.and_then(|w| handles.iter().find(|(_, h)| (*h - w).length() < 9.0 / zoom).cloned());
        // resize handles of a single selected node
        let single = if sel_nodes == 1 && sel_edges == 0 { dz.model.nodes.iter().find(|x| dz.sel.contains(&x.id)).cloned() } else { None };
        let corners = |x: &Node| [pos2(x.x, x.y), pos2(x.x + x.w, x.y), pos2(x.x + x.w, x.y + x.h), pos2(x.x, x.y + x.h)];
        let hot_corner = match (&single, world) {
            (Some(x), Some(w)) => corners(x).iter().position(|c| (*c - w).length() < 8.0 / zoom).map(|i| (x.clone(), i as u8)),
            _ => None,
        };

        // press
        if resp.drag_started() || (resp.is_pointer_button_down_on() && dz.drag == Drag::None && ui.input(|i| i.pointer.primary_pressed())) {
            if let Some(w) = world {
                let middle = ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
                if middle || mods.alt || ui.input(|i| i.key_down(egui::Key::Space)) {
                    dz.drag = Drag::Pan;
                } else if let Some((x, i)) = hot_corner.clone() {
                    dz.drag = Drag::Resize(x.id.clone(), i, Rect::from_min_size(pos2(x.x, x.y), vec2(x.w, x.h)));
                } else if let Some((hid, _)) = hot_handle.clone() {
                    dz.drag = Drag::Connect(hid);
                } else if let Some(nid) = dz.node_at(w) {
                    if mods.shift || mods.command {
                        if !dz.sel.remove(&nid) {
                            dz.sel.insert(nid);
                        }
                    } else if !dz.sel.contains(&nid) {
                        dz.sel.clear();
                        dz.sel.insert(nid);
                    }
                    let start: Vec<(String, f32, f32)> = dz.model.nodes.iter().filter(|x| dz.sel.contains(&x.id)).map(|x| (x.id.clone(), x.x, x.y)).collect();
                    dz.drag = Drag::Move(w, start);
                } else if let Some(eid) = dz.edge_at(w, 6.0 / zoom) {
                    if !(mods.shift || mods.command) {
                        dz.sel.clear();
                    }
                    dz.sel.insert(eid);
                } else {
                    if !(mods.shift || mods.command) {
                        dz.sel.clear();
                    }
                    dz.drag = Drag::Marquee(w);
                }
            }
        }
        // drag
        let down = ui.input(|i| i.pointer.any_down());
        if let (true, Some(w)) = (down, world) {
            match dz.drag.clone() {
                Drag::Move(start, nodes) => {
                    let delta = w - start;
                    if delta.length() > 2.0 / zoom {
                        let on = dz.snap;
                        for (nid, x0, y0) in &nodes {
                            if let Some(x) = dz.model.nodes.iter_mut().find(|x| &x.id == nid) {
                                x.x = snap(x0 + delta.x, on);
                                x.y = snap(y0 + delta.y, on);
                            }
                        }
                    }
                }
                Drag::Resize(nid, corner, r0) => {
                    let on = dz.snap;
                    let (mut l, mut t, mut r, mut b) = (r0.min.x, r0.min.y, r0.max.x, r0.max.y);
                    let (wx, wy) = (snap(w.x, on), snap(w.y, on));
                    match corner {
                        0 => { l = wx.min(r - 30.0); t = wy.min(b - 20.0); }
                        1 => { r = wx.max(l + 30.0); t = wy.min(b - 20.0); }
                        2 => { r = wx.max(l + 30.0); b = wy.max(t + 20.0); }
                        _ => { l = wx.min(r - 30.0); b = wy.max(t + 20.0); }
                    }
                    if let Some(x) = dz.model.nodes.iter_mut().find(|x| x.id == nid) {
                        (x.x, x.y, x.w, x.h) = (l, t, r - l, b - t);
                    }
                }
                Drag::Pan => {
                    dz.pan += ui.input(|i| i.pointer.delta());
                }
                _ => {}
            }
            ui.ctx().request_repaint();
        }
        // release
        if !down && dz.drag != Drag::None {
            match std::mem::replace(&mut dz.drag, Drag::None) {
                Drag::Move(start, nodes) => {
                    let moved = nodes.iter().any(|(nid, x0, y0)| dz.model.nodes.iter().any(|x| &x.id == nid && (x.x != *x0 || x.y != *y0)));
                    dz.dirty |= moved;
                    let _ = start;
                }
                Drag::Resize(..) => dz.dirty = true,
                Drag::Marquee(a) => {
                    if let Some(w) = world {
                        let r = Rect::from_two_pos(a, w);
                        if r.width() > 3.0 || r.height() > 3.0 {
                            for x in &dz.model.nodes {
                                if r.intersects(Rect::from_min_size(pos2(x.x, x.y), vec2(x.w, x.h))) {
                                    dz.sel.insert(x.id.clone());
                                }
                            }
                        }
                    }
                }
                Drag::Connect(from_id) => {
                    if let Some(w) = world {
                        match dz.node_at(w) {
                            Some(target) => dz.add_edge(&from_id, &target),
                            None => {
                                // dropping on empty canvas makes a matching block and links it
                                if let Some(src) = dz.model.nodes.iter().find(|x| x.id == from_id).cloned() {
                                    if (w - center(&src)).length() > 40.0 {
                                        let kind = if src.kind == "text" { "round".to_string() } else { src.kind.clone() };
                                        let nid = dz.add_node(&kind, w);
                                        if let Some(x) = dz.model.nodes.iter_mut().find(|x| x.id == nid) {
                                            x.fill = src.fill.clone();
                                            x.stroke = src.stroke.clone();
                                        }
                                        dz.add_edge(&from_id, &nid);
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // double-click edits text
        if resp.double_clicked() {
            if let Some(w) = world {
                if let Some(nid) = dz.node_at(w) {
                    let t = dz.model.nodes.iter().find(|x| x.id == nid).map(|x| x.text.clone()).unwrap_or_default();
                    dz.editing = Some((nid, t, false));
                } else if let Some(eid) = dz.edge_at(w, 6.0 / zoom) {
                    let t = dz.model.edges.iter().find(|x| x.id == eid).map(|x| x.text.clone()).unwrap_or_default();
                    dz.editing = Some((eid, t, true));
                }
            }
        }
        // keyboard
        if focused {
            let (del, esc, all, copy, paste, dup, undo, redo, enter) = ui.input(|i| (i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace), i.key_pressed(egui::Key::Escape), i.modifiers.command && i.key_pressed(egui::Key::A), i.events.iter().any(|e| matches!(e, egui::Event::Copy)), i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))), i.modifiers.command && i.key_pressed(egui::Key::D), i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z), i.modifiers.command && (i.key_pressed(egui::Key::Y) || (i.modifiers.shift && i.key_pressed(egui::Key::Z))), i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::F2)));
            if del && !dz.sel.is_empty() {
                dz.delete_selected();
            }
            if esc {
                dz.sel.clear();
            }
            if all {
                dz.sel = dz.model.nodes.iter().map(|x| x.id.clone()).collect();
            }
            if undo {
                history = Some(true);
            }
            if redo {
                history = Some(false);
            }
            if copy || dup {
                let sel = dz.sel.clone();
                dz.clip = Some(Model { nodes: dz.model.nodes.iter().filter(|x| sel.contains(&x.id)).cloned().collect(), edges: dz.model.edges.iter().filter(|e| sel.contains(&e.from) && sel.contains(&e.to)).cloned().collect() });
            }
            if (paste || dup) && dz.clip.as_ref().is_some_and(|c| !c.nodes.is_empty()) {
                let clip = dz.clip.clone().unwrap();
                let mut map = std::collections::HashMap::new();
                dz.sel.clear();
                for mut x in clip.nodes {
                    let nid = dz.next_id("n");
                    map.insert(x.id.clone(), nid.clone());
                    x.id = nid.clone();
                    x.x += 20.0;
                    x.y += 20.0;
                    dz.sel.insert(nid);
                    dz.model.nodes.push(x);
                }
                for mut e in clip.edges {
                    if let (Some(a), Some(b)) = (map.get(&e.from), map.get(&e.to)) {
                        e.id = dz.next_id("e");
                        e.from = a.clone();
                        e.to = b.clone();
                        dz.model.edges.push(e);
                    }
                }
                dz.dirty = true;
            }
            if enter && dz.sel.len() == 1 {
                let sid = dz.sel.iter().next().cloned().unwrap();
                if let Some(x) = dz.model.nodes.iter().find(|x| x.id == sid) {
                    dz.editing = Some((sid, x.text.clone(), false));
                } else if let Some(e) = dz.model.edges.iter().find(|x| x.id == sid) {
                    dz.editing = Some((sid, e.text.clone(), true));
                }
            }
            let step = if mods.shift { 1.0 } else { GRID };
            let (dx, dy) = ui.input(|i| ((i.key_pressed(egui::Key::ArrowRight) as i32 - i.key_pressed(egui::Key::ArrowLeft) as i32) as f32 * step, (i.key_pressed(egui::Key::ArrowDown) as i32 - i.key_pressed(egui::Key::ArrowUp) as i32) as f32 * step));
            if (dx != 0.0 || dy != 0.0) && sel_nodes > 0 {
                let sel = dz.sel.clone();
                for x in dz.model.nodes.iter_mut().filter(|x| sel.contains(&x.id)) {
                    x.x += dx;
                    x.y += dy;
                }
                dz.dirty = true;
            }
        }

        // ---- paint
        let accent = theme::rgb(0x5b5fe9);
        for e in &dz.model.edges {
            let Some(pts) = edge_points(e, &dz.model) else { continue };
            let c = e.stroke.as_deref().map(color).unwrap_or(theme::rgb(0x4b5563));
            let selected = dz.sel.contains(&e.id);
            let sp: Vec<Pos2> = pts.iter().map(|q| to(*q)).collect();
            if selected {
                p.add(Shape::line(sp.clone(), Stroke::new(6.0, theme::with_alpha(accent, 0.25))));
            }
            let stroke = Stroke::new(1.6 * zoom.max(0.6), c);
            if e.dash == Some(true) {
                p.extend(Shape::dashed_line(&sp, stroke, 6.0 * zoom, 5.0 * zoom));
            } else {
                p.add(Shape::line(sp.clone(), stroke));
            }
            let head = |a: Pos2, b: Pos2| {
                let u = (b - a).normalized();
                let base = b - u * 11.0 * zoom;
                let nrm = vec2(-u.y, u.x) * 4.5 * zoom;
                Shape::convex_polygon(vec![b, base + nrm, base - nrm], c, Stroke::NONE)
            };
            if e.arrow != "none" {
                p.add(head(sp[sp.len() - 2], sp[sp.len() - 1]));
            }
            if e.arrow == "both" {
                p.add(head(sp[1], sp[0]));
            }
            if !e.text.is_empty() {
                let i = (sp.len() - 1) / 2;
                let mid = sp[i] + (sp[i + 1] - sp[i]) / 2.0;
                let g = theme::text(ui, &e.text, 12.5 * zoom, theme::rgb(0x374151), 450.0);
                p.rect_filled(Rect::from_center_size(mid, g.size() + vec2(10.0, 6.0) * zoom), 4, Color32::WHITE);
                p.galley(mid - g.size() / 2.0, g, Color32::BLACK);
            }
        }
        for x in &dz.model.nodes {
            paint_node(&p, ui, x, &to, zoom);
            if dz.sel.contains(&x.id) {
                let r = Rect::from_min_max(to(pos2(x.x, x.y)), to(pos2(x.x + x.w, x.y + x.h))).expand(4.0);
                p.rect_stroke(r, 4, Stroke::new(1.5, accent), egui::StrokeKind::Outside);
            }
        }
        if let Some(x) = &single {
            for c in corners(x) {
                let s = to(c);
                p.rect_filled(Rect::from_center_size(s, vec2(8.0, 8.0)), 2, Color32::WHITE);
                p.rect_stroke(Rect::from_center_size(s, vec2(8.0, 8.0)), 2, Stroke::new(1.5, accent), egui::StrokeKind::Inside);
            }
        }
        for (_, h) in &handles {
            let hot = hot_handle.as_ref().is_some_and(|(_, hh)| hh == h);
            p.circle(to(*h), if hot { 6.5 } else { 5.0 }, if hot { accent } else { Color32::WHITE }, Stroke::new(1.5, accent));
        }
        match (&dz.drag, pointer) {
            (Drag::Marquee(a), Some(pt)) => {
                let r = Rect::from_two_pos(to(*a), pt);
                p.rect_filled(r, 2, theme::with_alpha(accent, 0.1));
                p.rect_stroke(r, 2, Stroke::new(1.0, accent), egui::StrokeKind::Inside);
            }
            (Drag::Connect(fid), Some(pt)) => {
                if let Some(src) = dz.model.nodes.iter().find(|x| &x.id == fid) {
                    let a = to(border(src, from(pt)));
                    p.line_segment([a, pt], Stroke::new(1.6, accent));
                    p.circle_filled(pt, 4.0, accent);
                }
            }
            _ => {}
        }
        if hot_handle.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        } else if hot_corner.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        } else if matches!(dz.drag, Drag::Move(..)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if hover_node.is_some() && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if dz.model.nodes.is_empty() {
            let g = theme::text(ui, "Click a block on the left to start. Drag from a block's blue dots to connect.", 13.5, theme::rgb(0x8b93a1), 450.0);
            p.galley(full.center() - g.size() / 2.0, g, Color32::GRAY);
        }

        // in-place text editing
        if let Some((eid, mut text, is_edge)) = dz.editing.take() {
            let rect = if is_edge {
                dz.model.edges.iter().find(|e| e.id == eid).and_then(|e| edge_points(e, &dz.model)).map(|pts| {
                    let i = (pts.len() - 1) / 2;
                    Rect::from_center_size(to(pts[i] + (pts[i + 1] - pts[i]) / 2.0), vec2(150.0, 30.0))
                })
            } else {
                dz.model.nodes.iter().find(|x| x.id == eid).map(|x| Rect::from_min_max(to(pos2(x.x, x.y)), to(pos2(x.x + x.w, x.y + x.h))).expand(2.0))
            };
            if let Some(rect) = rect {
                let mut done = false;
                egui::Area::new(Id::new("dz-edit")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ui.ctx(), |ui| {
                    egui::Frame::new().fill(Color32::WHITE).stroke(Stroke::new(1.5, accent)).corner_radius(6).inner_margin(4).show(ui, |ui| {
                        let te = egui::TextEdit::multiline(&mut text).desired_width(rect.width().max(120.0) - 8.0).desired_rows(1).text_color(theme::rgb(0x1f2328)).horizontal_align(egui::Align::Center).frame(egui::Frame::new());
                        let r = ui.add(te);
                        r.request_focus();
                        let (esc, commit) = ui.input(|i| (i.key_pressed(egui::Key::Escape), i.modifiers.command && i.key_pressed(egui::Key::Enter)));
                        if esc || commit || r.clicked_elsewhere() {
                            done = true;
                        }
                    });
                });
                if done {
                    let t = text.trim_end().to_string();
                    if is_edge {
                        if let Some(e) = dz.model.edges.iter_mut().find(|e| e.id == eid) {
                            dz.dirty |= e.text != t;
                            e.text = t;
                        }
                    } else if let Some(x) = dz.model.nodes.iter_mut().find(|x| x.id == eid) {
                        dz.dirty |= x.text != t;
                        x.text = t;
                    }
                    ui.memory_mut(|m| m.request_focus(id));
                } else {
                    dz.editing = Some((eid, text, is_edge));
                }
            }
        }
    });

    // write changes back to the document
    if let Some(undo) = history {
        if undo { d.buf.undo() } else { d.buf.redo() };
        d.edited_at = Some(now);
        dz.loaded = None;
        dz.dirty = false;
    } else if dz.dirty && dz.drag == Drag::None {
        dz.dirty = false;
        let svg = to_svg(&dz.model);
        if svg != d.buf.text() {
            d.buf.set_text(&svg);
            d.edited_at = Some(now);
        }
        dz.loaded = Some(d.buf.version);
        let _ = d.buf.take_dirty_line();
    }
    if copy_mermaid {
        let text = to_mermaid(&dz.model);
        ui.ctx().copy_text(text);
        app.toast("Copied as Mermaid");
    }
    if let Some(c) = export {
        app.pending.push(c.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Model {
        let mut d = Designer::default();
        d.snap = true;
        let a = d.add_node("pill", pos2(100.0, 100.0));
        let b = d.add_node("diamond", pos2(320.0, 100.0));
        let c = d.add_node("doc", pos2(320.0, 260.0));
        d.add_edge(&a, &b);
        d.add_edge(&b, &c);
        d.model.edges[1].text = "yes".into();
        d.model.edges[1].dash = Some(true);
        d.model
    }

    #[test]
    fn svg_round_trip_and_mermaid() {
        let m = sample();
        let svg = to_svg(&m);
        assert!(svg.starts_with("<svg xmlns") && svg.contains("<!--inkdown-diagram:"));
        assert_eq!(parse(&svg).unwrap(), m);
        // the saved file is a valid, renderable SVG
        let r = crate::assets::raster_svg(&svg, 1.0, 2000.0, None).unwrap();
        assert!(r.w > 200 && r.h > 100);
        let mmd = to_mermaid(&m);
        assert!(mmd.starts_with("flowchart "));
        assert!(mmd.contains("N1([\"Start / end\"])") && mmd.contains("N2{\"Decision\"}") && mmd.contains("N2 -.->|\"yes\"| N3"));
        assert!(crate::assets::mermaid_svg(&mmd, false).is_ok());
        // a model written by the web designer still loads
        let web = r##"{"nodes":[{"id":"n1","type":"rect","x":10,"y":20,"w":140,"h":60,"text":"A","fill":"#eef0ff","stroke":"#6366f1"}],"edges":[]}"##;
        let old = format!("<svg><!--inkdown-diagram:{}--></svg>", base64::engine::general_purpose::STANDARD.encode(web));
        assert_eq!(parse(&old).unwrap().nodes[0].text, "A");
        assert!(parse("<svg></svg>").is_none());
    }

    #[test]
    fn arrange_and_delete() {
        let mut d = Designer::default();
        let a = d.add_node("rect", pos2(100.0, 100.0));
        let b = d.add_node("rect", pos2(300.0, 180.0));
        d.add_edge(&a, &b);
        d.sel = [a.clone(), b.clone()].into_iter().collect();
        d.arrange("top");
        assert_eq!(d.model.nodes[0].y, d.model.nodes[1].y);
        d.sel = [a].into_iter().collect();
        d.delete_selected();
        assert_eq!((d.model.nodes.len(), d.model.edges.len()), (1, 0));
    }
}
