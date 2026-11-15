//! Scripted UI driving for tests: `INKDOWN_AUTOMATION=<script>` feeds synthetic input
//! through the normal event path and can save screenshots and state dumps.
//!
//! Script lines: `wait N`, `open PATH`, `cmd NAME [ARG]`, `type TEXT`, `key COMBO`,
//! `click X Y`, `dclick X Y`, `rclick X Y`, `drag X1 Y1 X2 Y2`, `move X Y`, `scroll DX DY`,
//! `zoom FACTOR`, `shot FILE.png`, `dump FILE.json`, `close_window`, `quit`. `\n` and `\t` in TEXT are expanded.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::{pos2, Event, Modifiers, PointerButton, Pos2};

use crate::app::InkApp;
use crate::keymap::Combo;

pub struct Automation {
    lines: VecDeque<String>,
    wait: u32,
    queue: Vec<Event>,
    shot: Option<(PathBuf, Arc<Mutex<Option<Arc<egui::ColorImage>>>>)>,
    drag: Option<(Pos2, Pos2, u32)>,
    pointer: Pos2,
    until: Option<std::time::Instant>,
}

impl Automation {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("INKDOWN_AUTOMATION")?;
        let text = std::fs::read_to_string(path).ok()?;
        Some(Self { lines: text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(String::from).collect(), wait: 3, queue: Vec::new(), shot: None, drag: None, pointer: pos2(0.0, 0.0), until: None })
    }

    pub fn inject(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        raw.events.append(&mut self.queue);
    }

    fn click(&mut self, p: Pos2, button: PointerButton, times: u32) {
        self.pointer = p;
        self.queue.push(Event::PointerMoved(p));
        for _ in 0..times {
            self.queue.push(Event::PointerButton { pos: p, button, pressed: true, modifiers: Modifiers::NONE });
            self.queue.push(Event::PointerButton { pos: p, button, pressed: false, modifiers: Modifiers::NONE });
        }
    }

    pub fn step(&mut self, app: &mut InkApp, ctx: &egui::Context) {
        ctx.request_repaint();
        // a pending screenshot blocks the script until it is written
        if let Some((path, slot)) = &self.shot {
            let img = slot.lock().unwrap().take();
            match img {
                Some(img) => {
                    let [w, h] = img.size;
                    let bytes: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = image::save_buffer(path, &bytes, w as u32, h as u32, image::ColorType::Rgba8);
                    self.shot = None;
                }
                None => return,
            }
        }
        if let Some((from, to, n)) = self.drag {
            // spread a drag over a few frames so widgets see movement
            let steps = 6;
            if n == 0 {
                self.queue.push(Event::PointerMoved(from));
                self.queue.push(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
            } else if n <= steps {
                let t = n as f32 / steps as f32;
                self.queue.push(Event::PointerMoved(from + (to - from) * t));
            } else {
                self.queue.push(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
                self.drag = None;
                self.pointer = to;
                return;
            }
            self.drag = Some((from, to, n + 1));
            return;
        }
        if let Some(t) = self.until {
            if std::time::Instant::now() < t {
                return;
            }
            self.until = None;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        let Some(line) = self.lines.pop_front() else { return };
        app.last_cmd = format!("script: {line}");
        let (op, rest) = line.split_once(' ').map(|(a, b)| (a, b.trim())).unwrap_or((line.as_str(), ""));
        let nums: Vec<f32> = rest.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        self.wait = 1;
        match op {
            "wait" => self.wait = nums.first().copied().unwrap_or(1.0) as u32,
            // real time, for things that finish on their own schedule (exports, builds, AutoSave)
            "sleep" => self.until = Some(std::time::Instant::now() + std::time::Duration::from_millis(nums.first().copied().unwrap_or(100.0) as u64)),
            "open" => {
                app.open_path(std::path::Path::new(rest));
            }
            "cmd" => app.run(ctx, rest),
            "type" => {
                let text = rest.replace("\\n", "\n").replace("\\t", "\t").replace("\\s", " ");
                for part in text.split_inclusive('\n') {
                    let (body, nl) = match part.strip_suffix('\n') {
                        Some(b) => (b, true),
                        None => (part, false),
                    };
                    if !body.is_empty() {
                        self.queue.push(Event::Text(body.to_string()));
                    }
                    if nl {
                        self.queue.push(Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
                    }
                }
            }
            "key" => {
                if let Some(c) = Combo::parse(rest) {
                    let modifiers = Modifiers { alt: c.alt, ctrl: c.ctrl, shift: c.shift, mac_cmd: false, command: c.ctrl };
                    self.queue.push(Event::Key { key: c.key, physical_key: None, pressed: true, repeat: false, modifiers });
                    self.queue.push(Event::Key { key: c.key, physical_key: None, pressed: false, repeat: false, modifiers });
                }
            }
            "paste" => self.queue.push(Event::Paste(rest.replace("\\n", "\n"))),
            "click" if nums.len() >= 2 => self.click(pos2(nums[0], nums[1]), PointerButton::Primary, 1),
            "dclick" if nums.len() >= 2 => self.click(pos2(nums[0], nums[1]), PointerButton::Primary, 2),
            "rclick" if nums.len() >= 2 => self.click(pos2(nums[0], nums[1]), PointerButton::Secondary, 1),
            "move" if nums.len() >= 2 => {
                self.pointer = pos2(nums[0], nums[1]);
                self.queue.push(Event::PointerMoved(self.pointer));
            }
            "drag" if nums.len() >= 4 => self.drag = Some((pos2(nums[0], nums[1]), pos2(nums[2], nums[3]), 0)),
            // a pinch, or Ctrl with the wheel: the factor by which to zoom
            "zoom" if !nums.is_empty() => {
                self.queue.push(Event::Zoom(nums[0]));
                self.wait = 6;
            }
            "scroll" if nums.len() >= 2 => {
                self.queue.push(Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(nums[0], nums[1]), phase: egui::TouchPhase::Move, modifiers: Modifiers::NONE, source: egui::MouseWheelSource::Wheel });
                self.wait = 12;
            }
            "shot" => {
                let slot = Arc::new(Mutex::new(None));
                let s2 = slot.clone();
                let c2 = ctx.clone();
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                    *s2.lock().unwrap() = Some(img);
                    c2.request_repaint();
                })));
                self.shot = Some((PathBuf::from(rest), slot));
            }
            "dump" => {
                let _ = std::fs::write(rest, serde_json::to_string_pretty(&dump(app, ctx)).unwrap_or_default());
            }
            "perf" => {
                // start a fresh measurement window
                app.frame_max = 0.0;
                app.frame_sum = 0.0;
                app.frame_count = 0;
                app.total_sum = 0.0;
            }
            // ends this test process abruptly, as a crash or a forced shutdown would:
            // nothing gets a chance to save
            "crash" => std::process::exit(3),
            // what the close button of the window does
            "close_window" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            "quit" => {
                // a script that ends means it: no question about unsaved work
                app.quit_anyway = true;
                app.quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }
}

pub fn dump(app: &InkApp, ctx: &egui::Context) -> serde_json::Value {
    let docs: Vec<serde_json::Value> = app
        .docs
        .iter()
        .map(|d| {
            serde_json::json!({
                "name": d.name,
                "path": d.path.as_ref().map(|p| p.to_string_lossy().to_string()),
                "kind": d.kind.label(),
                "mode": d.mode.id(),
                "dirty": d.dirty(),
                "wrap": d.wrap,
                "read_only": d.read_only,
                "syntax": d.hl.name(),
                "lines": d.buf.lines(),
                "chars": d.buf.len(),
                "sels": d.buf.sels.iter().map(|s| vec![s.anchor, s.head]).collect::<Vec<_>>(),
                "folds": d.ed.folds.len(),
                "find_matches": d.ed.find.matches.len(),
                "find_open": d.ed.find.open,
                "completion": d.ed.completion.as_ref().map(|c| c.items.iter().map(|i| i.0.clone()).collect::<Vec<_>>()),
                "encoding": d.encoding.name(),
                "crlf": d.crlf,
                "tab_width": d.cfg.tab_width,
                "soft_tabs": d.cfg.soft_tabs,
                "scroll": d.ed.scroll.y,
                "preview_scroll": d.view.md.scroll,
                "outline": d.view.md.outline.len(),
                "preview_height": d.view.md.total,
                "hl_pending": d.hl.pending,
                "read_zoom": d.view.zoom(),
                "canvas_zoom": d.view.canvas.zoom,
                "tree_rows": d.view.tree.count,
                "live": serde_json::json!({ "url": d.live.url, "follow": d.live.follow, "loading": d.live.loading, "streaming": d.live.streaming, "updates": d.live.updates, "error": d.live.error, "consumed": d.live.consumed }),
                "pdf_pages": d.view.pdf.page_count(),
                "pdf_text_pages": d.view.pdf.text_pages(),
                "pdf_matches": d.view.pdf.matches.len(),
                "pdf_match": d.view.pdf.current_match.map(|i| i + 1),
                "pdf_selection": d.view.pdf.selected_text(),
                "pdf_needs_password": d.view.pdf.needs_password(),
                "pdf_first_page": d.view.pdf.first_page.map(|p| vec![p.0, p.1, p.2]),
                "pdf_page": d.view.pdf.current + 1,
                "pdf_error": d.view.pdf.error(),
                "tree_error": d.view.tree.error.as_ref().map(|e| match e.1 { Some(l) => format!("line {l}: {}", e.0), None => e.0.clone() }),
                "text": if d.buf.len() < 20_000 { serde_json::Value::String(d.buf.text()) } else { serde_json::Value::Null },
            })
        })
        .collect();
    let rect = ctx.content_rect();
    serde_json::json!({
        "active": app.active,
        "docs": docs,
        "update": app.updater.label(),
        "update_version": app.updater.available().map(|r| r.version.clone()),
        "dialog_kind": app.dialog.as_ref().map(|d| format!("{d:?}").split(['(', ' ', '{']).next().unwrap_or("").to_string()),
        "launched": crate::platform::launched(),
        "bottom": format!("{:?}", app.bottom),
        "split": app.split.is_some(),
        "sheet": app.sheet.map(|s| format!("{s:?}")),
        "minimap": app.set.minimap,
        "word_wrap": app.set.word_wrap,
        "font_size": app.set.font_size,
        "status_bar": app.set.show_status_bar,
        "recording": app.macro_rec.is_some(),
        "theme": app.pal.id,
        "auto_save": app.set.auto_save,
        "line_numbers": app.set.line_numbers,
        "sidebar": app.set.show_sidebar,
        "folder": app.sidebar.folder.as_ref().map(|p| p.to_string_lossy().to_string()),
        "picker": app.picker.as_ref().map(|p| serde_json::json!({ "query": p.query, "items": p.visible_labels(12) })),
        "dialog": app.dialog.is_some(),
        "toasts": app.toasts.iter().map(|t| t.0.clone()).collect::<Vec<_>>(),
        "frame_ms": app.frame_ms,
        "frame_avg_ms": if app.frame_count > 0 { app.frame_sum / app.frame_count as f32 } else { 0.0 },
        "frame_max_ms": app.frame_max,
        "atlas_resets": app.atlas_resets,
        "frame_total_avg_ms": if app.frame_count > 0 { app.total_sum / app.frame_count as f32 } else { 0.0 },
        "frames": app.frame_count,
        "slow": app.slow,
        "first_frame_ms": app.first_frame_ms,
        "memory_mb": crate::platform::memory_mb(),
        "window": [rect.width(), rect.height()],
        "search_results": app.search.results.len(),
        "build_lines": app.build.lines.len(),
        "build_tail": app.build.lines.iter().rev().take(6).rev().cloned().collect::<Vec<_>>().join(" | "),
        "build_running": app.build.running(),
    })
}
