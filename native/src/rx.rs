//! Search patterns for Find, Replace and Find in Files.
//!
//! Most patterns run on the fast linear-time engine. Patterns that need look-around or
//! back-references (`(?<=a)b`, `(\w+) \1`) fall back to a backtracking engine.

/// A compiled search pattern.
pub enum Pattern {
    Fast(regex::Regex),
    Fancy(fancy_regex::Regex),
}

fn tidy(e: impl std::fmt::Display) -> String {
    e.to_string().lines().last().unwrap_or("Invalid pattern").trim().to_string()
}

impl Pattern {
    /// Compiles `pattern`. `multi_line` makes `^` and `$` match at line ends.
    pub fn new(pattern: &str, case_sensitive: bool, multi_line: bool) -> Result<Self, String> {
        match regex::RegexBuilder::new(pattern).case_insensitive(!case_sensitive).multi_line(multi_line).build() {
            Ok(r) => Ok(Pattern::Fast(r)),
            Err(first) => {
                // only what the fast engine cannot express is worth a second try
                let needs_fancy = pattern.contains("(?=") || pattern.contains("(?!") || pattern.contains("(?<=") || pattern.contains("(?<!") || has_back_reference(pattern) || pattern.contains("(?>");
                if !needs_fancy {
                    return Err(tidy(first));
                }
                fancy_regex::RegexBuilder::new(pattern).case_insensitive(!case_sensitive).multi_line(multi_line).build().map(Pattern::Fancy).map_err(tidy)
            }
        }
    }

    /// Calls `each` with the byte range of every match, in order, until it returns false.
    /// A match that cannot be decided (the backtracking engine gave up) ends the search.
    pub fn scan(&self, text: &str, mut each: impl FnMut(usize, usize) -> bool) {
        match self {
            Pattern::Fast(r) => {
                for m in r.find_iter(text) {
                    if !each(m.start(), m.end()) {
                        break;
                    }
                }
            }
            Pattern::Fancy(r) => {
                for m in r.find_iter(text) {
                    let Ok(m) = m else { break };
                    if !each(m.start(), m.end()) {
                        break;
                    }
                }
            }
        }
    }

    /// Byte ranges of every match, in order.
    pub fn matches(&self, text: &str) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        self.scan(text, |a, z| {
            out.push((a, z));
            true
        });
        out
    }

    pub fn is_match(&self, text: &str) -> bool {
        match self {
            Pattern::Fast(r) => r.is_match(text),
            Pattern::Fancy(r) => r.is_match(text).unwrap_or(false),
        }
    }

    /// The replacement for the first match in `text`, with `$1`, `${name}` filled in.
    pub fn expand(&self, text: &str, replacement: &str) -> Option<String> {
        let mut out = String::new();
        match self {
            Pattern::Fast(r) => r.captures(text)?.expand(replacement, &mut out),
            Pattern::Fancy(r) => r.captures(text).ok()??.expand(replacement, &mut out),
        }
        Some(out)
    }

    /// `text` with every match replaced (`$1` style references in `replacement`).
    pub fn replace_all(&self, text: &str, replacement: &str) -> String {
        match self {
            Pattern::Fast(r) => r.replace_all(text, replacement).into_owned(),
            Pattern::Fancy(r) => r.replace_all(text, replacement).into_owned(),
        }
    }
}

/// `\1` to `\9` outside a character class, or a named back-reference.
fn has_back_reference(p: &str) -> bool {
    let b = p.as_bytes();
    let mut i = 0;
    let mut in_class = false;
    while i < b.len() {
        match b[i] {
            b'\\' if i + 1 < b.len() => {
                if !in_class && (b[i + 1].is_ascii_digit() && b[i + 1] != b'0' || b[i + 1] == b'k') {
                    return true;
                }
                i += 2;
                continue;
            }
            b'[' => in_class = true,
            b']' => in_class = false,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Escapes `text` so that it matches itself.
pub fn escape(text: &str) -> String {
    regex::escape(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_and_fancy_patterns() {
        let p = Pattern::new("fo+", false, true).unwrap();
        assert!(matches!(p, Pattern::Fast(_)));
        assert_eq!(p.matches("Foo fo"), vec![(0, 3), (4, 6)]);

        // look-behind and look-ahead
        let p = Pattern::new(r"(?<=\$)\d+(?!\d*%)", true, true).unwrap();
        assert!(matches!(p, Pattern::Fancy(_)));
        assert_eq!(p.matches("cost $120, off 15%, was $90"), vec![(6, 9), (25, 27)]);

        // back-reference: a doubled word
        let p = Pattern::new(r"\b(\w+) \1\b", true, true).unwrap();
        assert_eq!(p.matches("this is is a test test"), vec![(5, 10), (13, 22)]);
        assert_eq!(p.replace_all("this is is a test test", "$1"), "this is a test");
        assert_eq!(p.expand("is is", "<$1>").as_deref(), Some("<is>"));

        // a digit inside a class is not a back-reference
        assert!(!has_back_reference(r"[\1]"));
        assert!(has_back_reference(r"(a)\1"));

        // a plain mistake is still an error, from the fast engine
        assert!(Pattern::new("(unclosed", true, true).is_err());
        assert!(Pattern::new(r"(?<=a", true, true).is_err());
        assert!(Pattern::new("^b$", true, true).unwrap().is_match("a\nb\nc"));
        assert!(!Pattern::new("^b$", true, false).unwrap().is_match("a\nb\nc"));
    }
}
