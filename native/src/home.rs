//! The home page: what the window shows when no document is open.
//!
//! Actions as tiles, then the recent files and folders as rows that say what each one is,
//! where it lives and when it last changed.

use std::path::Path;
use std::time::SystemTime;

use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Id, Rect, Sense, Stroke, Ui};

use crate::app::InkApp;
use crate::icons;
use crate::theme::Palette;

/// What is known about a recent entry, looked up now and then (not every frame).
#[derive(Clone)]
struct Entry {
    path: String,
    name: String,
    place: String,
    folder: bool,
    exists: bool,
    modified: Option<SystemTime>,
    size: u64,
}

#[derive(Clone, Default)]
struct Cache {
    key: u64,
    at: f64,
    entries: Vec<Entry>,
}

/// `C:\Users\me\Documents\notes` → `~\Documents\notes`.
pub fn friendly_place(path: &Path) -> String {
    let parent = path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    shorten_home(&parent, &home)
}

/// Replaces a leading home folder with `~`, whichever way the slashes lean.
pub fn shorten_home(parent: &str, home: &str) -> String {
    let same = |a: &str, b: &str| a.len() == b.len() && a.chars().zip(b.chars()).all(|(x, y)| x.eq_ignore_ascii_case(&y) || (matches!(x, '/' | '\\') && matches!(y, '/' | '\\')));
    let home = home.trim_end_matches(['/', '\\']);
    if !home.is_empty() && parent.len() >= home.len() && parent.is_char_boundary(home.len()) && same(&parent[..home.len()], home) && parent[home.len()..].chars().next().is_none_or(|c| matches!(c, '/' | '\\')) {
        return format!("~{}", &parent[home.len()..]);
    }
    parent.to_string()
}

/// "just now", "5 minutes ago", "yesterday", "3 weeks ago".
pub fn ago(seconds: u64) -> String {
    let plural = |n: u64, unit: &str| if n == 1 { format!("1 {unit} ago") } else { format!("{n} {unit}s ago") };
    match seconds {
        0..=59 => "just now".into(),
        60..=3599 => plural(seconds / 60, "minute"),
        3600..=86_399 => plural(seconds / 3600, "hour"),
        86_400..=172_799 => "yesterday".into(),
        172_800..=1_209_599 => plural(seconds / 86_400, "day"),
        1_209_600..=5_183_999 => plural(seconds / 604_800, "week"),
        5_184_000..=63_071_999 => plural(seconds / 2_592_000, "month"),
        _ => plural(seconds / 31_536_000, "year"),
    }
}

pub fn size_label(bytes: u64) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1_048_575 => format!("{:.0} KB", bytes as f64 / 1024.0),
        1_048_576..=1_073_741_823 => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
        _ => format!("{:.2} GB", bytes as f64 / 1_073_741_824.0),
    }
}

/// Shortens `text` in the middle until it fits `width`.
fn fit_middle(ui: &Ui, text: &str, font: &FontId, width: f32) -> String {
    let measure = |s: &str| ui.painter().layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE).size().x;
    if measure(text) <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len().saturating_sub(2);
    while keep > 6 {
        let head = keep / 3;
        let tail = keep - head;
        let s: String = chars[..head].iter().chain(['…'].iter()).chain(chars[chars.len() - tail..].iter()).collect();
        if measure(&s) <= width {
            return s;
        }
        keep -= 2;
    }
    chars.iter().take(6).collect()
}

fn entries(ui: &Ui, app: &InkApp) -> Vec<Entry> {
    let id = Id::new("home-recent");
    let key = crate::assets::hash64(&[&app.set.recent_files, &app.set.recent_folders]);
    let cached: Cache = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    if cached.key == key && app.now - cached.at < 3.0 && cached.at > 0.0 {
        return cached.entries;
    }
    let look = |path: &String, folder: bool| {
        let p = Path::new(path);
        let meta = std::fs::metadata(p).ok();
        Entry {
            path: path.clone(),
            name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.clone()),
            place: friendly_place(p),
            folder,
            exists: meta.as_ref().is_some_and(|m| m.is_dir() == folder),
            modified: meta.as_ref().and_then(|m| m.modified().ok()),
            size: meta.map(|m| m.len()).unwrap_or(0),
        }
    };
    let mut out: Vec<Entry> = app.set.recent_files.iter().take(30).map(|p| look(p, false)).collect();
    out.extend(app.set.recent_folders.iter().take(12).map(|p| look(p, true)));
    let fresh = Cache { key, at: app.now.max(0.001), entries: out.clone() };
    ui.data_mut(|d| d.insert_temp(id, fresh));
    out
}

enum Act {
    Open(String),
    Forget(String, bool),
}

/// One recent file or folder. Returns what the user asked for.
fn row(ui: &mut Ui, pal: &Palette, e: &Entry, width: f32) -> Option<Act> {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 48.0), Sense::click());
    let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
    if hovered {
        ui.painter().rect_filled(rect, CornerRadius::same(8), pal.hover());
    }
    let dim = if e.exists { pal.text } else { pal.faint };
    let icon = crate::sidebar::file_icon(&e.name, e.folder, false);
    ui.painter().text(pos2(rect.left() + 24.0, rect.center().y), Align2::CENTER_CENTER, icon, FontId::proportional(18.0), if e.exists { pal.accent } else { pal.faint });
    // right side: when it changed, or the button that removes the entry
    let right = rect.right() - 12.0;
    let mut act = None;
    let text_right;
    if hovered {
        let x = Rect::from_center_size(pos2(right - 12.0, rect.center().y), vec2(28.0, 28.0));
        let xr = ui.interact(x, resp.id.with("forget"), Sense::click()).on_hover_text("Remove from this list");
        if xr.hovered() {
            ui.painter().rect_filled(x, CornerRadius::same(6), pal.active());
        }
        ui.painter().text(x.center(), Align2::CENTER_CENTER, icons::X, FontId::proportional(14.0), pal.muted);
        if xr.clicked() {
            act = Some(Act::Forget(e.path.clone(), e.folder));
        }
        text_right = x.left() - 8.0;
    } else {
        let when = match (e.exists, e.modified) {
            (false, _) => "not found".to_string(),
            (true, Some(m)) => ago(SystemTime::now().duration_since(m).map(|d| d.as_secs()).unwrap_or(0)),
            (true, None) => String::new(),
        };
        let g = ui.painter().layout_no_wrap(when, FontId::proportional(11.5), if e.exists { pal.faint } else { pal.danger });
        text_right = right - g.size().x - 10.0;
        ui.painter().galley(pos2(right - g.size().x, rect.center().y - g.size().y / 2.0), g, pal.faint);
    }
    let left = rect.left() + 48.0;
    let room = (text_right - left).max(40.0);
    let name_font = FontId::proportional(14.0);
    let name = fit_middle(ui, &e.name, &name_font, room);
    ui.painter().text(pos2(left, rect.top() + 15.0), Align2::LEFT_CENTER, name, name_font, dim);
    let detail = if e.folder || !e.exists || e.size == 0 { e.place.clone() } else { format!("{}  ·  {}", e.place, size_label(e.size)) };
    let detail_font = FontId::proportional(11.5);
    let detail = fit_middle(ui, &detail, &detail_font, room);
    ui.painter().text(pos2(left, rect.top() + 33.0), Align2::LEFT_CENTER, detail, detail_font, pal.faint);
    let resp = resp.on_hover_text(&e.path);
    if act.is_none() && resp.clicked() {
        act = Some(Act::Open(e.path.clone()));
    }
    resp.context_menu(|ui| {
        if ui.button("Open").clicked() {
            act = Some(Act::Open(e.path.clone()));
            ui.close();
        }
        if ui.button("Remove from this list").clicked() {
            act = Some(Act::Forget(e.path.clone(), e.folder));
            ui.close();
        }
    });
    act
}

/// A large button with an icon, a title and what it does.
fn tile(ui: &mut Ui, pal: &Palette, icon: &str, title: &str, hint: &str, size: egui::Vec2) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        pal.active()
    } else if resp.hovered() {
        pal.hover()
    } else {
        pal.elev
    };
    ui.painter().rect(rect, CornerRadius::same(10), fill, Stroke::new(1.0, if resp.hovered() { pal.accent } else { pal.border_soft }), egui::StrokeKind::Inside);
    ui.painter().text(pos2(rect.left() + 14.0, rect.top() + 22.0), Align2::LEFT_CENTER, icon, FontId::proportional(19.0), pal.accent);
    let font = FontId::proportional(13.5);
    let title = fit_middle(ui, title, &font, size.x - 24.0);
    ui.painter().text(pos2(rect.left() + 14.0, rect.top() + 48.0), Align2::LEFT_CENTER, title, font, pal.text);
    if !hint.is_empty() {
        ui.painter().text(pos2(rect.right() - 12.0, rect.top() + 22.0), Align2::RIGHT_CENTER, hint, FontId::proportional(10.5), pal.faint);
    }
    resp.clicked()
}

pub fn show(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    let all = entries(ui, app);
    let filter_id = Id::new("home-filter");
    let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
    let avail = ui.available_width();
    let narrow = avail < 560.0;
    let width = (avail - if narrow { 24.0 } else { 64.0 }).min(680.0).max(240.0);
    let mut acts: Vec<Act> = Vec::new();
    egui::ScrollArea::vertical().id_salt("home").auto_shrink([false, false]).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.set_width(width);
            ui.add_space(if all.is_empty() { (ui.available_height() * 0.16).max(24.0) } else if narrow { 18.0 } else { 36.0 });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(icons::PEN_TOOL).size(30.0).color(pal.accent));
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(egui::RichText::new("Inkdown").size(22.0).strong());
                    ui.label(egui::RichText::new("Write, read and edit anything").size(12.5).color(pal.muted));
                });
            });
            ui.add_space(18.0);
            // actions
            let tiles: [(&str, &str, &str); 5] = [(icons::FILE_PLUS, "New document", "new_file"), (icons::FOLDER_OPEN, "Open file", "open_file"), (icons::FOLDER, "Open folder", "open_folder"), (icons::LAYOUT_TEMPLATE, "From template", "new_from_template"), (icons::LINK, "Web address", "open_url")];
            let per_row = if narrow { 2 } else { 5 };
            let gap = 8.0;
            let tile_w = ((width - gap * (per_row as f32 - 1.0)) / per_row as f32).floor();
            for chunk in tiles.chunks(per_row) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (icon, title, cmd) in chunk {
                        let key = if narrow { String::new() } else { app.keymap.label(cmd) };
                        if tile(ui, &pal, icon, title, &key, vec2(tile_w, 66.0)) {
                            app.pending.push(cmd.to_string());
                        }
                    }
                });
                ui.add_space(gap - 4.0);
            }
            if all.is_empty() {
                ui.add_space(22.0);
                ui.label(egui::RichText::new("Files and folders you open will be listed here.").size(12.5).color(pal.faint));
                return;
            }
            // recent
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Recent").size(15.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("Clear").size(11.5).color(pal.muted)).frame(false)).on_hover_text("Empty this list. No file is deleted.").clicked() {
                        app.pending.push("clear_recent".into());
                    }
                    if all.len() > 6 {
                        ui.add(egui::TextEdit::singleline(&mut filter).hint_text(format!("{}  Filter", icons::SEARCH)).desired_width(if narrow { 110.0 } else { 180.0 }));
                    }
                });
            });
            ui.add_space(6.0);
            let needle = filter.trim().to_lowercase();
            let wanted = |e: &&Entry| needle.is_empty() || e.name.to_lowercase().contains(&needle) || e.place.to_lowercase().contains(&needle);
            let files: Vec<&Entry> = all.iter().filter(|e| !e.folder).filter(wanted).collect();
            let folders: Vec<&Entry> = all.iter().filter(|e| e.folder).filter(wanted).collect();
            let limit = if needle.is_empty() { 12 } else { 30 };
            for e in files.iter().take(limit) {
                acts.extend(row(ui, &pal, e, width));
            }
            if files.len() > limit {
                ui.add_space(2.0);
                if ui.add(egui::Button::new(egui::RichText::new(format!("All {} recent files…", files.len())).size(12.0).color(pal.accent)).frame(false)).clicked() {
                    app.pending.push("open_recent".into());
                }
            }
            if !folders.is_empty() {
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("Folders").size(15.0).strong());
                });
                ui.add_space(6.0);
                for e in folders.iter().take(6) {
                    acts.extend(row(ui, &pal, e, width));
                }
            }
            if files.is_empty() && folders.is_empty() {
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Nothing recent matches.").size(12.5).color(pal.faint));
            }
            ui.add_space(24.0);
        });
    });
    ui.data_mut(|d| d.insert_temp(filter_id, filter));
    for act in acts {
        match act {
            Act::Open(path) => {
                if Path::new(&path).exists() {
                    app.pending.push(format!("open {path}"));
                } else {
                    app.toast("That file is no longer there. It was removed from the list.");
                    app.set.recent_files.retain(|p| p != &path);
                    app.set.recent_folders.retain(|p| p != &path);
                }
            }
            Act::Forget(path, folder) => {
                if folder {
                    app.set.recent_folders.retain(|p| p != &path);
                } else {
                    app.set.recent_files.retain(|p| p != &path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_sizes_read_naturally() {
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(59 * 60), "59 minutes ago");
        assert_eq!(ago(3600), "1 hour ago");
        assert_eq!(ago(30 * 3600), "yesterday");
        assert_eq!(ago(3 * 86_400), "3 days ago");
        assert_eq!(ago(21 * 86_400), "3 weeks ago");
        assert_eq!(ago(90 * 86_400), "3 months ago");
        assert_eq!(ago(800 * 86_400), "2 years ago");
        assert_eq!(shorten_home("C:/Users/me/Documents/a", r"C:\Users\me"), "~/Documents/a");
        assert_eq!(shorten_home("/home/me/notes", "/home/me/"), "~/notes");
        assert_eq!(shorten_home("/home/me", "/home/me"), "~");
        assert_eq!(shorten_home("/home/melissa/x", "/home/me"), "/home/melissa/x");
        assert_eq!(shorten_home("D:/work", ""), "D:/work");
        assert_eq!(size_label(512), "512 B");
        assert_eq!(size_label(2048), "2 KB");
        assert_eq!(size_label(5 * 1_048_576 + 524_288), "5.5 MB");
        assert_eq!(size_label(3 * 1_073_741_824), "3.00 GB");
    }
}
