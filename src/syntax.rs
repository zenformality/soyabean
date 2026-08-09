//! Lightweight syntax highlighting: per-line tokenizer with a tiny carry-over
//! state for multi-line block comments. No external parser dependencies.

use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Normal,
    Keyword,
    Type,
    Str,
    Comment,
    Number,
    Func,
    Punct,
}

/// State at the *start* of a line (only multi-line block comments carry over).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct LineState {
    pub in_block: bool,
}

pub struct Language {
    pub name: &'static str,
    pub exts: &'static [&'static str],
    pub keywords: &'static [&'static str],
    pub types: &'static [&'static str],
    pub line_comment: &'static str, // "" = none
    pub block_comment: Option<(&'static str, &'static str)>,
    pub strings: &'static [char],
    pub highlight_caps: bool, // treat Capitalized idents as types
}

pub static PLAIN: Language = Language {
    name: "text",
    exts: &["txt", "md", "log"],
    keywords: &[],
    types: &[],
    line_comment: "",
    block_comment: None,
    strings: &[],
    highlight_caps: false,
};

static RUST: Language = Language {
    name: "rust",
    exts: &["rs"],
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while",
    ],
    types: &[
        "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128", "usize", "isize",
        "f32", "f64", "bool", "char", "str",
    ],
    line_comment: "//",
    block_comment: Some(("/*", "*/")),
    strings: &['"'],
    highlight_caps: true,
};

static C: Language = Language {
    name: "c",
    exts: &["c", "h"],
    keywords: &[
        "auto", "break", "case", "const", "continue", "default", "do", "else", "enum", "extern",
        "for", "goto", "if", "inline", "register", "return", "sizeof", "static", "struct",
        "switch", "typedef", "union", "volatile", "while", "restrict",
    ],
    types: &[
        "char",
        "double",
        "float",
        "int",
        "long",
        "short",
        "signed",
        "unsigned",
        "void",
        "bool",
        "size_t",
        "ssize_t",
        "int8_t",
        "int16_t",
        "int32_t",
        "int64_t",
        "uint8_t",
        "uint16_t",
        "uint32_t",
        "uint64_t",
        "uintptr_t",
        "intptr_t",
        "FILE",
    ],
    line_comment: "//",
    block_comment: Some(("/*", "*/")),
    strings: &['"', '\''],
    highlight_caps: false,
};

static CPP: Language = Language {
    name: "c++",
    exts: &["cpp", "cc", "cxx", "hpp", "hh", "hxx"],
    keywords: &[
        "auto",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "constexpr",
        "continue",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "explicit",
        "extern",
        "false",
        "final",
        "for",
        "friend",
        "goto",
        "if",
        "inline",
        "mutable",
        "namespace",
        "new",
        "noexcept",
        "nullptr",
        "operator",
        "override",
        "private",
        "protected",
        "public",
        "return",
        "sizeof",
        "static",
        "struct",
        "switch",
        "template",
        "this",
        "throw",
        "true",
        "try",
        "typedef",
        "typename",
        "union",
        "using",
        "virtual",
        "volatile",
        "while",
    ],
    types: &[
        "char", "double", "float", "int", "long", "short", "signed", "unsigned", "void", "bool",
        "size_t", "string", "vector", "map", "set", "auto", "wchar_t",
    ],
    line_comment: "//",
    block_comment: Some(("/*", "*/")),
    strings: &['"', '\''],
    highlight_caps: true,
};

static PYTHON: Language = Language {
    name: "python",
    exts: &["py", "pyw"],
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "case", "class", "continue", "def",
        "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in",
        "is", "lambda", "match", "nonlocal", "not", "or", "pass", "raise", "return", "self", "try",
        "while", "with", "yield", "True", "False", "None",
    ],
    types: &[
        "int", "float", "str", "bool", "list", "dict", "set", "tuple", "bytes", "object",
    ],
    line_comment: "#",
    block_comment: None,
    strings: &['"', '\''],
    highlight_caps: true,
};

static JS: Language = Language {
    name: "javascript",
    exts: &["js", "jsx", "mjs", "cjs"],
    keywords: &[
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "from",
        "function",
        "get",
        "if",
        "import",
        "in",
        "instanceof",
        "let",
        "new",
        "null",
        "of",
        "return",
        "set",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "undefined",
        "var",
        "void",
        "while",
        "yield",
    ],
    types: &[
        "Array", "Object", "String", "Number", "Boolean", "Promise", "Map", "Set", "Math", "JSON",
    ],
    line_comment: "//",
    block_comment: Some(("/*", "*/")),
    strings: &['"', '\'', '`'],
    highlight_caps: false,
};

static TS: Language = Language {
    name: "typescript",
    exts: &["ts", "tsx"],
    keywords: &[
        "abstract",
        "any",
        "as",
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "declare",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "from",
        "function",
        "get",
        "if",
        "implements",
        "import",
        "in",
        "instanceof",
        "interface",
        "keyof",
        "let",
        "namespace",
        "never",
        "new",
        "null",
        "of",
        "private",
        "protected",
        "public",
        "readonly",
        "return",
        "set",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "type",
        "typeof",
        "undefined",
        "unknown",
        "var",
        "void",
        "while",
        "yield",
    ],
    types: &[
        "string", "number", "boolean", "object", "Array", "Promise", "Record", "Partial",
    ],
    line_comment: "//",
    block_comment: Some(("/*", "*/")),
    strings: &['"', '\'', '`'],
    highlight_caps: true,
};

static GO: Language = Language {
    name: "go",
    exts: &["go"],
    keywords: &[
        "break",
        "case",
        "chan",
        "const",
        "continue",
        "default",
        "defer",
        "else",
        "fallthrough",
        "false",
        "for",
        "func",
        "go",
        "goto",
        "if",
        "import",
        "interface",
        "map",
        "nil",
        "package",
        "range",
        "return",
        "select",
        "struct",
        "switch",
        "true",
        "type",
        "var",
    ],
    types: &[
        "bool",
        "string",
        "int",
        "int8",
        "int16",
        "int32",
        "int64",
        "uint",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
        "uintptr",
        "byte",
        "rune",
        "float32",
        "float64",
        "complex64",
        "complex128",
        "error",
        "any",
    ],
    line_comment: "//",
    block_comment: Some(("/*", "*/")),
    strings: &['"', '`', '\''],
    highlight_caps: false,
};

static JAVA: Language = Language {
    name: "java",
    exts: &["java"],
    keywords: &[
        "abstract",
        "assert",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "default",
        "do",
        "else",
        "enum",
        "extends",
        "false",
        "final",
        "finally",
        "for",
        "if",
        "implements",
        "import",
        "instanceof",
        "interface",
        "native",
        "new",
        "null",
        "package",
        "private",
        "protected",
        "public",
        "record",
        "return",
        "static",
        "strictfp",
        "super",
        "switch",
        "synchronized",
        "this",
        "throw",
        "throws",
        "transient",
        "true",
        "try",
        "var",
        "volatile",
        "while",
    ],
    types: &[
        "int", "long", "double", "float", "boolean", "char", "byte", "short", "void", "String",
    ],
    line_comment: "//",
    block_comment: Some(("/*", "*/")),
    strings: &['"', '\''],
    highlight_caps: true,
};

static JSON_LANG: Language = Language {
    name: "json",
    exts: &["json", "jsonc"],
    keywords: &["true", "false", "null"],
    types: &[],
    line_comment: "//",
    block_comment: Some(("/*", "*/")),
    strings: &['"'],
    highlight_caps: false,
};

static TOML: Language = Language {
    name: "toml",
    exts: &["toml", "ini", "cfg", "conf"],
    keywords: &["true", "false"],
    types: &[],
    line_comment: "#",
    block_comment: None,
    strings: &['"', '\''],
    highlight_caps: false,
};

static YAML: Language = Language {
    name: "yaml",
    exts: &["yml", "yaml"],
    keywords: &["true", "false", "null", "yes", "no"],
    types: &[],
    line_comment: "#",
    block_comment: None,
    strings: &['"', '\''],
    highlight_caps: false,
};

static HTML: Language = Language {
    name: "html",
    exts: &["html", "htm", "xml", "svg", "vue"],
    keywords: &[
        "div", "span", "html", "head", "body", "script", "style", "link", "meta", "title", "a",
        "p", "ul", "ol", "li", "table", "tr", "td", "th", "img", "input", "button", "form",
        "class", "id", "href", "src",
    ],
    types: &[],
    line_comment: "",
    block_comment: Some(("<!--", "-->")),
    strings: &['"', '\''],
    highlight_caps: false,
};

static CSS: Language = Language {
    name: "css",
    exts: &["css", "scss", "less"],
    keywords: &[
        "important",
        "media",
        "keyframes",
        "import",
        "font-face",
        "root",
    ],
    types: &[],
    line_comment: "",
    block_comment: Some(("/*", "*/")),
    strings: &['"', '\''],
    highlight_caps: false,
};

static SHELL: Language = Language {
    name: "shell",
    exts: &["sh", "bash", "zsh", "ps1", "bat", "cmd"],
    keywords: &[
        "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "in",
        "function", "return", "exit", "local", "export", "source", "echo", "read", "set", "shift",
        "break", "continue",
    ],
    types: &[],
    line_comment: "#",
    block_comment: None,
    strings: &['"', '\''],
    highlight_caps: false,
};

static LANGS: &[&Language] = &[
    &RUST, &C, &CPP, &PYTHON, &JS, &TS, &GO, &JAVA, &JSON_LANG, &TOML, &YAML, &HTML, &CSS, &SHELL,
];

pub fn detect(path: &Path) -> &'static Language {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    for lang in LANGS {
        if lang.exts.contains(&ext.as_str()) {
            return lang;
        }
    }
    &PLAIN
}

fn match_at(chars: &[char], i: usize, pat: &str) -> bool {
    for (j, pc) in (i..).zip(pat.chars()) {
        if j >= chars.len() || chars[j] != pc {
            return false;
        }
    }
    true
}

/// Highlight one line. Returns one `Tok` per char plus the state carried into
/// the next line.
pub fn highlight_line(line: &str, lang: &Language, state: LineState) -> (Vec<Tok>, LineState) {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut toks = vec![Tok::Normal; n];
    let mut st = state;
    let mut i = 0;

    while i < n {
        // Inside a multi-line block comment.
        if st.in_block {
            if let Some((_, close)) = lang.block_comment {
                if match_at(&chars, i, close) {
                    let clen = close.chars().count();
                    for t in toks.iter_mut().take((i + clen).min(n)).skip(i) {
                        *t = Tok::Comment;
                    }
                    i += clen;
                    st.in_block = false;
                    continue;
                }
            }
            toks[i] = Tok::Comment;
            i += 1;
            continue;
        }

        let c = chars[i];

        // Line comment: rest of line.
        if !lang.line_comment.is_empty() && match_at(&chars, i, lang.line_comment) {
            for t in toks.iter_mut().skip(i) {
                *t = Tok::Comment;
            }
            break;
        }

        // Block comment start.
        if let Some((open, _)) = lang.block_comment {
            if match_at(&chars, i, open) {
                let olen = open.chars().count();
                for t in toks.iter_mut().take((i + olen).min(n)).skip(i) {
                    *t = Tok::Comment;
                }
                i += olen;
                st.in_block = true;
                continue;
            }
        }

        // Strings.
        if lang.strings.contains(&c) {
            toks[i] = Tok::Str;
            let q = c;
            let mut j = i + 1;
            while j < n {
                toks[j] = Tok::Str;
                if chars[j] == '\\' && j + 1 < n {
                    toks[j + 1] = Tok::Str;
                    j += 2;
                    continue;
                }
                if chars[j] == q {
                    j += 1;
                    break;
                }
                j += 1;
            }
            i = j;
            continue;
        }

        // Numbers.
        if c.is_ascii_digit() || (c == '.' && i + 1 < n && chars[i + 1].is_ascii_digit()) {
            let mut j = i;
            while j < n && (chars[j].is_ascii_alphanumeric() || chars[j] == '.' || chars[j] == '_')
            {
                toks[j] = Tok::Number;
                j += 1;
            }
            i = j;
            continue;
        }

        // Identifiers / keywords / types / function calls.
        if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let tok = if lang.keywords.contains(&word.as_str()) {
                Tok::Keyword
            } else if lang.types.contains(&word.as_str())
                || (lang.highlight_caps && c.is_uppercase())
            {
                Tok::Type
            } else if j < n
                && (chars[j] == '(' || (chars[j] == '!' && j + 1 < n && chars[j + 1] == '('))
            {
                Tok::Func
            } else {
                Tok::Normal
            };
            for t in toks.iter_mut().take(j).skip(i) {
                *t = tok;
            }
            i = j;
            continue;
        }

        if "(){}[]<>=+-*/%&|^!~?:;,.@#$".contains(c) {
            toks[i] = Tok::Punct;
        }
        i += 1;
    }

    (toks, st)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(name: &str) -> &'static Language {
        LANGS.iter().find(|l| l.name == name).unwrap()
    }

    #[test]
    fn detect_by_extension() {
        assert_eq!(detect(Path::new("a/b/main.rs")).name, "rust");
        assert_eq!(detect(Path::new("x.py")).name, "python");
        assert_eq!(detect(Path::new("noext")).name, "text");
    }

    #[test]
    fn rust_tokens() {
        let (toks, st) = highlight_line("let x = 42; // hi", lang("rust"), LineState::default());
        assert!(!st.in_block);
        assert_eq!(toks[0], Tok::Keyword); // 'l' of let
        assert_eq!(toks[8], Tok::Number); // '4'
        assert_eq!(toks[12], Tok::Comment); // '/'
    }

    #[test]
    fn block_comment_carries_over() {
        let l = lang("rust");
        let (_, st) = highlight_line("foo /* start", l, LineState::default());
        assert!(st.in_block);
        let (toks, st2) = highlight_line("still */ let", l, st);
        assert!(!st2.in_block);
        assert_eq!(toks[0], Tok::Comment);
        assert_eq!(toks[9], Tok::Keyword); // 'l' of let after close
    }

    #[test]
    fn string_with_escape() {
        let (toks, _) = highlight_line(r#"a = "x\"y" b"#, lang("rust"), LineState::default());
        assert_eq!(toks[4], Tok::Str);
        assert_eq!(toks[9], Tok::Str); // closing quote
        assert_eq!(toks[11], Tok::Normal); // b outside string
    }

    #[test]
    fn comment_start_inside_string_ignored() {
        let (toks, st) =
            highlight_line(r#"s = "no // comment""#, lang("rust"), LineState::default());
        assert!(!st.in_block);
        assert_eq!(toks[6], Tok::Str);
        assert_eq!(toks[18], Tok::Str);
    }
}
