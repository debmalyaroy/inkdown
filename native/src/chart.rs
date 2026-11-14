//! Charts from an ECharts-style JSON option (the subset documents actually use):
//! bar, line, area, scatter and pie series with a category or value axis, drawn as SVG.

use serde_json::Value;

const COLORS: &[&str] = &["#5b5fe9", "#22a06b", "#e8833a", "#d6409f", "#0e9aa7", "#c9a227", "#8250df", "#e5484d", "#3b82f6", "#6b7280"];

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Object(o) => o.get("value").and_then(num),
        Value::Array(a) => a.last().and_then(num),
        _ => None,
    }
}

fn label_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(o) => o.get("name").or(o.get("value")).map(label_of).unwrap_or_default(),
        other => other.to_string(),
    }
}

fn first<'a>(v: &'a Value, key: &str) -> &'a Value {
    match &v[key] {
        Value::Array(a) => a.first().unwrap_or(&Value::Null),
        other => other,
    }
}

fn nice_ticks(min: f64, max: f64, target: usize) -> (f64, f64, f64) {
    let span = (max - min).abs().max(1e-9);
    let raw = span / target.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    ((min / step).floor() * step, (max / step).ceil() * step, step)
}

fn fmt_num(v: f64) -> String {
    let a = v.abs();
    if a >= 1e9 {
        format!("{:.1}B", v / 1e9)
    } else if a >= 1e6 {
        format!("{:.1}M", v / 1e6)
    } else if a >= 1e4 {
        format!("{:.0}k", v / 1e3)
    } else if (v - v.round()).abs() < 1e-9 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

/// Parse the option: JSON, or the looser JS object syntax ECharts examples use.
pub fn parse_option(src: &str) -> Result<Value, String> {
    let s = src.trim().trim_start_matches("option").trim_start().trim_start_matches('=').trim().trim_end_matches(';');
    if let Ok(v) = serde_json::from_str::<Value>(s) {
        return Ok(v);
    }
    // quote bare keys and turn single quotes into double quotes
    let re_key = regex::Regex::new(r#"([{,]\s*)([A-Za-z_][A-Za-z0-9_]*)\s*:"#).unwrap();
    let mut t = String::with_capacity(s.len());
    let mut in_dq = false;
    let mut in_sq = false;
    for c in s.chars() {
        match c {
            '"' if !in_sq => {
                in_dq = !in_dq;
                t.push(c)
            }
            '\'' if !in_dq => {
                in_sq = !in_sq;
                t.push('"')
            }
            _ => t.push(c),
        }
    }
    let t = re_key.replace_all(&t, "$1\"$2\":");
    let t = crate::settings::strip_json_comments(&t);
    serde_json::from_str(&t).map_err(|e| format!("Chart option is not valid JSON: {e}"))
}

pub fn chart_svg(src: &str, dark: bool, width: f32) -> Result<String, String> {
    let opt = parse_option(src)?;
    let series: Vec<&Value> = match &opt["series"] {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![&opt["series"]],
        _ => return Err("The chart has no series.".into()),
    };
    if series.is_empty() {
        return Err("The chart has no series.".into());
    }
    let (fg, muted, grid, bg) = if dark { ("#e3e6eb", "#9aa3af", "#333944", "none") } else { ("#1f2328", "#68707c", "#e3e6ea", "none") };
    let w = width.max(320.0) as f64;
    let h = (w * 0.56).clamp(260.0, 440.0);
    let title = first(&opt, "title")["text"].as_str().unwrap_or("");
    let mut out = String::new();
    out.push_str(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" font-family="sans-serif" font-size="12">"#));
    out.push_str(&format!(r#"<rect width="{w:.0}" height="{h:.0}" fill="{bg}"/>"#));
    let mut top = 16.0;
    if !title.is_empty() {
        out.push_str(&format!(r#"<text x="{:.1}" y="24" text-anchor="middle" font-size="15" font-weight="600" fill="{fg}">{}</text>"#, w / 2.0, esc(title)));
        top = 40.0;
    }
    let kind = |s: &Value| s["type"].as_str().unwrap_or("bar").to_string();
    let names: Vec<String> = series.iter().enumerate().map(|(i, s)| s["name"].as_str().map(String::from).unwrap_or_else(|| format!("Series {}", i + 1))).collect();

    if kind(series[0]) == "pie" {
        let data: Vec<(String, f64)> = series[0]["data"].as_array().map(|a| a.iter().filter_map(|d| num(d).map(|v| (label_of(d), v.max(0.0)))).collect()).unwrap_or_default();
        let total: f64 = data.iter().map(|d| d.1).sum();
        if total <= 0.0 {
            return Err("The pie chart has no data.".into());
        }
        let (cx, cy) = (w * 0.36, top + (h - top) / 2.0);
        let r = ((h - top) / 2.0 - 14.0).min(w * 0.3);
        let donut = series[0]["radius"].is_array();
        let mut a0 = -std::f64::consts::FRAC_PI_2;
        for (i, (_, v)) in data.iter().enumerate() {
            let a1 = a0 + v / total * std::f64::consts::TAU;
            let large = if a1 - a0 > std::f64::consts::PI { 1 } else { 0 };
            let (x0, y0, x1, y1) = (cx + r * a0.cos(), cy + r * a0.sin(), cx + r * a1.cos(), cy + r * a1.sin());
            let c = COLORS[i % COLORS.len()];
            if data.len() == 1 {
                out.push_str(&format!(r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{r:.1}" fill="{c}"/>"#));
            } else {
                out.push_str(&format!(r#"<path d="M{cx:.1},{cy:.1} L{x0:.1},{y0:.1} A{r:.1},{r:.1} 0 {large} 1 {x1:.1},{y1:.1} Z" fill="{c}" stroke="{}" stroke-width="1.5"/>"#, if dark { "#16181d" } else { "#ffffff" }));
            }
            a0 = a1;
        }
        if donut {
            out.push_str(&format!(r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{:.1}" fill="{}"/>"#, r * 0.55, if dark { "#16181d" } else { "#ffffff" }));
        }
        let lx = w * 0.68;
        let mut ly = cy - data.len() as f64 * 11.0 + 6.0;
        for (i, (name, v)) in data.iter().enumerate() {
            let c = COLORS[i % COLORS.len()];
            out.push_str(&format!(r#"<rect x="{lx:.1}" y="{:.1}" width="11" height="11" rx="3" fill="{c}"/><text x="{:.1}" y="{:.1}" fill="{fg}">{} <tspan fill="{muted}">{:.0}%</tspan></text>"#, ly - 9.5, lx + 18.0, ly, esc(name), v / total * 100.0));
            ly += 22.0;
        }
        out.push_str("</svg>");
        return Ok(out);
    }

    // cartesian
    let horizontal = first(&opt, "yAxis")["type"].as_str() == Some("category");
    let cat_axis = if horizontal { first(&opt, "yAxis") } else { first(&opt, "xAxis") };
    let mut cats: Vec<String> = cat_axis["data"].as_array().map(|a| a.iter().map(label_of).collect()).unwrap_or_default();
    let scatter = series.iter().all(|s| kind(s) == "scatter") && cats.is_empty();
    let values: Vec<Vec<Option<(f64, f64)>>> = series
        .iter()
        .map(|s| {
            s["data"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .enumerate()
                        .map(|(i, d)| match d {
                            Value::Array(p) if p.len() >= 2 => Some((num(&p[0]).unwrap_or(i as f64), num(&p[1])?)),
                            other => num(other).map(|v| (i as f64, v)),
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    let n = values.iter().map(Vec::len).max().unwrap_or(0);
    if n == 0 {
        return Err("The chart has no data.".into());
    }
    while cats.len() < n && !scatter {
        cats.push((cats.len() + 1).to_string());
    }
    let stacked = series.iter().any(|s| !s["stack"].is_null());
    let all: Vec<f64> = if stacked {
        (0..n).map(|i| values.iter().filter_map(|v| v.get(i).copied().flatten().map(|p| p.1)).sum()).collect()
    } else {
        values.iter().flatten().flatten().map(|p| p.1).collect()
    };
    let (mut lo, mut hi) = all.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    if lo > 0.0 && !scatter {
        lo = 0.0;
    }
    if hi < 0.0 {
        hi = 0.0;
    }
    if (hi - lo).abs() < 1e-12 {
        hi = lo + 1.0;
    }
    let (lo, hi, step) = nice_ticks(lo, hi, 5);
    let legend = series.len() > 1;
    let (left, right, bottom) = (if horizontal { 96.0 } else { 54.0 }, 22.0, if legend { 58.0 } else { 36.0 });
    let (pw, ph) = (w - left - right, h - top - bottom);
    let vx = |v: f64| left + (v - lo) / (hi - lo) * pw;
    let vy = |v: f64| top + ph - (v - lo) / (hi - lo) * ph;
    // grid + value axis labels
    let mut t = lo;
    while t <= hi + step * 1e-6 {
        if horizontal {
            let x = vx(t);
            out.push_str(&format!(r#"<line x1="{x:.1}" y1="{top:.1}" x2="{x:.1}" y2="{:.1}" stroke="{grid}"/><text x="{x:.1}" y="{:.1}" text-anchor="middle" fill="{muted}">{}</text>"#, top + ph, top + ph + 16.0, fmt_num(t)));
        } else {
            let y = vy(t);
            out.push_str(&format!(r#"<line x1="{left:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke="{grid}"/><text x="{:.1}" y="{:.1}" text-anchor="end" fill="{muted}">{}</text>"#, left + pw, left - 8.0, y + 4.0, fmt_num(t)));
        }
        t += step;
    }
    let (xlo, xhi) = if scatter {
        let xs: Vec<f64> = values.iter().flatten().flatten().map(|p| p.0).collect();
        let (a, b) = xs.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        let (a, b, _) = nice_ticks(a, if b > a { b } else { a + 1.0 }, 6);
        (a, b)
    } else {
        (0.0, n as f64)
    };
    let band = if horizontal { ph / n as f64 } else { pw / n as f64 };
    // category labels
    if scatter {
        let (_, _, sx) = nice_ticks(xlo, xhi, 6);
        let mut t = xlo;
        while t <= xhi + sx * 1e-6 {
            let x = left + (t - xlo) / (xhi - xlo) * pw;
            out.push_str(&format!(r#"<text x="{x:.1}" y="{:.1}" text-anchor="middle" fill="{muted}">{}</text>"#, top + ph + 16.0, fmt_num(t)));
            t += sx;
        }
    } else {
        let every = ((cats.len() as f64 * 46.0 / if horizontal { ph * 2.4 } else { pw }).ceil() as usize).max(1);
        for (i, c) in cats.iter().enumerate().take(n) {
            if i % every != 0 {
                continue;
            }
            let short: String = if c.chars().count() > 14 { c.chars().take(13).chain(std::iter::once('…')).collect() } else { c.clone() };
            if horizontal {
                out.push_str(&format!(r#"<text x="{:.1}" y="{:.1}" text-anchor="end" fill="{muted}">{}</text>"#, left - 8.0, top + band * (i as f64 + 0.5) + 4.0, esc(&short)));
            } else {
                out.push_str(&format!(r#"<text x="{:.1}" y="{:.1}" text-anchor="middle" fill="{muted}">{}</text>"#, left + band * (i as f64 + 0.5), top + ph + 16.0, esc(&short)));
            }
        }
    }
    out.push_str(&format!(r#"<line x1="{left:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{muted}" stroke-opacity=".6"/>"#, top + ph, left + pw, top + ph));
    let bars: Vec<usize> = series.iter().enumerate().filter(|(_, s)| kind(s) == "bar").map(|(i, _)| i).collect();
    let groups = if stacked { 1 } else { bars.len().max(1) };
    let mut stack_acc = vec![0.0f64; n];
    for (si, s) in series.iter().enumerate() {
        let c = s["itemStyle"]["color"].as_str().or(s["color"].as_str()).unwrap_or(COLORS[si % COLORS.len()]);
        let pts = &values[si];
        match kind(s).as_str() {
            "bar" => {
                let gi = if stacked { 0 } else { bars.iter().position(|b| *b == si).unwrap_or(0) };
                let bw = (band * 0.68 / groups as f64).min(64.0);
                let total_w = bw * groups as f64;
                for (i, p) in pts.iter().enumerate() {
                    let Some((_, v)) = p else { continue };
                    let base = if stacked { stack_acc[i] } else { 0.0 };
                    let (a, b) = (base, base + v);
                    if stacked {
                        stack_acc[i] += v;
                    }
                    let off = band * (i as f64 + 0.5) - total_w / 2.0 + bw * gi as f64;
                    if horizontal {
                        let (x0, x1) = (vx(a.min(b)), vx(a.max(b)));
                        out.push_str(&format!(r#"<rect x="{x0:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="3" fill="{c}"/>"#, top + off, (x1 - x0).max(0.5), bw - 2.0));
                    } else {
                        let (y0, y1) = (vy(a.max(b)), vy(a.min(b)));
                        out.push_str(&format!(r#"<rect x="{:.1}" y="{y0:.1}" width="{:.1}" height="{:.1}" rx="3" fill="{c}"/>"#, left + off, bw - 2.0, (y1 - y0).max(0.5)));
                    }
                }
            }
            "scatter" => {
                for p in pts.iter().flatten() {
                    let x = if scatter { left + (p.0 - xlo) / (xhi - xlo) * pw } else { left + band * (p.0 + 0.5) };
                    out.push_str(&format!(r#"<circle cx="{x:.1}" cy="{:.1}" r="4.5" fill="{c}" fill-opacity=".8"/>"#, vy(p.1)));
                }
            }
            _ => {
                let xy: Vec<(f64, f64)> = pts.iter().enumerate().filter_map(|(i, p)| p.map(|(_, v)| (left + band * (i as f64 + 0.5), vy(v)))).collect();
                if xy.is_empty() {
                    continue;
                }
                let smooth = s["smooth"].as_bool().unwrap_or(false);
                let mut d = format!("M{:.1},{:.1}", xy[0].0, xy[0].1);
                for k in 1..xy.len() {
                    if smooth {
                        let (p0, p1) = (xy[k - 1], xy[k]);
                        let mx = (p0.0 + p1.0) / 2.0;
                        d.push_str(&format!(" C{mx:.1},{:.1} {mx:.1},{:.1} {:.1},{:.1}", p0.1, p1.1, p1.0, p1.1));
                    } else {
                        d.push_str(&format!(" L{:.1},{:.1}", xy[k].0, xy[k].1));
                    }
                }
                if !s["areaStyle"].is_null() {
                    out.push_str(&format!(r#"<path d="{d} L{:.1},{:.1} L{:.1},{:.1} Z" fill="{c}" fill-opacity=".16"/>"#, xy[xy.len() - 1].0, vy(lo.max(0.0)), xy[0].0, vy(lo.max(0.0))));
                }
                out.push_str(&format!(r#"<path d="{d}" fill="none" stroke="{c}" stroke-width="2.4" stroke-linejoin="round" stroke-linecap="round"/>"#));
                if xy.len() <= 40 {
                    for (x, y) in &xy {
                        out.push_str(&format!(r#"<circle cx="{x:.1}" cy="{y:.1}" r="3.2" fill="{}" stroke="{c}" stroke-width="2"/>"#, if dark { "#16181d" } else { "#ffffff" }));
                    }
                }
            }
        }
    }
    if legend {
        let widths: Vec<f64> = names.iter().map(|n| 26.0 + n.chars().count() as f64 * 6.6 + 14.0).collect();
        let mut x = (w - widths.iter().sum::<f64>()) / 2.0;
        let y = h - 14.0;
        for (i, name) in names.iter().enumerate() {
            let c = COLORS[i % COLORS.len()];
            out.push_str(&format!(r#"<rect x="{x:.1}" y="{:.1}" width="14" height="10" rx="3" fill="{c}"/><text x="{:.1}" y="{y:.1}" fill="{fg}">{}</text>"#, y - 9.0, x + 20.0, esc(name)));
            x += widths[i];
        }
    }
    out.push_str("</svg>");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_charts() {
        let bar = chart_svg(r#"{"title":{"text":"Sales"},"xAxis":{"type":"category","data":["A","B"]},"yAxis":{},"series":[{"type":"bar","data":[3,7]},{"type":"line","data":[2,5],"smooth":true}]}"#, false, 600.0).unwrap();
        assert!(bar.contains("<rect") && bar.contains("<path") && bar.contains("Sales"));
        let pie = chart_svg("{ series: [{ type: 'pie', data: [{ name: 'x', value: 1 }, { name: 'y', value: 3 }] }] }", true, 500.0).unwrap();
        assert!(pie.contains("75%"));
        assert!(chart_svg("{}", false, 500.0).is_err());
        assert!(crate::assets::raster_svg(&bar, 1.0, 600.0, None).is_ok());
    }
}
