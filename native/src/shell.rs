//! Window chrome: top bar, tabs, status bar, menus, dialogs, toasts and the image lightbox.

use egui::{pos2, vec2, Align, Color32, CornerRadius, Id, Layout, Rect, Sense, Stroke, Ui};

use crate::app::{Dialog, InkApp};
use crate::doc::{Kind, Mode};
use crate::icons;
use crate::theme::{self, Palette};

fn icon_button(ui: &mut Ui, pal: &Palette, icon: &str, tip: &str) -> egui::Response {
    let b = egui::Button::new(egui::RichText::new(icon).size(15.0).color(pal.muted)).frame_when_inactive(false).min_size(vec2(32.0, 30.0)).corner_radius(7);
    ui.add(b).on_hover_text(tip)
}

/// A Word-style on/off switch.
pub fn switch(ui: &mut Ui, pal: &Palette, on: bool, id: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 19.0), Sense::click());
    let t = ui.ctx().animate_bool(Id::new(id), on);
    let track = theme::mix(theme::mix(pal.chrome2, pal.text, 0.22), pal.accent, t);
    ui.painter().rect_filled(rect, CornerRadius::same(10), track);
    let x = rect.min.x + 9.5 + t * (rect.width() - 19.0);
    ui.painter().circle_filled(pos2(x, rect.center().y), 7.0, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn menu_item(ui: &mut Ui, app: &mut InkApp, icon: &str, label: &str, cmd: &str) {
    if !crate::picker::available(cmd) {
        return;
    }
    // key names mean nothing on a touch screen and take room the labels need
    let key = if crate::platform::touch(ui.ctx()) { String::new() } else { app.keymap.label(cmd.split(' ').next().unwrap_or(cmd)) };
    let b = egui::Button::new(format!("{icon}   {label}")).shortcut_text(egui::RichText::new(key).size(11.5).color(app.pal.faint)).min_size(vec2(230.0, 28.0));
    if ui.add(b).clicked() {
        app.pending.push(cmd.to_string());
        ui.close();
    }
}

fn main_menu(ui: &mut Ui, app: &mut InkApp) {
    ui.set_min_width(250.0);
    menu_item(ui, app, icons::FILE_PLUS, "New Document", "new_file");
    menu_item(ui, app, icons::LAYOUT_TEMPLATE, "New from Template…", "new_from_template");
    menu_item(ui, app, icons::FOLDER_OPEN, "Open…", "open_file");
    menu_item(ui, app, icons::FOLDER, "Open Folder…", "open_folder");
    menu_item(ui, app, icons::LINK, "Open from Web Address…", "open_url");
    menu_item(ui, app, icons::HISTORY, "Open Recent…", "open_recent");
    ui.separator();
    menu_item(ui, app, icons::SAVE, "Save", "save");
    menu_item(ui, app, icons::SAVE, "Save As…", "save_as");
    menu_item(ui, app, icons::SAVE, "Save All", "save_all");
    ui.separator();
    ui.menu_button(format!("{}   Notes", icons::FOLDER), |ui| {
        menu_item(ui, app, icons::FOLDER, "File in Notes…", "notes_file");
        menu_item(ui, app, icons::FOLDER, "File in Inbox", "notes_file_to");
        menu_item(ui, app, icons::FOLDER, "File All Drafts in Inbox", "notes_file_all");
        ui.separator();
        menu_item(ui, app, icons::FOLDER_OPEN, "Browse Notes", "notes_open");
        menu_item(ui, app, icons::FILE_PLUS, "New Notebook…", "notes_new_notebook");
    });
    ui.menu_button(format!("{}   Edit", icons::PENCIL), |ui| {
        menu_item(ui, app, icons::UNDO_2, "Undo", "undo");
        menu_item(ui, app, icons::REDO_2, "Redo", "redo");
        ui.separator();
        menu_item(ui, app, icons::SCISSORS, "Cut", "cut");
        menu_item(ui, app, icons::COPY, "Copy", "copy");
        menu_item(ui, app, icons::CLIPBOARD, "Paste", "paste");
        menu_item(ui, app, icons::TEXT_SELECT, "Select All", "select_all");
        ui.separator();
        menu_item(ui, app, icons::SEARCH, "Find…", "show_find");
        menu_item(ui, app, icons::REPLACE, "Replace…", "show_replace");
        menu_item(ui, app, icons::SEARCH, "Find in Files…", "find_in_files");
    });
    ui.menu_button(format!("{}   Export", icons::FILE_DOWN), |ui| export_menu(ui, app));
    ui.menu_button(format!("{}   Tools", icons::WRENCH), |ui| {
        // a long list: scroll inside the screen instead of running off its edge
        let room = (ui.ctx().content_rect().height() - 150.0).max(200.0);
        egui::ScrollArea::vertical().max_height(room).show(ui, |ui| {
            for (id, title, icon) in crate::picker::COMMANDS.iter().filter(|c| c.0.starts_with("tool_")) {
                menu_item(ui, app, icon, title.trim_start_matches("Tools: "), id);
            }
        });
    });
    ui.menu_button(format!("{}   Preferences", icons::SETTINGS), |ui| {
        menu_item(ui, app, icons::PALETTE, "Theme…", "select_theme");
        menu_item(ui, app, icons::SETTINGS, "Settings", "open_settings");
        menu_item(ui, app, icons::KEYBOARD, "Key Bindings", "open_keybindings");
        menu_item(ui, app, icons::ZAP, "Snippets", "open_snippets");
        ui.separator();
        menu_item(ui, app, icons::ZOOM_IN, "Larger Text", "increase_font_size");
        menu_item(ui, app, icons::ZOOM_OUT, "Smaller Text", "decrease_font_size");
        menu_item(ui, app, icons::HASH, "Line Numbers", "toggle_line_numbers");
        menu_item(ui, app, icons::MAP, "Minimap", "toggle_minimap");
        menu_item(ui, app, icons::WRAP_TEXT, "Word Wrap", "toggle_word_wrap");
        menu_item(ui, app, icons::REFRESH_CW, if app.docs.get(app.active).is_some_and(|d| d.live.follow) { "Stop Following" } else { "Follow (Show New Lines)" }, "toggle_follow");
    });
    menu_item(ui, app, icons::PLUG, "Integrations…", "integrations");
    ui.separator();
    menu_item(ui, app, icons::SEARCH, "Command Palette…", "command_palette");
    ui.menu_button(format!("{}   Help", icons::INFO), |ui| help_menu(ui, app));
}

fn help_menu(ui: &mut Ui, app: &mut InkApp) {
    menu_item(ui, app, icons::BOOK_OPEN, "Welcome Tour", "welcome");
    menu_item(ui, app, icons::KEYBOARD, "Keyboard Shortcuts", "show_shortcuts");
    menu_item(ui, app, icons::BOOK_OPEN, "Documentation", "help_docs");
    menu_item(ui, app, icons::HISTORY, "Release Notes", "help_release_notes");
    menu_item(ui, app, icons::INFO, "Report an Issue", "help_report_issue");
    ui.separator();
    menu_item(ui, app, icons::REFRESH_CW, "Check for Updates…", "check_for_updates");
    menu_item(ui, app, icons::INFO, "About Inkdown", "about");
}

pub fn export_menu(ui: &mut Ui, app: &mut InkApp) {
    ui.set_min_width(240.0);
    let kind = app.docs.get(app.active).map(|d| d.kind);
    let prose = matches!(kind, Some(Kind::Markdown | Kind::Asciidoc));
    let diagram = matches!(kind, Some(Kind::Mermaid | Kind::Dot | Kind::Svg));
    // phones only: the document itself, to where other apps can open it
    menu_item(ui, app, icons::FILE_DOWN, "Save a Copy to Downloads", "export_downloads");
    if prose {
        menu_item(ui, app, icons::GLOBE, "HTML…", "export_html");
        menu_item(ui, app, icons::FILE_DOWN, "PDF…", "export_pdf");
        menu_item(ui, app, icons::FILE_TEXT, "Word (.docx)…", "export_docx");
        menu_item(ui, app, icons::FILE_TEXT, "Rich Text (.rtf)…", "export_rtf");
        ui.separator();
        menu_item(ui, app, icons::CLIPBOARD, "Copy as Rich Text", "export_copy_rich");
        menu_item(ui, app, icons::CODE, "Copy as HTML", "export_copy_html");
        ui.separator();
        menu_item(ui, app, icons::CLOUD_UPLOAD, "Google Docs", "export_gdoc");
    } else if diagram {
        menu_item(ui, app, icons::FILE_IMAGE, "PNG…", "export_png");
        menu_item(ui, app, icons::FILE_IMAGE, "SVG…", "export_svg");
    } else if !crate::picker::available("export_downloads") {
        ui.label(egui::RichText::new("Nothing to export for this file type").color(app.pal.muted).size(12.5));
    }
}

pub fn top_bar(ui: &mut Ui, app: &mut InkApp, narrow: bool) {
    let pal = app.pal;
    egui::Panel::top("top-bar").exact_size(46.0).frame(egui::Frame::new().fill(pal.chrome).inner_margin(egui::Margin::symmetric(8, 0))).show_separator_line(false).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let menu = icon_button(ui, &pal, icons::MENU, "Menu");
            egui::Popup::menu(&menu).show(|ui| main_menu(ui, app));
            if narrow {
                if icon_button(ui, &pal, icons::PANEL_LEFT, "Files and outline").clicked() {
                    app.set.show_sidebar = !app.set.show_sidebar;
                }
            } else {
                if icon_button(ui, &pal, icons::PANEL_LEFT, &format!("Sidebar ({})", app.keymap.label("toggle_sidebar"))).clicked() {
                    app.set.show_sidebar = !app.set.show_sidebar;
                }
                ui.add_space(6.0);
                // AutoSave, as prominent as in Word
                let on = app.set.auto_save;
                ui.label(egui::RichText::new("AutoSave").size(12.5).color(if on { pal.text } else { pal.muted }).strong());
                if switch(ui, &pal, on, "autosave-switch").on_hover_text("Save changes automatically as you type").clicked() {
                    app.pending.push("toggle_auto_save".into());
                }
                ui.label(egui::RichText::new(if on { "On" } else { "Off" }).size(11.5).color(if on { pal.accent } else { pal.faint }));
                ui.add_space(6.0);
                if let Some(d) = app.docs.get(app.active) {
                    let (text, color) = if d.scratch || d.read_only {
                        (String::new(), pal.faint)
                    } else if d.dirty() {
                        (if d.path.is_none() { "Not saved yet".to_string() } else if on { "Saving…".into() } else { "Unsaved changes".into() }, pal.warning)
                    } else if d.path.is_some() {
                        (format!("{}  Saved", icons::CHECK), pal.tip)
                    } else {
                        (String::new(), pal.faint)
                    };
                    if !text.is_empty() {
                        ui.label(egui::RichText::new(text).size(11.5).color(color));
                    }
                }
            }

            // right side
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let dark = pal.dark;
                if icon_button(ui, &pal, if dark { icons::MOON } else { icons::SUN }, "Theme").clicked() {
                    app.pending.push("select_theme".into());
                }
                let exportable = app.docs.get(app.active).is_some_and(|d| !matches!(d.kind, Kind::Code | Kind::Image | Kind::Pdf | Kind::Data(_) | Kind::Table(_)));
                if exportable {
                    let b = egui::Button::new(egui::RichText::new(if narrow { icons::SHARE_2.to_string() } else { format!("{}  Export", icons::SHARE_2) }).size(12.5).color(pal.on_accent())).fill(pal.accent).corner_radius(7).min_size(vec2(0.0, 30.0));
                    let r = ui.add(b);
                    egui::Popup::menu(&r).show(|ui| export_menu(ui, app));
                }
                if icon_button(ui, &pal, icons::SEARCH, &format!("Go to anything ({})", app.keymap.label("goto_anything"))).clicked() {
                    app.pending.push("goto_anything".into());
                }

                // the mode switch takes the centre of what is left
                let modes: Vec<Mode> = app.docs.get(app.active).map(|d| d.kind.modes().to_vec()).unwrap_or_default();
                if modes.len() > 1 {
                    let current = app.docs[app.active].mode;
                    ui.with_layout(Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                        let labels: Vec<(Mode, &str, &str)> = modes
                            .iter()
                            .map(|m| match m {
                                Mode::Read => (*m, icons::EYE, "Read"),
                                Mode::Write => (*m, icons::PEN_LINE, if app.docs[app.active].kind == Kind::Svg { "Design" } else { "Write" }),
                                Mode::Split => (*m, icons::COLUMNS_2, "Split"),
                                Mode::Code => (*m, icons::CODE, "Code"),
                            })
                            .collect();
                        let seg_w = if narrow { 38.0 } else { 78.0 };
                        let total = seg_w * labels.len() as f32 + 6.0;
                        let (rect, _) = ui.allocate_exact_size(vec2(total.min(ui.available_width()), 30.0), Sense::hover());
                        ui.painter().rect_filled(rect, CornerRadius::same(8), pal.chrome2);
                        let ix = labels.iter().position(|l| l.0 == current).unwrap_or(0);
                        let anim = ui.ctx().animate_value_with_time(Id::new("mode-seg"), ix as f32, 0.14);
                        let thumb = Rect::from_min_size(pos2(rect.min.x + 3.0 + anim * seg_w, rect.min.y + 3.0), vec2(seg_w, 24.0));
                        ui.painter().rect_filled(thumb, CornerRadius::same(6), pal.elev);
                        ui.painter().rect_stroke(thumb, CornerRadius::same(6), Stroke::new(1.0, pal.border_soft), egui::StrokeKind::Inside);
                        for (i, (m, icon, label)) in labels.iter().enumerate() {
                            let r = Rect::from_min_size(pos2(rect.min.x + 3.0 + i as f32 * seg_w, rect.min.y + 3.0), vec2(seg_w, 24.0));
                            let resp = ui.interact(r, Id::new(("mode", i)), Sense::click());
                            let on = *m == current;
                            let text = if narrow { icon.to_string() } else { format!("{icon}  {label}") };
                            let g = theme::text(ui, &text, 12.5, if on { pal.text } else { pal.muted }, if on { 620.0 } else { 450.0 });
                            ui.painter().galley(r.center() - g.size() / 2.0, g, pal.text);
                            if resp.clicked() {
                                app.pending.push(format!("mode {}", m.id()));
                            }
                            if resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                        }
                    });
                }
            });
        });
    });
}

pub fn tab_strip(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    if app.docs.is_empty() {
        return;
    }
    let mut activate = None;
    let mut close = None;
    let mut swap: Option<(usize, usize)> = None;
    let mut menu_cmd: Option<(usize, &'static str)> = None;
    egui::Panel::top("tab-strip").exact_size(36.0).frame(egui::Frame::new().fill(pal.chrome)).show_separator_line(false).show(ui, |ui| {
        let strip = ui.max_rect();
        ui.painter().line_segment([strip.left_bottom() - vec2(0.0, 0.5), strip.right_bottom() - vec2(0.0, 0.5)], Stroke::new(1.0, pal.border_soft));
        egui::ScrollArea::horizontal().id_salt("tabs").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.add_space(8.0);
                let pointer = ui.input(|i| i.pointer.interact_pos());
                let mut rects: Vec<Rect> = Vec::with_capacity(app.docs.len());
                let mut dragging = None;
                for (i, d) in app.docs.iter().enumerate() {
                    let active = i == app.active;
                    let dirty = d.dirty();
                    let title = theme::text(ui, &d.name, 12.5, if active { pal.text } else { pal.muted }, if active { 600.0 } else { 450.0 });
                    let tw = title.size().x.min(210.0);
                    let w = tw + 62.0;
                    let (rect, resp) = ui.allocate_exact_size(vec2(w, 36.0), Sense::click_and_drag());
                    rects.push(rect);
                    let body = Rect::from_min_max(pos2(rect.min.x, rect.min.y + 5.0), rect.max);
                    let hover = resp.hovered();
                    if active {
                        ui.painter().rect_filled(body, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, pal.bg);
                        ui.painter().rect_filled(Rect::from_min_size(pos2(body.min.x + 10.0, body.min.y), vec2(body.width() - 20.0, 2.0)), CornerRadius::same(1), pal.accent);
                    } else if hover {
                        ui.painter().rect_filled(body.shrink2(vec2(0.0, 2.0)), CornerRadius::same(7), pal.hover());
                    }
                    let icon = theme::text(ui, crate::sidebar::file_icon(&d.name, false, false), 13.0, if active { pal.accent } else { pal.faint }, 400.0);
                    ui.painter().galley(pos2(rect.min.x + 11.0, body.center().y - icon.size().y / 2.0), icon, pal.faint);
                    ui.painter().with_clip_rect(Rect::from_min_max(pos2(rect.min.x + 30.0, rect.min.y), pos2(rect.max.x - 28.0, rect.max.y))).galley(pos2(rect.min.x + 32.0, body.center().y - title.size().y / 2.0), title, pal.text);
                    // close button, or the unsaved dot
                    let cr = Rect::from_center_size(pos2(rect.max.x - 16.0, body.center().y), vec2(20.0, 20.0));
                    let over_close = pointer.is_some_and(|p| cr.contains(p));
                    if dirty && !over_close {
                        ui.painter().circle_filled(cr.center(), 4.0, pal.accent);
                    } else if hover || active {
                        if over_close {
                            ui.painter().rect_filled(cr, CornerRadius::same(5), pal.active());
                        }
                        let x = theme::text(ui, icons::X, 12.0, pal.muted, 400.0);
                        ui.painter().galley(cr.center() - x.size() / 2.0, x, pal.muted);
                    }
                    if resp.clicked() {
                        if over_close {
                            close = Some(i);
                        } else {
                            activate = Some(i);
                        }
                    }
                    if resp.middle_clicked() {
                        close = Some(i);
                    }
                    if resp.drag_started() {
                        activate = Some(i);
                    }
                    if resp.dragged() {
                        dragging = Some(i);
                    }
                    let tip = d.path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| d.name.clone());
                    let resp = resp.on_hover_text(tip);
                    resp.context_menu(|ui| {
                        ui.set_min_width(180.0);
                        for (label, cmd) in [("Close", "close"), ("Close Others", "close_others"), ("Close All", "close_all"), ("Copy Path", "copy_path"), ("Show in File Manager", "reveal_in_explorer")] {
                            if ui.button(label).clicked() {
                                menu_cmd = Some((i, cmd));
                                ui.close();
                            }
                        }
                    });
                }
                if let (Some(i), Some(p)) = (dragging, pointer) {
                    if i > 0 && p.x < rects[i - 1].center().x {
                        swap = Some((i, i - 1));
                    } else if i + 1 < rects.len() && p.x > rects[i + 1].center().x {
                        swap = Some((i, i + 1));
                    }
                }
                ui.add_space(4.0);
                if ui.add(egui::Button::new(egui::RichText::new(icons::PLUS).size(14.0).color(pal.muted)).frame_when_inactive(false).min_size(vec2(28.0, 26.0))).on_hover_text("New document").clicked() {
                    app.pending.push("new_file".into());
                }
                ui.add_space(8.0);
            });
        });
    });
    if let Some((a, b)) = swap {
        app.docs.swap(a, b);
        if app.active == a {
            app.active = b;
        } else if app.active == b {
            app.active = a;
        }
    }
    if let Some(i) = activate {
        app.active = i;
        if let Some(d) = app.doc() {
            d.ed.focus = d.mode != Mode::Read;
        }
    }
    if let Some((i, cmd)) = menu_cmd {
        app.active = i;
        app.pending.push(cmd.to_string());
    }
    if let Some(i) = close {
        app.close_tab(i);
    }
}

pub fn status_bar(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    egui::Panel::bottom("status-bar").exact_size(25.0).frame(egui::Frame::new().fill(pal.chrome).inner_margin(egui::Margin::symmetric(10, 0))).show_separator_line(false).show(ui, |ui| {
        let r = ui.max_rect();
        ui.painter().line_segment([pos2(r.min.x - 10.0, r.min.y), pos2(r.max.x + 10.0, r.min.y)], Stroke::new(1.0, pal.border_soft));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let text = |s: String| egui::RichText::new(s).size(11.5).color(pal.muted);
            let Some(d) = app.docs.get(app.active) else {
                ui.label(text("Ready".into()));
                return;
            };
            if !d.kind.viewed_only() {
                let s = d.buf.primary();
                let (line, col) = d.buf.line_col(s.head);
                let pos = if d.buf.sels.len() > 1 {
                    format!("{} selections", d.buf.sels.len())
                } else if !s.is_empty() {
                    let lines = d.buf.line_of(s.end()) - d.buf.line_of(s.start());
                    if lines > 0 { format!("{} lines, {} characters selected", lines + 1, s.end() - s.start()) } else { format!("{} characters selected", s.end() - s.start()) }
                } else {
                    format!("Ln {}, Col {}", line + 1, col + 1)
                };
                ui.label(text(pos));
            }
            if d.kind.prose() && d.view.md.words > 0 {
                ui.label(text(format!("{} words · {} min read", d.view.md.words, (d.view.md.words / 220).max(1))));
            } else if !d.kind.viewed_only() {
                ui.label(text(format!("{} lines", d.buf.lines())));
            }
            if app.macro_rec.is_some() {
                ui.label(egui::RichText::new(format!("{}  Recording macro", icons::CIRCLE)).size(11.5).color(pal.danger));
            }
            if let Some(c) = app.keymap.chord_pending() {
                ui.label(egui::RichText::new(format!("{} …", c.label())).size(11.5).color(pal.accent));
            }
            if let Some(p) = d.live.percent() {
                let what = if d.live.remote() { "Fetching".to_string() } else { format!("Reading {} MB", d.live.total >> 20) };
                ui.label(egui::RichText::new(format!("{}  {what}… {p}%", icons::REFRESH_CW)).size(11.5).color(pal.accent));
            } else if d.live.remote() {
                let ago = (app.now - d.live.checked_at).max(0.0);
                let state = match (&d.live.error, app.set.live_refresh) {
                    (Some(_), _) => "Web · cannot refresh".to_string(),
                    (None, false) => "Web · refresh off".to_string(),
                    (None, true) if d.live.streaming => "Web · streaming".to_string(),
                    (None, true) => format!("Web · live, checked {ago:.0} s ago"),
                };
                let r = ui.add(egui::Label::new(egui::RichText::new(format!("{}  {state}", icons::LINK)).size(11.5).color(if d.live.error.is_some() { pal.danger } else { pal.accent })).sense(egui::Sense::click())).on_hover_text(d.live.error.clone().unwrap_or_else(|| d.live.url.clone().unwrap_or_default()));
                if r.clicked() {
                    app.pending.push("reload_now".into());
                }
            } else if d.read_only && !d.kind.viewed_only() {
                ui.label(text(format!("{}  Read-only", icons::LOCK)));
            }
            if d.live.follow {
                let r = ui.add(egui::Label::new(egui::RichText::new(format!("{}  Following", icons::REFRESH_CW)).size(11.5).color(pal.tip)).sense(egui::Sense::click())).on_hover_text("New lines appear as they are written. Click to stop.");
                if r.clicked() {
                    app.pending.push("toggle_follow".into());
                }
            }
            if app.build.running() {
                ui.label(egui::RichText::new(format!("{}  Building…", icons::HAMMER)).size(11.5).color(pal.accent));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let mut link = |ui: &mut Ui, s: String, cmd: &str, tip: &str| {
                    let b = egui::Button::new(egui::RichText::new(s).size(11.5).color(pal.muted)).frame_when_inactive(false).corner_radius(5);
                    if ui.add(b).on_hover_text(tip).clicked() {
                        app.pending.push(cmd.to_string());
                    }
                };
                let roomy = ui.ctx().content_rect().width() >= 620.0;
                if !d.kind.viewed_only() && !roomy {
                    // phones: only the syntax name fits beside the position and word count
                    link(ui, d.hl.name().to_string(), "set_syntax", "Syntax");
                } else if !d.kind.viewed_only() {
                    link(ui, d.hl.name().to_string(), "set_syntax", "Syntax");
                    link(ui, if d.crlf { "CRLF".into() } else { "LF".into() }, "set_line_endings", "Line endings");
                    link(ui, format!("{}{}", d.encoding.name(), if d.bom && d.encoding == encoding_rs::UTF_8 { " BOM" } else { "" }), "set_encoding", "Encoding");
                    link(ui, if d.cfg.soft_tabs { format!("Spaces: {}", d.cfg.tab_width) } else { format!("Tab Size: {}", d.cfg.tab_width) }, "set_indentation", "Indentation");
                } else {
                    ui.label(text(d.kind.label().to_string()));
                }
            });
        });
    });
}

pub fn empty_state(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.22).max(20.0));
        ui.label(egui::RichText::new(icons::PEN_TOOL).size(40.0).color(pal.accent));
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Inkdown").size(26.0).strong());
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Write, read and edit anything").size(13.5).color(pal.muted));
        ui.add_space(20.0);
        for (label, cmd) in [("New Document", "new_file"), ("Open File…", "open_file"), ("Open Folder…", "open_folder"), ("New from Template…", "new_from_template")] {
            let key = app.keymap.label(cmd);
            if ui.add(egui::Button::new(label).shortcut_text(egui::RichText::new(key).size(11.5).color(pal.faint)).min_size(vec2(260.0, 32.0))).clicked() {
                app.pending.push(cmd.to_string());
            }
            ui.add_space(2.0);
        }
        let recent: Vec<String> = app.set.recent_files.iter().take(6).cloned().collect();
        if !recent.is_empty() {
            ui.add_space(16.0);
            ui.label(egui::RichText::new("RECENT").size(10.5).color(pal.faint).strong());
            ui.add_space(4.0);
            for r in recent {
                let name = std::path::Path::new(&r).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if ui.add(egui::Button::new(egui::RichText::new(name).color(pal.accent)).frame_when_inactive(false)).on_hover_text(&r).clicked() {
                    app.pending.push(format!("open {r}"));
                }
            }
        }
    });
}

fn dialog_frame(ctx: &egui::Context, pal: &Palette, id: &str, width: f32, content: impl FnOnce(&mut Ui)) -> bool {
    let modal = egui::Modal::new(Id::new(id)).frame(egui::Frame::popup(&ctx.global_style()).fill(pal.elev).corner_radius(12).inner_margin(20)).backdrop_color(Color32::from_black_alpha(if pal.dark { 120 } else { 60 }));
    let r = modal.show(ctx, |ui| {
        ui.set_width(width.min(ui.ctx().content_rect().width() - 60.0));
        content(ui);
    });
    r.should_close()
}

fn primary(ui: &mut Ui, pal: &Palette, label: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(label).color(pal.on_accent())).fill(pal.accent).min_size(vec2(84.0, 30.0)).corner_radius(7))
}

pub fn dialogs(ctx: &egui::Context, app: &mut InkApp) {
    let Some(dialog) = app.dialog.take() else { return };
    let pal = app.pal;
    let mut keep = true;
    let mut dialog = dialog;
    match &mut dialog {
        Dialog::ConfirmClose(ix) => {
            let ix = *ix;
            let name = app.docs.get(ix).map(|d| d.name.clone()).unwrap_or_default();
            let mut act = 0;
            let esc = dialog_frame(ctx, &pal, "dlg-close", 400.0, |ui| {
                ui.label(egui::RichText::new(format!("Save changes to {name}?")).size(16.0).strong());
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Your changes will be lost if you don't save them.").color(pal.muted));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if primary(ui, &pal, "Save").clicked() {
                        act = 1;
                    }
                    if ui.add(egui::Button::new("Don't Save").min_size(vec2(84.0, 30.0))).clicked() {
                        act = 2;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(vec2(84.0, 30.0))).clicked() {
                        act = 3;
                    }
                });
            });
            match act {
                1 => {
                    if app.save_doc(ix, false) {
                        app.close_tab(ix);
                    }
                    keep = false;
                }
                2 => {
                    if let Some(d) = app.docs.get_mut(ix) {
                        d.buf.mark_saved();
                    }
                    app.close_tab(ix);
                    keep = false;
                }
                3 => keep = false,
                _ => {}
            }
            if esc {
                keep = false;
            }
        }
        Dialog::ConfirmQuit => {
            let open = app.unprotected();
            let names: Vec<String> = open.iter().filter_map(|i| app.docs.get(*i)).map(|d| d.name.clone()).collect();
            let why = if app.set.hot_exit { "They are too large to be kept as drafts, so they will not come back when Inkdown starts again." } else { "Keeping unsaved work for the next start is switched off (\"hot_exit\"), so they will not come back." };
            let mut act = 0;
            let esc = dialog_frame(ctx, &pal, "dlg-quit", 440.0, |ui| {
                ui.label(egui::RichText::new(if names.len() == 1 { format!("Save changes to {}?", names[0]) } else { format!("Save changes to {} documents?", names.len()) }).size(16.0).strong());
                ui.add_space(6.0);
                if names.len() > 1 {
                    ui.label(egui::RichText::new(names.iter().take(6).cloned().collect::<Vec<_>>().join(", ") + if names.len() > 6 { ", …" } else { "" }).color(pal.text));
                    ui.add_space(4.0);
                }
                ui.label(egui::RichText::new(why).color(pal.muted));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if primary(ui, &pal, if names.len() == 1 { "Save" } else { "Save All" }).clicked() {
                        act = 1;
                    }
                    if ui.add(egui::Button::new("Don't Save").min_size(vec2(84.0, 30.0))).clicked() {
                        act = 2;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(vec2(84.0, 30.0))).clicked() {
                        act = 3;
                    }
                });
            });
            match act {
                1 => {
                    for ix in open {
                        if !app.save_doc(ix, false) {
                            break;
                        }
                    }
                    keep = false;
                    // everything is safe now: close; otherwise a save was cancelled, so stay
                    if app.unprotected().is_empty() {
                        app.quit = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                2 => {
                    app.quit_anyway = true;
                    app.quit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    keep = false;
                }
                3 => keep = false,
                _ => {}
            }
            if esc {
                keep = false;
            }
        }
        Dialog::ReloadChanged(ix) => {
            let ix = *ix;
            let name = app.docs.get(ix).map(|d| d.name.clone()).unwrap_or_default();
            let mut act = 0;
            let esc = dialog_frame(ctx, &pal, "dlg-reload", 420.0, |ui| {
                ui.label(egui::RichText::new(format!("{name} changed on disk")).size(16.0).strong());
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Another program modified this file, and you have unsaved changes here.").color(pal.muted));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if primary(ui, &pal, "Keep Mine").clicked() {
                        act = 1;
                    }
                    if ui.add(egui::Button::new("Reload from Disk").min_size(vec2(84.0, 30.0))).clicked() {
                        act = 2;
                    }
                });
            });
            if act == 2 {
                let set = app.set.clone();
                if let Some(d) = app.docs.get_mut(ix) {
                    let _ = d.reload(&set);
                }
            }
            if act != 0 || esc {
                keep = false;
            }
        }
        Dialog::Message(title, body) => {
            let mut ok = false;
            let esc = dialog_frame(ctx, &pal, "dlg-msg", 440.0, |ui| {
                ui.label(egui::RichText::new(title.as_str()).size(16.0).strong());
                ui.add_space(6.0);
                ui.label(egui::RichText::new(body.as_str()).color(pal.muted));
                ui.add_space(16.0);
                ok = primary(ui, &pal, "OK").clicked();
            });
            if ok || esc {
                keep = false;
            }
        }
        Dialog::ConfirmDelete(path) => {
            let mut act = 0;
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let is_dir = path.is_dir();
            let esc = dialog_frame(ctx, &pal, "dlg-del", 420.0, |ui| {
                ui.label(egui::RichText::new(format!("Delete {name}?")).size(16.0).strong());
                ui.add_space(6.0);
                ui.label(egui::RichText::new(if is_dir { "The folder and everything in it will be permanently deleted." } else { "The file will be permanently deleted." }).color(pal.muted));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("Delete").color(Color32::WHITE)).fill(pal.danger).min_size(vec2(84.0, 30.0)).corner_radius(7)).clicked() {
                        act = 1;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(vec2(84.0, 30.0))).clicked() {
                        act = 2;
                    }
                });
            });
            if act == 1 {
                let r = if is_dir { std::fs::remove_dir_all(&*path) } else { std::fs::remove_file(&*path) };
                match r {
                    Ok(()) => {
                        if let Some(i) = app.docs.iter().position(|d| d.path.as_ref() == Some(path)) {
                            app.docs[i].buf.mark_saved();
                            app.close_tab(i);
                        }
                        app.sidebar.refresh();
                    }
                    Err(e) => app.toast(format!("Could not delete: {e}")),
                }
            }
            if act != 0 || esc {
                keep = false;
            }
        }
        Dialog::Rename(path, name) => {
            let mut act = 0;
            let esc = dialog_frame(ctx, &pal, "dlg-rename", 400.0, |ui| {
                ui.label(egui::RichText::new("Rename").size(16.0).strong());
                ui.add_space(8.0);
                let r = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                r.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    act = 1;
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if primary(ui, &pal, "Rename").clicked() {
                        act = 1;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(vec2(84.0, 30.0))).clicked() {
                        act = 2;
                    }
                });
            });
            if act == 1 && !name.trim().is_empty() {
                let target = path.with_file_name(name.trim());
                if target.exists() {
                    app.toast("A file with that name already exists");
                } else {
                    match std::fs::rename(&*path, &target) {
                        Ok(()) => {
                            let set = app.set.clone();
                            if let Some(d) = app.docs.iter_mut().find(|d| d.path.as_ref() == Some(path)) {
                                d.set_path(target, &set);
                            }
                            app.sidebar.refresh();
                        }
                        Err(e) => app.toast(format!("Could not rename: {e}")),
                    }
                }
            }
            if act != 0 || esc {
                keep = false;
            }
        }
        Dialog::OpenUrl(url) => {
            let mut act = 0;
            let esc = dialog_frame(ctx, &pal, "dlg-url", 480.0, |ui| {
                ui.label(egui::RichText::new("Open from Web Address").size(16.0).strong());
                ui.add_space(4.0);
                ui.label(egui::RichText::new("The document is read from the server and refreshed while it is open. A log is followed from its end.").size(11.5).color(pal.faint));
                ui.add_space(8.0);
                let r = ui.add(egui::TextEdit::singleline(url).hint_text("https://example.com/app.log").desired_width(f32::INFINITY));
                r.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    act = 1;
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if primary(ui, &pal, "Open").clicked() {
                        act = 1;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(vec2(84.0, 30.0))).clicked() {
                        act = 2;
                    }
                });
            });
            if act == 1 {
                let address = url.trim().to_string();
                let address = if address.contains("://") || address.is_empty() { address } else { format!("https://{address}") };
                if app.open_url(ctx, &address) {
                    keep = false;
                }
            }
            if act == 2 || esc {
                keep = false;
            }
        }
        Dialog::NewFile(dir, name, is_dir) => {
            let mut act = 0;
            let folder = *is_dir;
            let esc = dialog_frame(ctx, &pal, "dlg-new", 400.0, |ui| {
                ui.label(egui::RichText::new(if folder { "New Folder" } else { "New File" }).size(16.0).strong());
                ui.add_space(4.0);
                ui.label(egui::RichText::new(format!("in {}", dir.display())).size(11.5).color(pal.faint));
                ui.add_space(8.0);
                let r = ui.add(egui::TextEdit::singleline(name).hint_text(if folder { "name" } else { "name.md" }).desired_width(f32::INFINITY));
                r.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    act = 1;
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if primary(ui, &pal, "Create").clicked() {
                        act = 1;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(vec2(84.0, 30.0))).clicked() {
                        act = 2;
                    }
                });
            });
            if act == 1 && !name.trim().is_empty() {
                let target = dir.join(name.trim());
                if target.exists() {
                    app.toast("That name is already taken");
                } else {
                    let r = if folder { std::fs::create_dir_all(&target) } else { std::fs::write(&target, "") };
                    match r {
                        Ok(()) => {
                            app.sidebar.refresh();
                            if !folder {
                                app.open_path(&target);
                            }
                        }
                        Err(e) => app.toast(format!("Could not create: {e}")),
                    }
                }
            }
            if act != 0 || esc {
                keep = false;
            }
        }
        Dialog::About => {
            let mut ok = false;
            let esc = dialog_frame(ctx, &pal, "dlg-about", 400.0, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(icons::PEN_TOOL).size(34.0).color(pal.accent));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Inkdown").size(22.0).strong());
                    ui.label(egui::RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(pal.muted));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("A universal editor: Markdown, code, data and diagrams.\nPure Rust: no web view, no JavaScript.").color(pal.muted));
                    ui.add_space(8.0);
                    if let Some(mb) = crate::platform::memory_mb() {
                        ui.label(egui::RichText::new(format!("Using {mb:.0} MB of memory")).size(11.5).color(pal.faint));
                    }
                    // scroll a document, then open this screen: these show how smooth it was
                    let (build, total, gap) = app.perf;
                    let fps = if gap > 0.0 { 1000.0 / gap } else { 0.0 };
                    ui.label(egui::RichText::new(format!("Frames: build {build:.1} ms · paint {:.1} ms · {gap:.1} ms apart ({fps:.0} per second)", (total - build).max(0.0))).size(11.5).color(pal.faint));
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        // centre the two buttons
                        ui.add_space(((ui.available_width() - 250.0) / 2.0).max(0.0));
                        if ui.add(egui::Button::new("Check for Updates").min_size(vec2(150.0, 30.0)).corner_radius(7)).clicked() {
                            app.pending.push("check_for_updates".into());
                            ok = true;
                        }
                        ok |= primary(ui, &pal, "Close").clicked();
                    });
                });
            });
            if ok || esc {
                keep = false;
            }
        }
        Dialog::Update => {
            use crate::update::State;
            let state = app.updater.state.clone();
            let mut close = false;
            let mut action = "";
            let esc = dialog_frame(ctx, &pal, "dlg-update", 440.0, |ui| {
                ui.label(egui::RichText::new("Software Update").size(17.0).strong());
                ui.add_space(8.0);
                let current = crate::update::current_version();
                let line = |ui: &mut Ui, text: String| {
                    ui.label(egui::RichText::new(text).color(pal.muted));
                };
                match &state {
                    State::Idle | State::Checking => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            line(ui, "Checking for updates…".into());
                        });
                    }
                    State::UpToDate => line(ui, format!("{}  Inkdown {current} is the latest version.", icons::CIRCLE_CHECK)),
                    State::Available(r) => {
                        ui.label(egui::RichText::new(format!("Inkdown {} is available.", r.version)).strong());
                        line(ui, format!("You have version {current}."));
                        if !crate::update::can_self_update() {
                            ui.add_space(4.0);
                            line(ui, "This copy was not installed with the Inkdown installer, so it cannot update itself. Download the new version from the release page.".into());
                        } else if r.installer.is_none() {
                            ui.add_space(4.0);
                            line(ui, "This release has no Windows installer yet. See the release page.".into());
                        }
                    }
                    State::Downloading(r) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            line(ui, format!("Downloading Inkdown {}…", r.version));
                        });
                    }
                    State::Ready(r, _) => {
                        ui.label(egui::RichText::new(format!("Inkdown {} is ready to install.", r.version)).strong());
                        line(ui, "The download was verified. Inkdown will close, update and open again with the same documents.".into());
                    }
                    State::Failed(msg) => {
                        ui.label(egui::RichText::new(format!("{}  {msg}", icons::CIRCLE_ALERT)).color(pal.danger));
                    }
                }
                ui.add_space(16.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    let plain = |ui: &mut Ui, label: &str| ui.add(egui::Button::new(label).min_size(vec2(84.0, 30.0)).corner_radius(7)).clicked();
                    match &state {
                        State::Available(r) => {
                            if crate::update::can_self_update() && r.installer.is_some() {
                                if primary(ui, &pal, "Download and Install").clicked() {
                                    action = "download";
                                }
                            } else if primary(ui, &pal, "Open Download Page").clicked() {
                                action = "page";
                            }
                            if plain(ui, "Release Notes") {
                                action = "notes";
                            }
                            close |= plain(ui, "Later");
                        }
                        State::Ready(..) => {
                            if primary(ui, &pal, "Restart and Update").clicked() {
                                action = "install";
                            }
                            close |= plain(ui, "Later");
                        }
                        State::Failed(_) => {
                            if primary(ui, &pal, "Try Again").clicked() {
                                action = "retry";
                            }
                            if plain(ui, "Open Download Page") {
                                action = "page";
                            }
                            close |= plain(ui, "Close");
                        }
                        State::UpToDate => {
                            if plain(ui, "Release Notes") {
                                action = "page";
                            }
                            close |= primary(ui, &pal, "Close").clicked();
                        }
                        _ => close |= plain(ui, "Hide"),
                    }
                });
            });
            match action {
                "download" => app.updater.download(ctx),
                "retry" => app.updater.check(ctx, true),
                "page" => crate::shell_open(&crate::update::releases_url()),
                "notes" => {
                    if let Some(r) = app.updater.available() {
                        crate::shell_open(&r.page);
                    }
                }
                "install" => {
                    // unsaved work is kept: the session is written before the window closes
                    if app.updater.install_and_restart() {
                        app.pending.push("quit".into());
                        close = true;
                    }
                }
                _ => {}
            }
            if app.updater.busy() {
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
            if close || esc {
                keep = false;
            }
        }
        Dialog::Shortcuts => {
            let mut close = false;
            let esc = dialog_frame(ctx, &pal, "dlg-keys", 560.0, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Keyboard Shortcuts").size(16.0).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(icons::X).frame_when_inactive(false)).clicked() {
                            close = true;
                        }
                        if ui.button("Customise…").clicked() {
                            app.pending.push("open_keybindings".into());
                            close = true;
                        }
                    });
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical().max_height((ui.ctx().content_rect().height() - 220.0).max(160.0)).show(ui, |ui| {
                    let mut last_group = "";
                    for (id, title, _) in crate::picker::COMMANDS.iter().filter(|c| crate::picker::available(c.0)) {
                        let key = app.keymap.label(id.split(' ').next().unwrap_or(id));
                        if key.is_empty() {
                            continue;
                        }
                        let (group, name) = title.split_once(": ").unwrap_or(("", title));
                        if group != last_group {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new(group.to_uppercase()).size(10.5).color(pal.faint).strong());
                            last_group = group;
                        }
                        ui.horizontal(|ui| {
                            ui.label(name.trim_end_matches('…'));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(egui::RichText::new(key).monospace().size(12.0).color(pal.accent));
                            });
                        });
                    }
                });
            });
            if close || esc {
                keep = false;
            }
        }
        Dialog::Integrations => {
            if crate::export_ui::integrations_dialog(ctx, app) {
                keep = false;
            }
        }
    }
    if keep && app.dialog.is_none() {
        app.dialog = Some(dialog);
    }
}

pub fn toasts(ctx: &egui::Context, app: &mut InkApp) {
    if app.toasts.is_empty() {
        return;
    }
    let now = ctx.input(|i| i.time);
    for t in &mut app.toasts {
        if t.1 < 0.0 {
            t.1 = now + 2.8 + (t.0.len() as f64 / 40.0).min(3.0);
        }
    }
    app.toasts.retain(|t| t.1 > now);
    let pal = app.pal;
    let screen = ctx.content_rect();
    let (bg, fg) = if pal.dark { (theme::rgb(0xeef0f3), theme::rgb(0x1b1d22)) } else { (theme::rgb(0x23262d), theme::rgb(0xf4f5f7)) };
    let mut y = screen.max.y - 54.0;
    for (i, (msg, until)) in app.toasts.iter().enumerate().rev().take(4) {
        let alpha = ((until - now) / 0.25).clamp(0.0, 1.0) as f32;
        let mut job = egui::text::LayoutJob::single_section(msg.clone(), theme::fmt(13.0, fg.gamma_multiply(alpha), 500.0));
        job.wrap.max_width = (screen.width() - 80.0).min(560.0);
        let g = ctx.fonts_mut(|f| f.layout_job(job));
        let size = g.size() + vec2(28.0, 18.0);
        let rect = Rect::from_center_size(pos2(screen.center().x, y - size.y / 2.0), size);
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new(("toast", i))));
        p.rect_filled(rect, CornerRadius::same(9), bg.gamma_multiply(alpha));
        p.galley(rect.min + vec2(14.0, 9.0), g, fg);
        y -= size.y + 8.0;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
}

pub fn lightbox(ctx: &egui::Context, app: &mut InkApp) {
    let Some((key, mut view)) = app.lightbox.take() else { return };
    let pal = app.pal;
    let screen = ctx.content_rect();
    let Some(asset) = app.assets.peek(key) else { return };
    let (tex, size, svg) = (asset.tex.id(), asset.size, asset.svg.clone());
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    egui::Area::new(Id::new("ink-lightbox")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
        ui.painter().rect_filled(rect, 0, Color32::from_black_alpha(215));
        if view.zoom <= 0.0 {
            view.zoom = ((rect.width() - 80.0) / size.x).min((rect.height() - 80.0) / size.y).clamp(0.05, 3.0);
        }
        if resp.dragged() {
            view.pan += resp.drag_delta();
        }
        let (scroll, zoom, ptr) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos()));
        let factor = if zoom != 1.0 { zoom } else { (scroll * 0.0025).exp() };
        if factor != 1.0 {
            let old = view.zoom;
            view.zoom = (view.zoom * factor).clamp(0.05, 12.0);
            if let Some(p) = ptr {
                let rel = p - rect.center() - view.pan;
                view.pan -= rel * (view.zoom / old - 1.0);
            }
        }
        let shown = size * view.zoom;
        let r = Rect::from_center_size(rect.center() + view.pan, shown);
        // diagrams are drawn for a light page
        ui.painter().rect_filled(r.expand(14.0), CornerRadius::same(10), if pal.dark && svg.is_some() { theme::rgb(0x1b1e24) } else { Color32::WHITE });
        egui::Image::from_texture((tex, shown)).paint_at(ui, r);
        if resp.clicked() && !ptr.is_some_and(|p| r.expand(14.0).contains(p)) {
            close = true;
        }
        let hint = theme::text(ui, "Scroll to zoom · drag to pan · Esc to close", 12.0, Color32::from_white_alpha(170), 450.0);
        ui.painter().galley(pos2(rect.center().x - hint.size().x / 2.0, rect.max.y - 34.0), hint, Color32::WHITE);
        let cr = Rect::from_min_size(pos2(rect.max.x - 52.0, rect.min.y + 16.0), vec2(36.0, 36.0));
        let cresp = ui.interact(cr, Id::new("lightbox-x"), Sense::click());
        ui.painter().rect_filled(cr, CornerRadius::same(18), Color32::from_white_alpha(if cresp.hovered() { 60 } else { 30 }));
        let x = theme::text(ui, icons::X, 16.0, Color32::WHITE, 500.0);
        ui.painter().galley(cr.center() - x.size() / 2.0, x, Color32::WHITE);
        if cresp.clicked() {
            close = true;
        }
    });
    if !close {
        app.lightbox = Some((key, view));
    }
}
