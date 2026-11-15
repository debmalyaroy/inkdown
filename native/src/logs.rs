//! Colouring for log files.
//!
//! Every line is coloured on its own from its severity, so this costs the same on a file
//! of a few lines as on one of several million: only the lines on screen are looked at.

use crate::theme::{TOK_ATTR, TOK_NUMBER, TOK_STRING, TOK_TEXT, TOK_TYPE};

pub const TOK_LOG_ERROR: u8 = 17;
pub const TOK_LOG_WARN: u8 = 18;
pub const TOK_LOG_INFO: u8 = 19;
pub const TOK_LOG_DEBUG: u8 = 20;
pub const TOK_LOG_TRACE: u8 = 21;
pub const TOK_LOG_TIME: u8 = 22;
/// Added to a severity token for the severity word itself, which is drawn bold.
pub const LEVEL_WORD: u8 = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    pub fn token(self) -> u8 {
        match self {
            Level::Error => TOK_LOG_ERROR,
            Level::Warn => TOK_LOG_WARN,
            Level::Info => TOK_LOG_INFO,
            Level::Debug => TOK_LOG_DEBUG,
            Level::Trace => TOK_LOG_TRACE,
        }
    }
}

fn level_of_word(word: &str) -> Option<Level> {
    Some(match word.to_ascii_uppercase().as_str() {
        "FATAL" | "CRITICAL" | "CRIT" | "EMERG" | "EMERGENCY" | "ALERT" | "PANIC" | "SEVERE" | "ERROR" | "ERR" | "FAILURE" => Level::Error,
        "WARN" | "WARNING" | "WRN" => Level::Warn,
        "INFO" | "INF" | "INFORMATION" | "NOTICE" | "CONFIG" => Level::Info,
        "DEBUG" | "DBG" | "FINE" => Level::Debug,
        "TRACE" | "TRC" | "VERBOSE" | "FINER" | "FINEST" => Level::Trace,
        _ => return None,
    })
}

/// The severity named near the start of a line: `(start, end, level)` in bytes.
pub fn find_level(line: &str) -> Option<(usize, usize, Level)> {
    let b = line.as_bytes();
    let limit = b.len().min(160);
    let mut i = 0;
    let mut words = 0;
    while i < limit {
        if !b[i].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        let word = &line[start..i];
        let before = if start > 0 { b[start - 1] } else { b' ' };
        let after = b.get(i).copied().unwrap_or(b' ');
        // part of a longer name such as `error_count` or `com.x.ErrorHandler`
        if before.is_ascii_alphanumeric() || before == b'_' || before == b'.' || after.is_ascii_digit() || after == b'_' {
            continue;
        }
        if word.len() == 1 {
            // Android logcat: `E/Tag(123): …` or `… 1234 5678 E Tag: …`
            let letter = match b[start] {
                b'F' | b'E' => Some(Level::Error),
                b'W' => Some(Level::Warn),
                b'I' => Some(Level::Info),
                b'D' => Some(Level::Debug),
                b'V' => Some(Level::Trace),
                _ => None,
            };
            let logcat = after == b'/' && (start == 0 || before == b' ') || (before == b' ' && after == b' ' && start >= 2 && b[..start].iter().rev().find(|c| **c != b' ').is_some_and(|c| c.is_ascii_digit()) && b[0].is_ascii_digit());
            if let (Some(l), true) = (letter, logcat) {
                return Some((start, i, l));
            }
            continue;
        }
        if let Some(l) = level_of_word(word) {
            let upper = word.bytes().all(|c| c.is_ascii_uppercase());
            // lower case counts only where a level is clearly meant: `[error]`, `level=warn`, `info:`
            let marked = matches!(before, b'[' | b'<' | b'=' | b'"' | b'|' | b'(') || matches!(after, b']' | b'>' | b':' | b'|' | b')') || (start == 0 && after == b' ');
            if upper || marked {
                return Some((start, i, l));
            }
        }
        words += 1;
        if words > 12 {
            break;
        }
    }
    None
}

/// Length of a date and time at the start of the line, or 0.
fn timestamp_len(line: &str) -> usize {
    let b = line.as_bytes();
    let mut i = 0;
    let (mut digits, mut colons) = (0, 0);
    let open = b.first() == Some(&b'[');
    if open {
        i = 1;
    }
    while i < b.len() && i < 48 {
        match b[i] {
            b'0'..=b'9' => digits += 1,
            b':' => colons += 1,
            b'-' | b'.' | b',' | b'/' | b'+' | b'T' | b'Z' => {}
            b' ' if b.get(i + 1).is_some_and(|c| c.is_ascii_digit()) && colons == 0 => {}
            // month names: `Nov 15 10:00:00`
            c if c.is_ascii_alphabetic() && digits == 0 && i < 4 + open as usize => {}
            b' ' if digits == 0 => {}
            _ => break,
        }
        i += 1;
    }
    if open && b.get(i) == Some(&b']') {
        i += 1;
    }
    if digits >= 4 && colons >= 1 {
        i
    } else {
        0
    }
}

/// True for lines that belong to the entry before them: stack frames, wrapped text.
pub fn continues(line: &str) -> bool {
    if line.starts_with([' ', '\t']) && !line.trim().is_empty() || line.starts_with("Caused by") || line.starts_with("Traceback") || line.starts_with("...") {
        return true;
    }
    // the first line of a stack trace: `java.lang.IllegalStateException: boom`, `ValueError: bad`
    let head = line.split([':', ' ']).next().unwrap_or("");
    let named = head.ends_with("Exception") || head.ends_with("Error") || head.ends_with("Throwable") || head.ends_with("Panic");
    named && head.len() > 5 && head.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '$')) && head.chars().next().is_some_and(|c| c.is_alphabetic()) && timestamp_len(line) == 0
}

/// Numbers, quoted text, `key=` names and `[groups]` in the part of a line after its level.
fn detail(line: &str, from: usize, base: u8, out: &mut Vec<(u32, u8)>) {
    let b = line.as_bytes();
    let mut i = from;
    let mut run = from;
    let flush = |upto: usize, run: &mut usize, out: &mut Vec<(u32, u8)>| {
        if upto > *run {
            out.push((upto as u32, base));
        }
        *run = upto;
    };
    while i < b.len() {
        let c = b[i];
        let prev_word = i > from && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if c == b'"' || c == b'\'' && !prev_word {
            if let Some(len) = b[i + 1..].iter().position(|x| *x == c).filter(|l| *l < 400) {
                flush(i, &mut run, out);
                i += len + 2;
                out.push((i as u32, TOK_STRING));
                run = i;
                continue;
            }
        } else if c.is_ascii_digit() && !prev_word {
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'.' && b.get(j + 1).is_some_and(|x| x.is_ascii_digit())) {
                j += 1;
            }
            flush(i, &mut run, out);
            out.push((j as u32, TOK_NUMBER));
            i = j;
            run = j;
            continue;
        } else if c == b'=' && prev_word {
            let mut s = i;
            while s > run && (b[s - 1].is_ascii_alphanumeric() || matches!(b[s - 1], b'_' | b'.' | b'-')) {
                s -= 1;
            }
            flush(s, &mut run, out);
            out.push((i as u32, TOK_ATTR));
            run = i;
        } else if c == b'[' {
            if let Some(len) = b[i + 1..].iter().position(|x| *x == b']').filter(|l| *l < 80) {
                flush(i, &mut run, out);
                i += len + 2;
                out.push((i as u32, TOK_TYPE));
                run = i;
                continue;
            }
        }
        i += 1;
    }
    flush(b.len(), &mut run, out);
}

/// Colour spans `(end byte, token)` for one line. `inherited` is the level of the entry
/// a continuation line belongs to.
pub fn spans(line: &str, inherited: Option<Level>) -> Vec<(u32, u8)> {
    let mut out = Vec::with_capacity(12);
    let Some((start, end, level)) = find_level(line) else {
        match inherited {
            Some(l) if l != Level::Info => out.push((line.len() as u32, l.token())),
            _ => {
                let t = timestamp_len(line);
                if t > 0 {
                    out.push((t as u32, TOK_LOG_TIME));
                }
                detail(line, t, TOK_TEXT, &mut out);
            }
        }
        if out.is_empty() {
            out.push((line.len() as u32, TOK_TEXT));
        }
        return out;
    };
    let t = timestamp_len(line).min(start);
    if t > 0 {
        out.push((t as u32, TOK_LOG_TIME));
    }
    if start > t {
        detail(&line[..start], t, TOK_TEXT, &mut out);
    }
    out.push((end as u32, level.token() + LEVEL_WORD));
    match level {
        // the whole entry takes the colour of its severity
        Level::Error | Level::Warn | Level::Debug | Level::Trace => out.push((line.len() as u32, level.token())),
        Level::Info => detail(line, end, TOK_TEXT, &mut out),
    }
    out.retain(|s| s.0 > 0);
    out
}

/// True when text that is not named `.log` reads like a log: most of its first lines
/// start with a time and name a level.
pub fn looks_like_log(head: &str) -> bool {
    let lines: Vec<&str> = head.lines().filter(|l| !l.trim().is_empty()).take(12).collect();
    if lines.len() < 2 {
        // a single line: it must have both a time and a level
        return lines.first().is_some_and(|l| timestamp_len(l) > 0 && find_level(l).is_some());
    }
    let hits = lines.iter().filter(|l| timestamp_len(l) > 0 && find_level(l).is_some()).count();
    hits * 2 > lines.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(line: &str) -> Option<Level> {
        find_level(line).map(|l| l.2)
    }

    #[test]
    fn levels_are_found_in_common_formats() {
        assert_eq!(level("2026-11-15T10:00:00.000Z ERROR [worker-1] com.x.Handler - failed"), Some(Level::Error));
        assert_eq!(level("2026-11-15 10:00:00,123 WARN  pool exhausted"), Some(Level::Warn));
        assert_eq!(level("[2026-11-15 10:00:00] [info] server started"), Some(Level::Info));
        assert_eq!(level("Nov 15 10:00:00 host app[12]: <debug> tick"), Some(Level::Debug));
        assert_eq!(level("time=2026-11-15T10:00:00Z level=warn msg=\"slow\""), Some(Level::Warn));
        assert_eq!(level("{\"ts\":\"2026-11-15\",\"level\":\"error\",\"msg\":\"x\"}"), Some(Level::Error));
        assert_eq!(level("11-15 10:00:00.000  1234  5678 E ActivityManager: ANR in app"), Some(Level::Error));
        assert_eq!(level("W/System.err( 123): at java.lang.Thread.run"), Some(Level::Warn));
        assert_eq!(level("TRACE entering loop"), Some(Level::Trace));
        // words that merely contain or mention a level
        assert_eq!(level("2026-11-15 10:00:00 request to /error_report took 5 ms"), None);
        assert_eq!(level("com.example.ErrorHandler started"), None);
        assert_eq!(level("there was an error in the data"), None);
        assert_eq!(level("    at com.example.Main.run(Main.java:10)"), None);
    }

    #[test]
    fn lines_are_coloured_by_severity() {
        let line = "2026-11-15T10:00:00.000Z ERROR [worker-1] failed id=7";
        let s = spans(line, None);
        assert_eq!(s[0], (24, TOK_LOG_TIME));
        assert!(s.contains(&(30, TOK_LOG_ERROR + LEVEL_WORD)));
        assert_eq!(*s.last().unwrap(), (line.len() as u32, TOK_LOG_ERROR));
        let line = "2026-11-15T10:00:00.000Z INFO  [worker-1] done status=200 path=\"/a b\"";
        let s = spans(line, None);
        assert!(s.contains(&(29, TOK_LOG_INFO + LEVEL_WORD)));
        assert!(s.iter().any(|x| x.1 == TOK_NUMBER) && s.iter().any(|x| x.1 == TOK_STRING) && s.iter().any(|x| x.1 == TOK_ATTR) && s.iter().any(|x| x.1 == TOK_TYPE));
        assert_eq!(s.last().unwrap().0 as usize, line.len());
        // spans never run backwards and always cover the line
        for l in [line, "", "   ", "plain text", "\tat x.y(Z.java:1)", "ünïcödé ERROR ñ"] {
            let s = spans(l, Some(Level::Error));
            assert!(s.windows(2).all(|w| w[0].0 < w[1].0), "{l}: {s:?}");
            assert_eq!(s.last().map(|x| x.0 as usize).unwrap_or(0), l.len(), "{l}");
            assert!(s.iter().all(|x| l.is_char_boundary(x.0 as usize)));
        }
        // a stack frame takes the colour of the entry it belongs to
        assert_eq!(spans("\tat x.y(Z.java:1)", Some(Level::Error)), vec![(17, TOK_LOG_ERROR)]);
        assert!(continues("\tat x.y(Z.java:1)") && continues("Caused by: java.io.IOException") && !continues("2026 INFO x"));
        assert!(continues("java.lang.IllegalStateException: boom") && continues("ValueError: bad value") && continues("thread.MyError"));
        assert!(!continues("Error") && !continues("ERROR something failed") && !continues("An Error: no") && !continues("12:00:01 java.lang.Error: x"));
    }

    #[test]
    fn logs_are_recognised_by_content() {
        assert!(looks_like_log("2026-11-15 10:00:00 INFO a\n2026-11-15 10:00:01 WARN b\n2026-11-15 10:00:02 INFO c\n"));
        assert!(!looks_like_log("# Notes\n\nSome INFO about things\nand more text\n"));
        assert!(!looks_like_log("one line only"));
    }
}
