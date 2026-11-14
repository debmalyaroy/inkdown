//! Built-in snippets and templates, plus the starter content of the user config files.

use serde_json::Value;

pub const KEYMAP_TEMPLATE: &str = r#"// Your key bindings. They override the defaults (same format as Sublime Text).
// Save this file to apply. Example:
//   { "keys": ["ctrl+shift+d"], "command": "duplicate_line" },
//   { "keys": ["ctrl+k", "ctrl+u"], "command": "upper_case" }
[
]
"#;

pub const SNIPPETS_TEMPLATE: &str = r#"// Your snippets, by file extension ("*" applies everywhere).
// Type the trigger and press Tab. $1, $2 are tab stops, ${1:text} has a default, $0 is the end.
{
  "*": {
    "todo": "TODO: $0"
  },
  "md": {
    "note": "> [!NOTE]\n> $0"
  }
}
"#;

const BUILTIN: &[(&str, &[(&str, &str)])] = &[
    ("*", &[("lorem", "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.$0")]),
    (
        "md",
        &[
            ("link", "[${1:text}](${2:url})$0"),
            ("img", "![${1:alt}](${2:path})$0"),
            ("code", "```${1:lang}\n$0\n```"),
            ("table", "| ${1:Column} | ${2:Column} |\n| --- | --- |\n| $3 | $4 |\n$0"),
            ("task", "- [ ] $0"),
            ("note", "> [!NOTE]\n> $0"),
            ("tip", "> [!TIP]\n> $0"),
            ("warn", "> [!WARNING]\n> $0"),
            ("mermaid", "```mermaid\nflowchart TD\n    ${1:A}[${2:Start}] --> ${3:B}[${4:End}]\n```\n$0"),
            ("math", "$$\n$0\n$$"),
            ("details", "<details><summary>${1:Summary}</summary>\n\n$0\n\n</details>"),
            ("front", "---\ntitle: ${1:Title}\ndate: ${2:2026-01-01}\n---\n\n$0"),
        ],
    ),
    (
        "rs",
        &[
            ("fn", "fn ${1:name}($2) {\n\t$0\n}"),
            ("pfn", "pub fn ${1:name}($2) -> ${3:()} {\n\t$0\n}"),
            ("impl", "impl ${1:Type} {\n\t$0\n}"),
            ("struct", "struct ${1:Name} {\n\t$0\n}"),
            ("enum", "enum ${1:Name} {\n\t$0\n}"),
            ("match", "match ${1:value} {\n\t${2:_} => $0,\n}"),
            ("test", "#[test]\nfn ${1:name}() {\n\t$0\n}"),
            ("derive", "#[derive(${1:Debug, Clone})]$0"),
            ("println", "println!(\"${1:{}}\", $2);$0"),
            ("for", "for ${1:item} in ${2:iter} {\n\t$0\n}"),
            ("iflet", "if let ${1:Some(x)} = ${2:value} {\n\t$0\n}"),
        ],
    ),
    (
        "js",
        &[
            ("fn", "function ${1:name}($2) {\n\t$0\n}"),
            ("afn", "const ${1:name} = ($2) => {\n\t$0\n};"),
            ("log", "console.log($1);$0"),
            ("for", "for (const ${1:item} of ${2:items}) {\n\t$0\n}"),
            ("if", "if (${1:condition}) {\n\t$0\n}"),
            ("class", "class ${1:Name} {\n\tconstructor($2) {\n\t\t$0\n\t}\n}"),
            ("imp", "import { $2 } from '${1:module}';$0"),
            ("try", "try {\n\t$1\n} catch (${2:err}) {\n\t$0\n}"),
        ],
    ),
    (
        "py",
        &[
            ("def", "def ${1:name}($2):\n\t$0"),
            ("class", "class ${1:Name}:\n\tdef __init__(self$2):\n\t\t$0"),
            ("main", "if __name__ == \"__main__\":\n\t$0"),
            ("for", "for ${1:item} in ${2:items}:\n\t$0"),
            ("with", "with open(${1:path}) as ${2:f}:\n\t$0"),
            ("try", "try:\n\t$1\nexcept ${2:Exception} as ${3:e}:\n\t$0"),
        ],
    ),
    (
        "html",
        &[
            ("html5", "<!doctype html>\n<html lang=\"en\">\n<head>\n\t<meta charset=\"utf-8\">\n\t<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\t<title>${1:Title}</title>\n</head>\n<body>\n\t$0\n</body>\n</html>"),
            ("div", "<div class=\"$1\">\n\t$0\n</div>"),
            ("a", "<a href=\"${1:#}\">$2</a>$0"),
            ("script", "<script>\n\t$0\n</script>"),
            ("link", "<link rel=\"stylesheet\" href=\"${1:style.css}\">$0"),
        ],
    ),
    ("go", &[("fn", "func ${1:name}($2) {\n\t$0\n}"), ("iferr", "if err != nil {\n\treturn ${1:err}\n}$0"), ("main", "package main\n\nfunc main() {\n\t$0\n}")]),
    ("c", &[("main", "int main(int argc, char **argv) {\n\t$0\n\treturn 0;\n}"), ("for", "for (int ${1:i} = 0; $1 < ${2:n}; $1++) {\n\t$0\n}"), ("inc", "#include <${1:stdio.h}>$0")]),
    ("sh", &[("bash", "#!/usr/bin/env bash\nset -euo pipefail\n\n$0"), ("if", "if [[ ${1:condition} ]]; then\n\t$0\nfi"), ("for", "for ${1:item} in ${2:items}; do\n\t$0\ndone")]),
    ("java", &[("main", "public static void main(String[] args) {\n\t$0\n}"), ("sout", "System.out.println($1);$0"), ("class", "public class ${1:Name} {\n\t$0\n}")]),
    ("sql", &[("sel", "SELECT ${1:*}\nFROM ${2:table}\nWHERE ${3:condition};$0"), ("ins", "INSERT INTO ${1:table} (${2:columns})\nVALUES (${3:values});$0")]),
];

fn group(ext: &str, syntax: &str) -> &'static str {
    let s = syntax.to_lowercase();
    match ext {
        "md" | "markdown" | "mdx" => "md",
        "rs" => "rs",
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => "js",
        "py" | "pyw" => "py",
        "html" | "htm" | "vue" | "svelte" => "html",
        "go" => "go",
        "c" | "h" | "cpp" | "cc" | "hpp" => "c",
        "sh" | "bash" | "zsh" => "sh",
        "java" | "kt" => "java",
        "sql" => "sql",
        _ if s.contains("markdown") => "md",
        _ if s.contains("rust") => "rs",
        _ if s.contains("javascript") || s.contains("typescript") => "js",
        _ if s.contains("python") => "py",
        _ if s.contains("html") => "html",
        _ if s.contains("shell") || s.contains("bash") => "sh",
        _ => "",
    }
}

pub fn for_language(ext: &str, syntax: &str, user: &Value) -> Vec<(String, String)> {
    let g = group(ext, syntax);
    let mut out: Vec<(String, String)> = Vec::new();
    // user snippets first, so they win over built-ins with the same trigger
    for key in [ext, g, "*"] {
        if let Some(o) = user.get(key).and_then(Value::as_object) {
            for (k, v) in o {
                if let Some(body) = v.as_str() {
                    if !out.iter().any(|x| &x.0 == k) {
                        out.push((k.clone(), body.to_string()));
                    }
                }
            }
        }
    }
    for (name, list) in BUILTIN {
        if *name == g || *name == "*" {
            for (k, v) in *list {
                if !out.iter().any(|x| x.0 == *k) {
                    out.push((k.to_string(), v.to_string()));
                }
            }
        }
    }
    out
}

pub const TEMPLATES: &[(&str, &str, &str)] = &[
    ("Blank document", "Untitled.md", ""),
    ("Meeting notes", "Meeting notes.md", "# Meeting notes\n\n**Date:** \n**Attendees:** \n\n## Agenda\n\n1. \n\n## Notes\n\n- \n\n## Action items\n\n- [ ] \n"),
    ("README", "README.md", "# Project name\n\nOne paragraph describing what this project does and who it is for.\n\n## Install\n\n```sh\n\n```\n\n## Usage\n\n```sh\n\n```\n\n## License\n\n"),
    ("Blog post", "Post.md", "---\ntitle: \ndate: \ntags: []\n---\n\n# Title\n\nIntro paragraph.\n\n## Section\n\n"),
    ("Daily journal", "Journal.md", "# Journal\n\n## Today\n\n- [ ] \n\n## Notes\n\n\n## Gratitude\n\n1. \n"),
    ("Project plan", "Plan.md", "# Project plan\n\n## Goal\n\n\n## Milestones\n\n| Milestone | Owner | Due | Status |\n| --- | --- | --- | --- |\n|  |  |  |  |\n\n## Timeline\n\n```mermaid\ngantt\n    title Plan\n    dateFormat YYYY-MM-DD\n    section Phase 1\n    Research :a1, 2026-01-05, 7d\n    Build    :after a1, 14d\n```\n\n## Risks\n\n- \n"),
    ("Technical spec", "Spec.md", "# Title\n\n> [!NOTE]\n> Status: Draft\n\n## Summary\n\n\n## Background\n\n\n## Design\n\n```mermaid\nflowchart LR\n    Client --> API --> DB[(Database)]\n```\n\n## Alternatives considered\n\n\n## Open questions\n\n- [ ] \n"),
    ("Flowchart (Mermaid)", "diagram.mmd", "flowchart TD\n    A[Start] --> B{Decision}\n    B -->|Yes| C[Do it]\n    B -->|No| D[Skip]\n    C --> E[End]\n    D --> E\n"),
    ("Sequence diagram (Mermaid)", "sequence.mmd", "sequenceDiagram\n    participant U as User\n    participant A as App\n    participant S as Server\n    U->>A: Click\n    A->>S: Request\n    S-->>A: Response\n    A-->>U: Update\n"),
    ("Graph (Graphviz)", "graph.dot", "digraph G {\n    rankdir=LR;\n    a -> b;\n    b -> c;\n    a -> c;\n}\n"),
    ("JSON", "data.json", "{\n  \"name\": \"\",\n  \"items\": []\n}\n"),
    ("YAML", "config.yaml", "name: \nitems:\n  - \n"),
    ("TOML", "config.toml", "[package]\nname = \"\"\nversion = \"0.1.0\"\n"),
    ("CSV table", "table.csv", "Name,Quantity,Price\nApples,3,1.20\nPears,5,0.80\n"),
    ("HTML page", "index.html", "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <title>Page</title>\n</head>\n<body>\n  <h1>Hello</h1>\n</body>\n</html>\n"),
    ("Diagram (designer)", "diagram.svg", ""),
    ("Plain text", "untitled.txt", ""),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_snippets() {
        let user: Value = serde_json::from_str(r#"{"rs": {"fn": "custom"}, "*": {"sig": "me"}}"#).unwrap();
        let s = for_language("rs", "Rust", &user);
        assert_eq!(s.iter().find(|x| x.0 == "fn").unwrap().1, "custom");
        assert!(s.iter().any(|x| x.0 == "sig") && s.iter().any(|x| x.0 == "impl") && s.iter().any(|x| x.0 == "lorem"));
        assert!(for_language("zzz", "Plain Text", &Value::Null).iter().all(|x| x.0 == "lorem"));
    }
}
