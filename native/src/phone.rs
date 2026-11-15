//! The interface on a phone: a title bar, the modes along the bottom edge where a thumb
//! reaches them, a formatting strip above the keyboard, and bottom sheets with large rows
//! in place of desktop menus.
//!
//! Set `INKDOWN_PHONE=1` to use this layout in a narrow desktop window (used by the tests).

use egui::{pos2, vec2, Align, Color32, CornerRadius, Id, Layout, Rect, Sense, Stroke, Ui};

use crate::app::InkApp;
use crate::doc::{Kind, Mode};
use crate::icons;
use crate::theme::{self, Palette};

/// Smallest comfortable touch target, in points.
const TOUCH: f32 = 46.0;

/// Which bottom sheet is open.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sheet {
    /// The main menu.
    Menu,
    /// Open documents.
    Documents,
    Export,
    Tools,
    Settings,
    Help,
}

/// True when the phone layout should be used for this window.
pub fn active(ctx: &egui::Context) -> bool {
    let forced = std::env::var_os("INKDOWN_PHONE").is_some();
    // the short side decides: a phone held sideways is wide, but it is still a phone
    let size = ctx.content_rect().size();
    (cfg!(any(target_os = "android", target_os = "ios")) || forced) && size.x.min(size.y) < 600.0
}

/// Bigger targets and text for fingers. Called when the style has just been applied.
pub fn tune_style(ctx: &egui::Context) {
    ctx.global_style_mut(|s| {
        s.spacing.interact_size.y = 40.0;
        s.spacing.button_padding = vec2(12.0, 9.0);
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.scroll.bar_width = 4.0;
    });
}

fn icon_tap(ui: &mut Ui, pal: &Palette, icon: &str, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(TOUCH, TOUCH), Sense::click());
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect.shrink(4.0), CornerRadius::same(20), pal.active());
    }
    let g = theme::text(ui, icon, 20.0, color, 400.0);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, color);
    resp
}

/// Title bar: drawer, document name (tap to switch documents), find and the menu.
pub fn top_bar(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    egui::Panel::top("phone-top").exact_size(54.0).frame(egui::Frame::new().fill(pal.chrome).inner_margin(egui::Margin::symmetric(4, 0))).show_separator_line(false).show(ui, |ui| {
        let r = ui.max_rect();
        ui.painter().line_segment([pos2(r.min.x - 4.0, r.max.y - 0.5), pos2(r.max.x + 4.0, r.max.y - 0.5)], Stroke::new(1.0, pal.border_soft));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if icon_tap(ui, &pal, icons::MENU, pal.text).clicked() {
                app.set.show_sidebar = !app.set.show_sidebar;
                app.overlay_opened = app.frame_count;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if icon_tap(ui, &pal, icons::ELLIPSIS_VERTICAL, pal.text).clicked() {
                    app.sheet = Some(Sheet::Menu);
                    app.overlay_opened = app.frame_count;
                }
                if icon_tap(ui, &pal, icons::SEARCH, pal.text).clicked() {
                    // on a phone the magnifier means: find in what I am reading
                    app.pending.push("show_find".into());
                }
                // the title takes what is left
                let (name, sub, dirty) = match app.docs.get(app.active) {
                    Some(d) => {
                        let state = if d.read_only { "Read-only" } else if d.dirty() { "Edited" } else if d.path.is_some() { "Saved" } else { "Not saved yet" };
                        (d.name.clone(), format!("{} · {}", d.kind.label(), state), d.dirty())
                    }
                    None => ("Inkdown".to_string(), String::new(), false),
                };
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH), Sense::click());
                let clip = ui.painter().with_clip_rect(rect);
                let more = if app.docs.len() > 1 { format!("  {}", icons::CHEVRON_DOWN) } else { String::new() };
                let title = theme::text(ui, &format!("{name}{more}"), 16.0, pal.text, 620.0);
                clip.galley(pos2(rect.min.x + 6.0, rect.center().y - 17.0), title, pal.text);
                let sub_color = if dirty { pal.warning } else { pal.muted };
                let subtitle = theme::text(ui, &sub, 11.5, sub_color, 450.0);
                clip.galley(pos2(rect.min.x + 6.0, rect.center().y + 3.0), subtitle, sub_color);
                if resp.clicked() {
                    app.sheet = Some(Sheet::Documents);
                    app.overlay_opened = app.frame_count;
                }
            });
        });
    });
}

/// The modes this document offers on a phone. Split needs more width than a phone has.
fn phone_modes(kind: Kind) -> Vec<Mode> {
    kind.modes().iter().copied().filter(|m| *m != Mode::Split).collect()
}

/// Bottom edge: the formatting strip while editing, and the mode switch.
pub fn bottom_bar(ui: &mut Ui, app: &mut InkApp, inset_bottom: f32) {
    let pal = app.pal;
    let Some(d) = app.docs.get(app.active) else { return };
    let (kind, mode, read_only) = (d.kind, d.mode, d.read_only);
    let modes = phone_modes(kind);
    // a phone has no room for Split: show the nearest mode instead
    if mode == Mode::Split && modes.len() > 1 {
        app.pending.push(format!("mode {}", if modes.contains(&Mode::Read) { "read" } else { "code" }));
    }
    let editing = !read_only && matches!(mode, Mode::Write | Mode::Code);
    // with the keyboard up, the mode switch gives its room to the text
    let keyboard = ui.ctx().content_rect().height() < 420.0 || inset_bottom > 180.0;

    if modes.len() > 1 && !keyboard {
        egui::Panel::bottom("phone-modes").exact_size(58.0).frame(egui::Frame::new().fill(pal.chrome)).show_separator_line(false).show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, pal.border_soft));
            let w = r.width() / modes.len() as f32;
            for (i, m) in modes.iter().enumerate() {
                let cell = Rect::from_min_size(pos2(r.min.x + i as f32 * w, r.min.y), vec2(w, r.height()));
                let resp = ui.interact(cell, Id::new(("phone-mode", i)), Sense::click());
                let on = *m == mode;
                let (icon, label) = match m {
                    Mode::Read => (icons::EYE, "Read"),
                    Mode::Write => (icons::PEN_LINE, if kind == Kind::Svg { "Design" } else { "Write" }),
                    Mode::Split => (icons::COLUMNS_2, "Split"),
                    Mode::Code => (icons::CODE, if kind == Kind::Markdown { "Markdown" } else { "Source" }),
                };
                let color = if on { pal.accent } else { pal.muted };
                if on {
                    let pill = Rect::from_center_size(pos2(cell.center().x, cell.min.y + 19.0), vec2(58.0, 28.0));
                    ui.painter().rect_filled(pill, CornerRadius::same(14), theme::mix(pal.chrome, pal.accent, 0.18));
                }
                let g = theme::text(ui, icon, 19.0, color, 400.0);
                ui.painter().galley(pos2(cell.center().x - g.size().x / 2.0, cell.min.y + 19.0 - g.size().y / 2.0), g, color);
                let g = theme::text(ui, label, 11.5, color, if on { 650.0 } else { 480.0 });
                ui.painter().galley(pos2(cell.center().x - g.size().x / 2.0, cell.min.y + 37.0), g, color);
                if resp.clicked() && !on {
                    app.pending.push(format!("mode {}", m.id()));
                }
            }
        });
    }

    if editing {
        egui::Panel::bottom("phone-format").exact_size(48.0).frame(egui::Frame::new().fill(pal.chrome2)).show_separator_line(false).show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, pal.border_soft));
            egui::ScrollArea::horizontal().id_salt("phone-format-scroll").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let mut items: Vec<(&str, &str)> = vec![(icons::UNDO_2, "undo"), (icons::REDO_2, "redo")];
                    if kind == Kind::Markdown {
                        items.extend([
                            (icons::BOLD, "md_bold"),
                            (icons::ITALIC, "md_italic"),
                            (icons::HEADING_1, "md_h1"),
                            (icons::HEADING_2, "md_h2"),
                            (icons::LIST, "md_ul"),
                            (icons::LIST_ORDERED, "md_ol"),
                            (icons::LIST_CHECKS, "md_task"),
                            (icons::QUOTE, "md_quote"),
                            (icons::LINK, "md_link"),
                            (icons::CODE, "md_code"),
                            (icons::STRIKETHROUGH, "md_strike"),
                            (icons::HIGHLIGHTER, "md_highlight"),
                            (icons::IMAGE, "md_image"),
                            (icons::TABLE, "md_table"),
                            (icons::SQUARE_CODE, "md_codeblock"),
                            (icons::SIGMA, "md_math"),
                            (icons::WORKFLOW, "md_mermaid"),
                        ]);
                    }
                    items.extend([(icons::CLIPBOARD, "paste"), (icons::COPY, "copy"), (icons::SCISSORS, "cut"), (icons::TEXT_SELECT, "select_all")]);
                    for (icon, cmd) in items {
                        if icon_tap(ui, &pal, icon, pal.text).clicked() {
                            app.pending.push(cmd.to_string());
                        }
                    }
                });
            });
        });
    }
}

struct Row<'a> {
    icon: &'a str,
    label: String,
    detail: String,
    /// Shown as a row that leads to another sheet.
    more: bool,
}

/// One sheet row. Returns true when tapped.
fn row(ui: &mut Ui, pal: &Palette, r: Row<'_>) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
    if resp.is_pointer_button_down_on() || resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(10), pal.active());
    }
    let g = theme::text(ui, r.icon, 19.0, pal.muted, 400.0);
    ui.painter().galley(pos2(rect.min.x + 16.0, rect.center().y - g.size().y / 2.0), g, pal.muted);
    let clip = ui.painter().with_clip_rect(Rect::from_min_max(rect.min, pos2(rect.max.x - 44.0, rect.max.y)));
    let g = theme::text(ui, &r.label, 15.5, pal.text, 480.0);
    let y = if r.detail.is_empty() { rect.center().y - g.size().y / 2.0 } else { rect.center().y - 17.0 };
    clip.galley(pos2(rect.min.x + 54.0, y), g, pal.text);
    if !r.detail.is_empty() {
        let g = theme::text(ui, &r.detail, 11.5, pal.muted, 450.0);
        clip.galley(pos2(rect.min.x + 54.0, rect.center().y + 3.0), g, pal.muted);
    }
    if r.more {
        let g = theme::text(ui, icons::CHEVRON_RIGHT, 16.0, pal.faint, 400.0);
        ui.painter().galley(pos2(rect.max.x - 30.0, rect.center().y - g.size().y / 2.0), g, pal.faint);
    }
    resp.clicked()
}

fn command_row(ui: &mut Ui, app: &mut InkApp, icon: &str, label: &str, cmd: &str) {
    if !crate::picker::available(cmd) {
        return;
    }
    let pal = app.pal;
    if row(ui, &pal, Row { icon, label: label.to_string(), detail: String::new(), more: false }) {
        app.pending.push(cmd.to_string());
        app.sheet = None;
    }
}

fn nav_row(ui: &mut Ui, app: &mut InkApp, icon: &str, label: &str, to: Sheet) {
    let pal = app.pal;
    if row(ui, &pal, Row { icon, label: label.to_string(), detail: String::new(), more: true }) {
        app.sheet = Some(to);
    }
}

fn divider(ui: &mut Ui, pal: &Palette) {
    ui.add_space(4.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r.shrink2(vec2(16.0, 0.0)), 0, pal.border_soft);
    ui.add_space(4.0);
}

/// The open bottom sheet, if any.
pub fn sheet(ctx: &egui::Context, app: &mut InkApp, inset_bottom: f32) {
    let Some(which) = app.sheet else { return };
    let pal = app.pal;
    let screen = ctx.content_rect();
    ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, Id::new("sheet-dim"))).rect_filled(screen, 0, Color32::from_black_alpha(110));
    // The sheet sits on the bottom edge, as tall as its rows need, up to most of the screen
    // (then the rows scroll).
    let titled = which != Sheet::Menu;
    let header = 14.0 + if titled { TOUCH + 8.0 } else { 0.0 };
    let list_h = rows_height(which, app).min((screen.height() - inset_bottom - header - 90.0).max(160.0));
    let total = header + list_h + inset_bottom + 18.0;
    let area = egui::Area::new(Id::new("phone-sheet")).order(egui::Order::Foreground).fixed_pos(pos2(screen.min.x, screen.max.y - total)).constrain(false).show(ctx, |ui| {
        egui::Frame::new().fill(pal.elev).corner_radius(CornerRadius { nw: 18, ne: 18, sw: 0, se: 0 }).inner_margin(egui::Margin { left: 6, right: 6, top: 6, bottom: 6 }).show(ui, |ui| {
            ui.set_width(screen.width() - 12.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            // grab handle
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
            ui.painter().rect_filled(Rect::from_center_size(r.center(), vec2(38.0, 4.0)), CornerRadius::same(2), pal.border_soft);
            let title = match which {
                Sheet::Menu => "",
                Sheet::Documents => "Open documents",
                Sheet::Export => "Share and export",
                Sheet::Tools => "Tools",
                Sheet::Settings => "Settings",
                Sheet::Help => "Help",
            };
            if !title.is_empty() {
                ui.horizontal(|ui| {
                    if which != Sheet::Documents && icon_tap(ui, &pal, icons::CHEVRON_LEFT, pal.text).clicked() {
                        app.sheet = Some(Sheet::Menu);
                    }
                    if which == Sheet::Documents {
                        ui.add_space(16.0);
                    }
                    ui.label(egui::RichText::new(title).size(17.0).strong().color(pal.text));
                });
                ui.add_space(4.0);
            }
            egui::ScrollArea::vertical().max_height(list_h).min_scrolled_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(ui.available_width());
                match which {
                    Sheet::Menu => menu_rows(ui, app),
                    Sheet::Documents => document_rows(ui, app),
                    Sheet::Export => export_rows(ui, app),
                    Sheet::Tools => {
                        for (id, title, icon) in crate::picker::COMMANDS.iter().filter(|c| c.0.starts_with("tool_")) {
                            command_row(ui, app, icon, title.trim_start_matches("Tools: "), id);
                        }
                    }
                    Sheet::Settings => {
                        command_row(ui, app, icons::PALETTE, "Theme", "select_theme");
                        command_row(ui, app, icons::ZOOM_IN, "Larger text", "increase_font_size");
                        command_row(ui, app, icons::ZOOM_OUT, "Smaller text", "decrease_font_size");
                        command_row(ui, app, icons::WRAP_TEXT, "Word wrap", "toggle_word_wrap");
                        command_row(ui, app, icons::HASH, "Line numbers", "toggle_line_numbers");
                        command_row(ui, app, icons::REFRESH_CW, if app.docs.get(app.active).is_some_and(|d| d.live.follow) { "Follow new lines: on" } else { "Follow new lines: off" }, "toggle_follow");
                        command_row(ui, app, icons::SAVE, if app.set.auto_save { "AutoSave: on" } else { "AutoSave: off" }, "toggle_auto_save");
                        divider(ui, &pal);
                        command_row(ui, app, icons::SETTINGS, "All settings (as a file)", "open_settings");
                        command_row(ui, app, icons::ZAP, "Snippets", "open_snippets");
                    }
                    Sheet::Help => {
                        command_row(ui, app, icons::BOOK_OPEN, "Welcome tour", "welcome");
                        command_row(ui, app, icons::BOOK_OPEN, "Documentation", "help_docs");
                        command_row(ui, app, icons::HISTORY, "Release notes", "help_release_notes");
                        command_row(ui, app, icons::INFO, "Report an issue", "help_report_issue");
                        command_row(ui, app, icons::REFRESH_CW, "Check for updates", "check_for_updates");
                        command_row(ui, app, icons::INFO, "About Inkdown", "about");
                    }
                }
            });
            ui.add_space(inset_bottom + 6.0);
        });
    });
    let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p)));
    if (outside && app.overlay_opened != app.frame_count) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.sheet = None;
    }
}

/// Height the rows of a sheet need.
fn rows_height(which: Sheet, app: &InkApp) -> f32 {
    let have = |cmds: &[&str]| cmds.iter().filter(|c| crate::picker::available(c)).count() as f32;
    const ROW: f32 = 52.0;
    const DIVIDER: f32 = 9.0;
    match which {
        Sheet::Menu => 16.0 * ROW + 4.0 * DIVIDER,
        Sheet::Documents => app.docs.len() as f32 * 54.0 + DIVIDER + 2.0 * ROW,
        Sheet::Export => {
            let kind = app.docs.get(app.active).map(|d| d.kind);
            let own = have(&["export_downloads"]);
            if matches!(kind, Some(Kind::Markdown | Kind::Asciidoc)) {
                (own + have(&["export_pdf", "export_html", "export_docx", "export_rtf", "export_copy_html", "export_copy_rich", "export_gdoc"])) * ROW
            } else if matches!(kind, Some(Kind::Mermaid | Kind::Dot | Kind::Svg)) {
                (own + 2.0) * ROW
            } else {
                own.max(1.0) * ROW
            }
        }
        Sheet::Tools => crate::picker::COMMANDS.iter().filter(|c| c.0.starts_with("tool_") && crate::picker::available(c.0)).count() as f32 * ROW,
        Sheet::Settings => 9.0 * ROW + DIVIDER,
        Sheet::Help => 6.0 * ROW,
    }
}

fn menu_rows(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    command_row(ui, app, icons::FILE_PLUS, "New document", "new_file");
    command_row(ui, app, icons::LAYOUT_TEMPLATE, "New from template", "new_from_template");
    command_row(ui, app, icons::FOLDER_OPEN, "Open", "open_file");
    command_row(ui, app, icons::LINK, "Open from web address", "open_url");
    command_row(ui, app, icons::HISTORY, "Recent", "open_recent");
    divider(ui, &pal);
    command_row(ui, app, icons::SAVE, "Save", "save");
    command_row(ui, app, icons::SAVE, "Save as", "save_as");
    nav_row(ui, app, icons::SHARE_2, "Share and export", Sheet::Export);
    divider(ui, &pal);
    command_row(ui, app, icons::FOLDER, "File in Notes", "notes_file");
    command_row(ui, app, icons::FOLDER_OPEN, "Browse notes", "notes_open");
    divider(ui, &pal);
    command_row(ui, app, icons::SEARCH, "Find in document", "show_find");
    command_row(ui, app, icons::REPLACE, "Replace", "show_replace");
    nav_row(ui, app, icons::WRENCH, "Tools", Sheet::Tools);
    command_row(ui, app, icons::SEARCH, "All commands", "command_palette");
    divider(ui, &pal);
    nav_row(ui, app, icons::SETTINGS, "Settings", Sheet::Settings);
    nav_row(ui, app, icons::INFO, "Help", Sheet::Help);
}

fn export_rows(ui: &mut Ui, app: &mut InkApp) {
    let kind = app.docs.get(app.active).map(|d| d.kind);
    let prose = matches!(kind, Some(Kind::Markdown | Kind::Asciidoc));
    let diagram = matches!(kind, Some(Kind::Mermaid | Kind::Dot | Kind::Svg));
    command_row(ui, app, icons::FILE_DOWN, "Save a copy to Downloads", "export_downloads");
    if prose {
        command_row(ui, app, icons::FILE_DOWN, "Print or save as PDF", "export_pdf");
        command_row(ui, app, icons::GLOBE, "Web page (HTML)", "export_html");
        command_row(ui, app, icons::FILE_TEXT, "Word document", "export_docx");
        command_row(ui, app, icons::FILE_TEXT, "Rich text (RTF)", "export_rtf");
        command_row(ui, app, icons::CODE, "Copy as HTML", "export_copy_html");
        command_row(ui, app, icons::CLIPBOARD, "Copy as rich text", "export_copy_rich");
        command_row(ui, app, icons::CLOUD_UPLOAD, "Google Docs", "export_gdoc");
    } else if diagram {
        command_row(ui, app, icons::FILE_IMAGE, "Picture (PNG)", "export_png");
        command_row(ui, app, icons::FILE_IMAGE, "Drawing (SVG)", "export_svg");
    }
}

fn document_rows(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    let mut activate = None;
    let mut close = None;
    for (i, d) in app.docs.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 54.0), Sense::click());
        let on = i == app.active;
        if on {
            ui.painter().rect_filled(rect, CornerRadius::same(10), theme::mix(pal.elev, pal.accent, 0.14));
        }
        let color = if on { pal.accent } else { pal.muted };
        let g = theme::text(ui, icons::FILE_TEXT, 19.0, color, 400.0);
        ui.painter().galley(pos2(rect.min.x + 16.0, rect.center().y - g.size().y / 2.0), g, color);
        let clip = ui.painter().with_clip_rect(Rect::from_min_max(rect.min, pos2(rect.max.x - TOUCH - 4.0, rect.max.y)));
        let g = theme::text(ui, &d.name, 15.5, pal.text, if on { 620.0 } else { 480.0 });
        clip.galley(pos2(rect.min.x + 54.0, rect.center().y - 17.0), g, pal.text);
        let state = if d.read_only { "Read-only" } else if d.dirty() { "Edited" } else if d.path.is_some() { "Saved" } else { "Not saved yet" };
        let g = theme::text(ui, &format!("{} · {}", d.kind.label(), state), 11.5, pal.muted, 450.0);
        clip.galley(pos2(rect.min.x + 54.0, rect.center().y + 3.0), g, pal.muted);
        // close button
        let x = Rect::from_min_size(pos2(rect.max.x - TOUCH - 2.0, rect.center().y - TOUCH / 2.0), vec2(TOUCH, TOUCH));
        let xr = ui.interact(x, Id::new(("phone-doc-close", i)), Sense::click());
        let g = theme::text(ui, icons::X, 16.0, pal.muted, 400.0);
        ui.painter().galley(x.center() - g.size() / 2.0, g, pal.muted);
        if xr.clicked() {
            close = Some(i);
        } else if resp.clicked() {
            activate = Some(i);
        }
    }
    divider(ui, &pal);
    command_row(ui, app, icons::FILE_PLUS, "New document", "new_file");
    command_row(ui, app, icons::FOLDER_OPEN, "Open", "open_file");
    if let Some(i) = activate {
        app.active = i;
        app.sheet = None;
    }
    if let Some(i) = close {
        app.pending.push(format!("select_tab_{}", i + 1));
        app.pending.push("close".into());
        if app.docs.len() <= 1 {
            app.sheet = None;
        }
    }
}
