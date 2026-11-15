//! Notes: turning quick drafts into Markdown files in one organised place.
//!
//! Untitled tabs are where notes start. *File in Notes* saves such a draft as a Markdown
//! file in the Notes folder, named after its first line, inside a notebook (a subfolder).
//! Files that are already saved can be moved between notebooks the same way.

use std::path::{Path, PathBuf};

use crate::app::InkApp;

/// The notebook that receives notes filed without choosing one.
pub const INBOX: &str = "Inbox";

/// Where notes live: the folder from the settings, or `Documents/Inkdown Notes`.
pub fn folder(app: &InkApp) -> PathBuf {
    if !app.set.notes_folder.trim().is_empty() {
        return PathBuf::from(app.set.notes_folder.trim());
    }
    default_folder()
}

fn default_folder() -> PathBuf {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        crate::mobile::documents_dir().join("Notes")
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(crate::settings::config_dir);
        let documents = home.join("Documents");
        (if documents.is_dir() { documents } else { home }).join("Inkdown Notes")
    }
}

/// Notebooks (subfolders of the Notes folder), Inbox first.
pub fn notebooks(app: &InkApp) -> Vec<String> {
    let mut out = vec![INBOX.to_string()];
    if let Ok(entries) = std::fs::read_dir(folder(app)) {
        let mut names: Vec<String> = entries.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| !n.starts_with('.') && n != INBOX).collect();
        names.sort_by_key(|n| n.to_lowercase());
        out.extend(names);
    }
    out
}

/// A file name for a note from its text: the first line that says something, without
/// Markdown markers and without characters a file name cannot hold.
pub fn title_of(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty() && *l != "---" && !l.chars().all(|c| "#-=*_`> ".contains(c))).unwrap_or("");
    let line = line.trim_start_matches(['#', '>', '-', '*', '+', ' ']).trim_start_matches("[ ]").trim_start_matches("[x]").trim();
    let mut name = String::new();
    for c in line.chars() {
        match c {
            '*' | '_' | '`' | '~' | '[' | ']' => {}
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' => name.push(' '),
            c if c.is_control() => {}
            c => name.push(c),
        }
        if name.chars().count() >= 60 {
            break;
        }
    }
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    name.trim_end_matches(['.', ' ']).to_string()
}

/// `dir/name.md`, or `dir/name 2.md` and so on when that exists.
fn free_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(format!("{name}.md"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{name} {n}.md"))).find(|p| !p.exists()).unwrap()
}

fn untitled_name() -> String {
    let (y, mo, d, h, mi) = crate::tools::now_local();
    format!("Note {y:04}-{mo:02}-{d:02} {h:02}.{mi:02}")
}

/// Files the document at `ix` in `notebook`. An untitled draft is saved there as Markdown;
/// a saved file is moved there. Returns the new path.
pub fn file_doc(app: &mut InkApp, ix: usize, notebook: &str) -> Result<PathBuf, String> {
    let dir = folder(app).join(notebook);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let set = app.set.clone();
    let Some(d) = app.docs.get_mut(ix) else { return Err("No document to file.".into()) };
    if d.read_only {
        return Err("This document is read-only.".into());
    }
    let old = d.path.clone();
    let name = match &old {
        // keep the name of a file that already has one, unless it never got a real one
        Some(p) => {
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let from_text = title_of(&d.buf.text());
            if stem.to_lowercase().starts_with("untitled") && !from_text.is_empty() { from_text } else { stem }
        }
        None => title_of(&d.buf.text()),
    };
    let name = if name.is_empty() { untitled_name() } else { name };
    if let Some(p) = &old {
        if p.parent() == Some(dir.as_path()) {
            return Err(format!("Already in {notebook}."));
        }
    }
    let target = free_path(&dir, &name);
    d.set_path(target.clone(), &set);
    d.scratch = false;
    if let Err(e) = d.save(&set) {
        // put things back as they were
        match old {
            Some(p) => d.set_path(p, &set),
            None => {
                d.path = None;
            }
        }
        return Err(e);
    }
    // the note now lives in the notebook: remove where it used to be
    if let Some(p) = old {
        let _ = std::fs::remove_file(&p);
    }
    app.set.add_recent(&target);
    Ok(target)
}

pub fn command(app: &mut InkApp, name: &str, arg: &str) {
    match name {
        "notes_file" => app.picker = Some(crate::picker::Picker::new(crate::picker::PickKind::Notebooks, app)),
        "notes_file_to" => {
            let notebook = if arg.is_empty() { INBOX } else { arg };
            match file_doc(app, app.active, notebook) {
                Ok(p) => app.toast(format!("Filed in {notebook} as {}", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())),
                Err(e) => app.toast(e),
            }
            app.sidebar.refresh();
        }
        "notes_file_all" => {
            let drafts: Vec<u64> = app.docs.iter().filter(|d| d.path.is_none() && !d.scratch && !d.read_only && d.buf.len() > 0).map(|d| d.id).collect();
            if drafts.is_empty() {
                app.toast("There are no unsaved drafts to file");
                return;
            }
            let mut done = 0;
            let mut failed = None;
            for id in drafts {
                let Some(ix) = app.docs.iter().position(|d| d.id == id) else { continue };
                match file_doc(app, ix, INBOX) {
                    Ok(_) => done += 1,
                    Err(e) => failed = Some(e),
                }
            }
            match failed {
                Some(e) => app.toast(format!("Filed {done}; one could not be filed: {e}")),
                None => app.toast(format!("Filed {done} {} in {INBOX}", if done == 1 { "draft" } else { "drafts" })),
            }
            app.sidebar.refresh();
        }
        "notes_open" => {
            let dir = folder(app);
            if let Err(e) = std::fs::create_dir_all(dir.join(INBOX)) {
                app.toast(format!("Cannot create {}: {e}", dir.display()));
                return;
            }
            app.sidebar.set_folder(Some(dir));
            app.set.show_sidebar = true;
        }
        "notes_new_notebook" => {
            let dir = folder(app);
            let _ = std::fs::create_dir_all(&dir);
            app.dialog = Some(crate::app::Dialog::NewFile(dir, String::new(), true));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_free_names() {
        assert_eq!(title_of("# Trip plan\n\nPack light"), "Trip plan");
        assert_eq!(title_of("\n\n---\n\n## **Meeting** with `ops` / infra?\nagenda"), "Meeting with ops infra");
        assert_eq!(title_of("- [ ] buy milk\n- [ ] eggs"), "buy milk");
        assert_eq!(title_of("plain first line.\nsecond"), "plain first line");
        assert_eq!(title_of("   \n###\n"), "");
        assert!(title_of(&"word ".repeat(40)).chars().count() <= 60);
        let dir = std::env::temp_dir().join(format!("ink-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "A"), dir.join("A.md"));
        std::fs::write(dir.join("A.md"), "x").unwrap();
        assert_eq!(free_path(&dir, "A"), dir.join("A 2.md"));
        std::fs::write(dir.join("A 2.md"), "x").unwrap();
        assert_eq!(free_path(&dir, "A"), dir.join("A 3.md"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
