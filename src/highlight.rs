//! Syntax highlighting for code blocks.
//!
//! The word after a block's opening fence (```` ```rust ````) names its
//! language. A tokenizer for that language splits the code into tokens, and
//! every token gets one of a small set of roles; the theme has one colour per
//! role. A block with no language, or one
//! this module does not know, is left uncoloured.
//!
//! One tokenizer serves both the editor and Reading view, so the two always
//! agree. It is
//! deliberately approximate: good at what code looks like, not a parser.

use std::ops::Range;

/// What a piece of code is: the roles a theme has a colour for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Comment,
    /// Function, macro and type names.
    Function,
    /// Variables with a sigil (`$name`) and other things that stand out.
    Important,
    Keyword,
    Operator,
    /// Member names, JSON and TOML keys, CSS properties.
    Property,
    Punctuation,
    String,
    /// Markup tags, constants, lifetimes, removed lines in a diff.
    Tag,
    /// Numbers and literals such as `true`.
    Value,
}

/// Tokens of `code` in order. Text between them has no role.
pub fn highlight(language: &str, code: &str) -> Vec<(Range<usize>, Token)> {
    let mut out = Vec::new();
    match lexer(language) {
        Some(Lexer::Generic(syntax)) => generic(syntax, code, 0, &mut out),
        Some(Lexer::Markup) => markup(code, &mut out),
        Some(Lexer::Css) => css(code, &mut out),
        Some(Lexer::Config { toml }) => config(code, toml, &mut out),
        Some(Lexer::Diff) => diff(code, &mut out),
        None => {}
    }
    // Callers slice the code with these ranges, so whatever the tokenizers did
    // with unusual input, hand back only ranges that are safe to slice with.
    let mut at = 0;
    out.retain(|(range, _)| {
        let ok = range.start >= at
            && range.start < range.end
            && range.end <= code.len()
            && code.is_char_boundary(range.start)
            && code.is_char_boundary(range.end);
        if ok {
            at = range.end;
        }
        ok
    });
    out
}

enum Lexer {
    Generic(&'static Syntax),
    Markup,
    Css,
    Config { toml: bool },
    Diff,
}

/// The tokenizer for a language name as written after a fence.
fn lexer(language: &str) -> Option<Lexer> {
    let name = language.trim().to_ascii_lowercase();
    let name = name.strip_prefix("language-").unwrap_or(&name);
    let syntax = match name {
        "rust" | "rs" => &RUST,
        "python" | "py" => &PYTHON,
        "javascript" | "js" | "jsx" | "mjs" | "cjs" | "node" => &JAVASCRIPT,
        "typescript" | "ts" | "tsx" => &TYPESCRIPT,
        "json" | "jsonc" | "json5" => &JSON,
        "c" | "h" => &C,
        "cpp" | "c++" | "cc" | "cxx" | "hpp" => &CPP,
        "csharp" | "cs" | "c#" | "dotnet" => &CSHARP,
        "java" => &JAVA,
        "kotlin" | "kt" | "kts" => &KOTLIN,
        "go" | "golang" => &GO,
        "swift" => &SWIFT,
        "php" => &PHP,
        "ruby" | "rb" => &RUBY,
        "lua" => &LUA,
        "bash" | "sh" | "shell" | "zsh" => &BASH,
        "powershell" | "ps1" | "pwsh" | "ps" => &POWERSHELL,
        "sql" | "sqlite" | "mysql" | "postgresql" | "postgres" | "plsql" => &SQL,
        "html" | "xml" | "svg" | "markup" | "xhtml" | "mathml" => return Some(Lexer::Markup),
        "css" | "scss" | "less" => return Some(Lexer::Css),
        "yaml" | "yml" => return Some(Lexer::Config { toml: false }),
        "toml" | "ini" | "cfg" | "conf" => return Some(Lexer::Config { toml: true }),
        "diff" | "patch" => return Some(Lexer::Diff),
        _ => return None,
    };
    Some(Lexer::Generic(syntax))
}

/// What the generic tokenizer needs to know about a language.
struct Syntax {
    line_comments: &'static [&'static str],
    block_comments: &'static [(&'static str, &'static str)],
    /// String delimiters, longest first.
    strings: &'static [&'static str],
    /// Whether an ordinary quoted string may run over several lines.
    multiline_strings: bool,
    /// Space-separated word lists.
    keywords: &'static str,
    values: &'static str,
    /// Well-known functions and commands that are called without `(`.
    builtins: &'static str,
    case_insensitive: bool,
    /// `Capitalised` names are types and `ALL_CAPS` names constants.
    types: bool,
    /// `value.name` is a property.
    members: bool,
    /// Characters that start a variable, like the `$` of `$name`.
    sigils: &'static str,
    /// Names may contain dashes (`Get-ChildItem`).
    dashed: bool,
    /// A string followed by `:` is a key.
    string_keys: bool,
    /// `#include` and friends.
    preprocessor: bool,
    /// Lifetimes (`'a`), macros (`name!`) and attributes (`#[...]`).
    rust: bool,
}

const C_LIKE: Syntax = Syntax {
    line_comments: &["//"],
    block_comments: &[("/*", "*/")],
    strings: &["\"", "'"],
    multiline_strings: false,
    keywords: "",
    values: "true false null",
    builtins: "",
    case_insensitive: false,
    types: true,
    members: true,
    sigils: "",
    dashed: false,
    string_keys: false,
    preprocessor: false,
    rust: false,
};

const SCRIPT: Syntax = Syntax { line_comments: &["#"], block_comments: &[], types: false, ..C_LIKE };

const RUST: Syntax = Syntax {
    keywords: "as async await break const continue crate dyn else enum extern fn for if impl in let \
        loop match mod move mut pub ref return self Self static struct super trait type unsafe use \
        where while bool char f32 f64 i8 i16 i32 i64 i128 isize str u8 u16 u32 u64 u128 usize",
    values: "true false",
    rust: true,
    ..C_LIKE
};

const PYTHON: Syntax = Syntax {
    strings: &["\"\"\"", "'''", "\"", "'"],
    keywords: "and as assert async await break case class continue def del elif else except finally \
        for from global if import in is lambda match nonlocal not or pass raise return try while \
        with yield",
    values: "True False None",
    types: true,
    ..SCRIPT
};

const JAVASCRIPT: Syntax = Syntax {
    strings: &["\"", "'", "`"],
    keywords: "async await break case catch class const continue debugger default delete do else \
        export extends finally for from function get if import in instanceof let new of return set \
        static super switch this throw try typeof var void while with yield",
    values: "true false null undefined NaN Infinity",
    ..C_LIKE
};

const TYPESCRIPT: Syntax = Syntax {
    keywords: "abstract any as async await bigint boolean break case catch class const continue \
        debugger declare default delete do else enum export extends finally for from function get \
        if implements import in infer instanceof interface is keyof let namespace never new number \
        object of private protected public readonly return satisfies set static string super \
        switch symbol this throw try type typeof unknown var void while with yield",
    ..JAVASCRIPT
};

const JSON: Syntax = Syntax { strings: &["\""], types: false, members: false, string_keys: true, ..C_LIKE };

const C: Syntax = Syntax {
    keywords: "auto break case char const continue default do double else enum extern float for \
        goto if inline int long register restrict return short signed sizeof static struct switch \
        typedef union unsigned void volatile while",
    values: "true false NULL",
    preprocessor: true,
    ..C_LIKE
};

const CPP: Syntax = Syntax {
    keywords: "alignas alignof auto bool break case catch char class concept const const_cast \
        constexpr continue co_await co_return co_yield decltype default delete do double \
        dynamic_cast else enum explicit export extern final float for friend goto if inline int \
        long mutable namespace new noexcept operator override private protected public register \
        reinterpret_cast requires return short signed sizeof static static_assert static_cast \
        struct switch template this thread_local throw try typedef typeid typename union unsigned \
        using virtual void volatile wchar_t while",
    values: "true false nullptr NULL",
    ..C
};

const CSHARP: Syntax = Syntax {
    keywords: "abstract as async await base bool break byte case catch char checked class const \
        continue decimal default delegate do double else enum event explicit extern finally fixed \
        float for foreach get goto if implicit in init int interface internal is lock long \
        namespace new object operator out override params private protected public readonly \
        record ref return sbyte sealed set short sizeof stackalloc static string struct switch \
        this throw try typeof uint ulong unchecked unsafe ushort using var virtual void volatile \
        when where while yield",
    preprocessor: true,
    ..C_LIKE
};

const JAVA: Syntax = Syntax {
    keywords: "abstract assert boolean break byte case catch char class const continue default do \
        double else enum extends final finally float for goto if implements import instanceof int \
        interface long native new package permits private protected public record return sealed \
        short static strictfp super switch synchronized this throw throws transient try var void \
        volatile while yield",
    ..C_LIKE
};

const KOTLIN: Syntax = Syntax {
    strings: &["\"\"\"", "\"", "'"],
    keywords: "abstract actual annotation as break by catch class companion const constructor \
        continue crossinline data do else enum expect external final finally for fun get if \
        import in infix init inline inner interface internal is lateinit noinline object open \
        operator out override package private protected public reified return sealed set super \
        suspend tailrec this throw try typealias val var vararg when where while",
    ..C_LIKE
};

const GO: Syntax = Syntax {
    strings: &["\"", "'", "`"],
    keywords: "any bool break byte case chan complex64 complex128 const continue default defer \
        else error fallthrough float32 float64 for func go goto if import int int8 int16 int32 \
        int64 interface map package range return rune select string struct switch type uint \
        uint8 uint16 uint32 uint64 uintptr var",
    values: "true false nil iota",
    ..C_LIKE
};

const SWIFT: Syntax = Syntax {
    strings: &["\"\"\"", "\""],
    keywords: "actor as associatedtype async await break case catch class continue default defer \
        deinit do else enum extension fallthrough fileprivate final for func guard if import in \
        indirect init inout internal is lazy let mutating nonmutating open operator override \
        private protocol public repeat rethrows return self Self some static struct subscript \
        super switch throw throws try typealias var weak where while",
    values: "true false nil",
    ..C_LIKE
};

const PHP: Syntax = Syntax {
    line_comments: &["//", "#"],
    multiline_strings: true,
    keywords: "abstract and array as break callable case catch class clone const continue declare \
        default do echo else elseif empty enum extends final finally fn for foreach function \
        global goto if implements include include_once instanceof insteadof interface isset list \
        match namespace new or print private protected public readonly require require_once \
        return static switch throw trait try unset use var while xor yield",
    values: "true false null TRUE FALSE NULL",
    sigils: "$",
    ..C_LIKE
};

const RUBY: Syntax = Syntax {
    block_comments: &[("=begin", "=end")],
    strings: &["\"", "'", "`"],
    multiline_strings: true,
    keywords: "alias and begin break case class def do else elsif end ensure extend for if in \
        include module next not or private protected public raise redo require require_relative \
        rescue retry return self super then undef unless until when while yield",
    values: "true false nil",
    builtins: "puts print p attr_accessor attr_reader attr_writer",
    types: true,
    members: true,
    sigils: "@$",
    ..SCRIPT
};

const LUA: Syntax = Syntax {
    line_comments: &["--"],
    block_comments: &[("--[[", "]]")],
    keywords: "and break do else elseif end for function goto if in local not or repeat return \
        then until while",
    values: "true false nil",
    ..SCRIPT
};

const BASH: Syntax = Syntax {
    strings: &["\"", "'", "`"],
    multiline_strings: true,
    keywords: "alias break case continue coproc declare do done elif else esac exit export fi for \
        function if in local readonly return select shift source then time typeset unset until \
        while",
    values: "true false",
    builtins: "apt awk cargo cat cd chmod chown cp curl docker echo find git grep kill ln ls make \
        mkdir mv npm pip printf pwd python read rm sed ssh sudo tar test touch wget",
    members: false,
    sigils: "$",
    ..SCRIPT
};

const POWERSHELL: Syntax = Syntax {
    block_comments: &[("<#", "#>")],
    multiline_strings: true,
    keywords: "begin break catch class continue data do dynamicparam else elseif end exit filter \
        finally for foreach from function if in param process return switch throw trap try until \
        using var while",
    values: "",
    case_insensitive: true,
    members: true,
    sigils: "$",
    dashed: true,
    ..SCRIPT
};

const SQL: Syntax = Syntax {
    line_comments: &["--"],
    strings: &["'", "\"", "`"],
    multiline_strings: true,
    keywords: "add all alter and as asc begin between boolean by case char check column commit \
        constraint create cross date decimal default delete desc distinct drop else end exists \
        float foreign from full group having if in index inner insert int integer into is join \
        key left like limit not numeric offset on or order outer primary references right \
        rollback select set table text then timestamp transaction union unique update values \
        varchar view when where with",
    case_insensitive: true,
    types: false,
    members: false,
    ..C_LIKE
};

/// Values in YAML and TOML, after the key has been taken off the line.
const CONFIG_VALUE: Syntax = Syntax {
    values: "true false null yes no on off True False Null Yes No On Off TRUE FALSE NULL",
    members: false,
    ..SCRIPT
};

/// Keywords after which the next name is a function being defined.
const DEFINERS: &str = "fn def function func fun";
const OPERATORS: &[u8] = b"+-*/%=<>!&|^~?";
const PUNCTUATION: &[u8] = b"{}[]();,.:";

/// Tokenize `code`, pushing ranges shifted by `base`.
fn generic(s: &Syntax, code: &str, base: usize, out: &mut Vec<(Range<usize>, Token)>) {
    let b = code.as_bytes();
    let mut push = |range: Range<usize>, token| out.push((base + range.start..base + range.end, token));
    let mut i = 0;
    // Whether the previous token was `fn`, `def` or the like, which makes
    // the next name a function even when no `(` follows it directly.
    let mut definer = false;

    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let after_definer = std::mem::take(&mut definer);
        let rest = &code[i..];

        if let Some((open, close)) = s.block_comments.iter().find(|(open, _)| rest.starts_with(open)) {
            let end = rest[open.len()..].find(close).map_or(b.len(), |p| i + open.len() + p + close.len());
            push(i..end, Token::Comment);
            i = end;
            continue;
        }
        if s.line_comments.iter().any(|prefix| rest.starts_with(prefix)) {
            let end = line_end(code, i);
            push(i..end, Token::Comment);
            i = end;
            continue;
        }
        if c == b'#' && s.preprocessor && code[..i].rsplit('\n').next().is_some_and(|l| l.trim().is_empty()) {
            let end = i + 1 + word_len(&rest[1..], false);
            push(i..end, Token::Property);
            // The header of `#include <stdio.h>` reads as a string.
            let include = &rest[..end - i] == "#include";
            let line = &code[end..line_end(code, end)];
            let open = line.len() - line.trim_start().len();
            i = match line.find('>') {
                Some(close) if include && line[open..].starts_with('<') => {
                    push(end + open..end + close + 1, Token::String);
                    end + close + 1
                }
                _ => end,
            };
            continue;
        }
        if c == b'#' && s.rust && (rest.starts_with("#[") || rest.starts_with("#![")) {
            let end = rest.find(']').map_or(line_end(code, i), |p| i + p + 1);
            push(i..end, Token::String);
            i = end;
            continue;
        }
        if s.sigils.as_bytes().contains(&c) {
            let end = variable_end(code, i);
            if end > i + 1 {
                push(i..end, Token::Important);
                i = end;
                continue;
            }
        }
        if c == b'\'' && s.rust {
            let (end, token) = rust_quote(code, i);
            push(i..end, token);
            i = end;
            continue;
        }
        if let Some(delimiter) = s.strings.iter().find(|d| rest.starts_with(**d)) {
            let multiline = s.multiline_strings || delimiter.len() > 1 || *delimiter == "`";
            let end = string_end(code, i + delimiter.len(), delimiter, multiline);
            let is_key = s.string_keys && code[end..].trim_start().starts_with(':');
            push(i..end, if is_key { Token::Property } else { Token::String });
            i = end;
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let end = number_end(b, i);
            push(i..end, Token::Value);
            i = end;
            continue;
        }

        let first = rest.chars().next().unwrap_or(' ');
        if first.is_alphabetic() || first == '_' {
            let word = &rest[..word_len(rest, s.dashed)];
            let end = i + word.len();
            let next = b.get(end).copied();
            let listed = |list: &str| {
                list.split_ascii_whitespace().any(|w| if s.case_insensitive { w.eq_ignore_ascii_case(word) } else { w == word })
            };
            let after_dot = i > 0 && b[i - 1] == b'.' && !(i > 1 && b[i - 2] == b'.');
            let token = if listed(s.keywords) {
                definer = DEFINERS.split(' ').any(|d| d == word);
                Some(Token::Keyword)
            } else if listed(s.values) {
                Some(Token::Value)
            } else if s.rust && next == Some(b'!') && matches!(b.get(end + 1), Some(b'(' | b'[' | b'{')) {
                // A macro call; the `!` belongs to the name.
                push(i..end + 1, Token::Property);
                i = end + 1;
                continue;
            } else if next == Some(b'(') || listed(s.builtins) || after_definer {
                Some(Token::Function)
            } else if s.dashed && word.contains('-') {
                Some(Token::Function)
            } else if s.members && after_dot {
                Some(Token::Property)
            } else if s.types && first.is_uppercase() {
                let constant = word.len() > 1 && !word.chars().any(char::is_lowercase);
                Some(if constant { Token::Tag } else { Token::Function })
            } else {
                None
            };
            if let Some(token) = token {
                push(i..end, token);
            }
            i = end;
            continue;
        }

        if OPERATORS.contains(&c) {
            let end = i + b[i..].iter().take_while(|c| OPERATORS.contains(c)).count();
            push(i..end, Token::Operator);
            i = end;
        } else if PUNCTUATION.contains(&c) {
            push(i..i + 1, Token::Punctuation);
            i += 1;
        } else {
            i += first.len_utf8();
        }
    }
}

/// Offset of the end of the line that `from` is on.
fn line_end(code: &str, from: usize) -> usize {
    code[from..].find('\n').map_or(code.len(), |p| from + p)
}

/// Length of the name at the start of `text`.
fn word_len(text: &str, dashed: bool) -> usize {
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let dash = dashed && c == '-' && chars.peek().is_some_and(|(_, next)| next.is_alphabetic());
        if !(c.is_alphanumeric() || c == '_' || dash) {
            return at;
        }
    }
    text.len()
}

/// End of the string whose contents start at `from`, past its closing delimiter.
/// An unclosed string ends with its line, or with the code if it may span lines.
fn string_end(code: &str, from: usize, delimiter: &str, multiline: bool) -> usize {
    let b = code.as_bytes();
    let mut j = from;
    while j < b.len() {
        if b[j] == b'\\' {
            j += 2;
        } else if b[j] == b'\n' && !multiline {
            return j;
        } else if b[j..].starts_with(delimiter.as_bytes()) {
            return j + delimiter.len();
        } else {
            j += 1;
        }
    }
    b.len()
}

fn number_end(b: &[u8], from: usize) -> usize {
    let hex = b[from..].starts_with(b"0x") || b[from..].starts_with(b"0X");
    let mut j = from + 1;
    while j < b.len() {
        let more = match b[j] {
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' => true,
            // `1.5`, but not the range `1..5` or the call `1.max(2)`.
            b'.' => b.get(j + 1).is_some_and(u8::is_ascii_digit),
            // The sign of an exponent: `1e-5`.
            b'+' | b'-' => !hex && matches!(b[j - 1], b'e' | b'E') && b.get(j + 1).is_some_and(u8::is_ascii_digit),
            _ => false,
        };
        if !more {
            break;
        }
        j += 1;
    }
    j
}

/// End of the variable whose sigil is at `from`: `$name`, `${name}`, `$1`, `$?`.
fn variable_end(code: &str, from: usize) -> usize {
    let b = code.as_bytes();
    let after = from + 1;
    match b.get(after) {
        Some(b'{') => code[after..].find('}').map_or(line_end(code, after), |p| after + p + 1),
        Some(c) if b"#?@*!$-".contains(c) || c.is_ascii_digit() => after + 1,
        Some(_) => after + word_len(&code[after..], false),
        None => after,
    }
}

/// What starts at the `'` at `from` in Rust: a character or a lifetime.
fn rust_quote(code: &str, from: usize) -> (usize, Token) {
    let rest = &code[from + 1..];
    if rest.starts_with('\\') {
        return (string_end(code, from + 1, "'", false), Token::String);
    }
    let mut chars = rest.chars();
    match (chars.next(), chars.next()) {
        (Some(c), Some('\'')) if c != '\'' => (from + 1 + c.len_utf8() + 1, Token::String),
        _ => match word_len(rest, false) {
            0 => (from + 1, Token::Punctuation),
            len => (from + 1 + len, Token::Tag),
        },
    }
}

/// HTML and XML: tags, attributes, comments and entities.
fn markup(code: &str, out: &mut Vec<(Range<usize>, Token)>) {
    let b = code.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i..].starts_with(b"<!--") {
            let end = code[i..].find("-->").map_or(b.len(), |p| i + p + 3);
            out.push((i..end, Token::Comment));
            i = end;
        } else if b[i..].starts_with(b"<!") || b[i..].starts_with(b"<?") {
            // Doctype, CDATA, processing instructions.
            let end = code[i..].find('>').map_or(b.len(), |p| i + p + 1);
            out.push((i..end, Token::Comment));
            i = end;
        } else if b[i] == b'<' && b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'/') {
            i = markup_tag(code, i, out);
        } else if b[i] == b'&' {
            let name = b[i + 1..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == b'#').count();
            if name > 0 && b.get(i + 1 + name) == Some(&b';') {
                out.push((i..i + name + 2, Token::Operator));
                i += name + 2;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
}

/// One tag starting at the `<` at `from`; returns where it ends.
fn markup_tag(code: &str, from: usize, out: &mut Vec<(Range<usize>, Token)>) -> usize {
    let b = code.as_bytes();
    let name_char = |c: u8| c.is_ascii_alphanumeric() || b"-_:.".contains(&c);
    let run = |from: usize, pred: &dyn Fn(u8) -> bool| from + b[from..].iter().take_while(|c| pred(**c)).count();

    out.push((from..from + 1, Token::Punctuation));
    let mut j = from + 1;
    if b.get(j) == Some(&b'/') {
        out.push((j..j + 1, Token::Punctuation));
        j += 1;
    }
    let end = run(j, &name_char);
    out.push((j..end, Token::Tag));
    j = end;

    while j < b.len() {
        match b[j] {
            b'>' => {
                out.push((j..j + 1, Token::Punctuation));
                return j + 1;
            }
            b'/' | b'=' => {
                out.push((j..j + 1, Token::Punctuation));
                j += 1;
            }
            quote @ (b'"' | b'\'') => {
                let end = b[j + 1..].iter().position(|c| *c == quote).map_or(b.len(), |p| j + p + 2);
                out.push((j..end, Token::Keyword));
                j = end;
            }
            c if name_char(c) => {
                // A bare word is an attribute's value after `=`, its name otherwise.
                let end = run(j, &name_char);
                let is_value = code[..j].trim_end().ends_with('=');
                out.push((j..end, if is_value { Token::Keyword } else { Token::String }));
                j = end;
            }
            _ => j += 1,
        }
    }
    b.len()
}

/// CSS, and near enough SCSS and Less: selectors, at-rules and declarations.
fn css(code: &str, out: &mut Vec<(Range<usize>, Token)>) {
    let b = code.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
        } else if b[i..].starts_with(b"/*") {
            let end = code[i..].find("*/").map_or(b.len(), |p| i + p + 2);
            out.push((i..end, Token::Comment));
            i = end;
        } else if b[i..].starts_with(b"//") {
            let end = line_end(code, i);
            out.push((i..end, Token::Comment));
            i = end;
        } else if b"{};".contains(&b[i]) {
            out.push((i..i + 1, Token::Punctuation));
            i += 1;
        } else {
            // Everything up to the next `{`, `;` or `}` is one selector or declaration.
            let end = css_segment_end(b, i);
            let text_end = i + code[i..end].trim_end().len();
            if b[i] == b'@' {
                let name = i + 1 + word_len(&code[i + 1..end], true);
                out.push((i..name, Token::Keyword));
                css_values(code, name..end, out);
            } else if b.get(end) == Some(&b'{') {
                out.push((i..text_end, Token::String));
            } else if let Some(colon) = code[i..end].find(':') {
                out.push((i..i + code[i..i + colon].trim_end().len(), Token::Property));
                out.push((i + colon..i + colon + 1, Token::Punctuation));
                css_values(code, i + colon + 1..end, out);
            } else {
                css_values(code, i..end, out);
            }
            i = end.max(i + 1);
        }
    }
}

/// Where the selector or declaration starting at `from` ends: at the next
/// `{`, `;` or `}` that is not inside a string or parentheses.
fn css_segment_end(b: &[u8], from: usize) -> usize {
    let mut depth = 0usize;
    let mut j = from;
    while j < b.len() {
        match b[j] {
            quote @ (b'"' | b'\'') => {
                j += 1 + b[j + 1..].iter().position(|c| *c == quote || *c == b'\n').unwrap_or(b.len() - j - 1);
            }
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'{' | b';' | b'}' if depth == 0 => return j,
            _ => {}
        }
        j += 1;
    }
    b.len()
}

/// The value part of a CSS declaration or at-rule.
fn css_values(code: &str, range: Range<usize>, out: &mut Vec<(Range<usize>, Token)>) {
    let b = code.as_bytes();
    let mut j = range.start;
    while j < range.end {
        let c = b[j];
        if c == b'"' || c == b'\'' {
            let end = string_end(code, j + 1, if c == b'"' { "\"" } else { "'" }, false).min(range.end);
            out.push((j..end, Token::String));
            j = end;
        } else if c == b'!' && code[j..range.end].starts_with("!important") {
            out.push((j..j + 10, Token::Important));
            j += 10;
        } else if c.is_ascii_digit()
            || (b".-+".contains(&c) && b.get(j + 1).is_some_and(u8::is_ascii_digit))
            || (c == b'-' && b[j + 1..range.end].starts_with(b"."))
            || (c == b'#' && b.get(j + 1).is_some_and(u8::is_ascii_hexdigit))
        {
            // A number with its unit (`1.5rem`, `50%`) or a hex colour.
            let more = |c: &u8| c.is_ascii_alphanumeric() || *c == b'.' || *c == b'%';
            let end = j + 1 + b[j + 1..range.end].iter().take_while(|c| more(c)).count();
            out.push((j..end, Token::Value));
            j = end;
        } else if c.is_ascii_alphabetic() || c == b'-' || c == b'$' || c == b'@' {
            let end = (j + 1 + word_len(&code[j + 1..range.end], true)).min(range.end);
            if c == b'$' || c == b'@' || code[j..end].starts_with("--") {
                out.push((j..end, Token::Important));
            } else if b.get(end) == Some(&b'(') {
                out.push((j..end, Token::Function));
            }
            j = end;
        } else {
            if b"(),".contains(&c) {
                out.push((j..j + 1, Token::Punctuation));
            }
            j += code[j..].chars().next().map_or(1, char::len_utf8);
        }
    }
}

/// YAML and TOML (and INI, which TOML's rules fit): a key, then a value, per line.
fn config(code: &str, toml: bool, out: &mut Vec<(Range<usize>, Token)>) {
    let mut start = 0;
    for line in code.split('\n') {
        let line_start = start;
        start += line.len() + 1;
        let body = line.trim_start();
        let mut at = line_start + line.len() - body.len();
        let end = line_start + line.trim_end().len();
        if body.trim_end().is_empty() {
            continue;
        }
        if body.starts_with('#') || (toml && body.starts_with(';')) {
            out.push((at..end, Token::Comment));
            continue;
        }
        if toml && body.starts_with('[') {
            // A `[table]` header, possibly with a comment after it.
            let header = body.rfind(']').map_or(end, |p| at + p + 1);
            out.push((at..header, Token::Function));
            generic(&CONFIG_VALUE, &code[header..end], header, out);
            continue;
        }
        if !toml {
            // The dashes of a YAML list, and `---` between documents.
            if matches!(body.trim_end(), "---" | "...") {
                out.push((at..end, Token::Punctuation));
                continue;
            }
            while code[at..end].starts_with("- ") || &code[at..end] == "-" {
                out.push((at..at + 1, Token::Punctuation));
                at += 1 + code[at + 1..end].len() - code[at + 1..end].trim_start().len();
            }
        }
        if let Some(separator) = config_separator(&code[at..end], toml) {
            let key_end = at + code[at..at + separator].trim_end().len();
            out.push((at..key_end, if toml { Token::Property } else { Token::Keyword }));
            out.push((at + separator..at + separator + 1, Token::Punctuation));
            at += separator + 1;
        }
        generic(&CONFIG_VALUE, &code[at..end], at, out);
    }
}

/// Offset of the `=` (TOML) or `:` (YAML) that ends the key of `line`, if it
/// has one. Separators inside quotes do not count, and YAML's `:` must be
/// followed by a space or the end of the line.
fn config_separator(line: &str, toml: bool) -> Option<usize> {
    let b = line.as_bytes();
    let mut quote = None;
    for (j, &c) in b.iter().enumerate() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if toml && c == b'=' => return Some(j),
            None if !toml && c == b':' && matches!(b.get(j + 1), None | Some(b' ' | b'\t')) => return Some(j),
            // Past this point a YAML line is a value, not a key.
            None if !toml && (c == b'#' || c == b'{' || c == b'[') => return None,
            None => {}
        }
    }
    None
}

/// Unified diffs: added and removed lines.
fn diff(code: &str, out: &mut Vec<(Range<usize>, Token)>) {
    let mut start = 0;
    for line in code.split('\n') {
        let range = start..start + line.trim_end_matches('\r').len();
        start += line.len() + 1;
        let token = if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") || line.starts_with("index ") {
            Token::Punctuation
        } else if line.starts_with("@@") {
            Token::Important
        } else if line.starts_with('+') {
            Token::String
        } else if line.starts_with('-') {
            Token::Tag
        } else {
            continue;
        };
        out.push((range, token));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Token::*;

    /// Tokens as (text, role) pairs.
    fn tokens<'a>(language: &str, code: &'a str) -> Vec<(&'a str, Token)> {
        highlight(language, code).into_iter().map(|(range, token)| (&code[range], token)).collect()
    }

    /// The role given to the first token whose text is `text`.
    fn role(language: &str, code: &str, text: &str) -> Option<Token> {
        tokens(language, code).into_iter().find(|(t, _)| *t == text).map(|(_, token)| token)
    }

    #[test]
    fn rust() {
        let code = "#[derive(Debug)]\nfn main<'a>() {\n    // hi\n    let x: u8 = 0x1F + 1.5e-3; /* c */\n    println!(\"a\\\"b {}\", 'c');\n    for i in 0..MAX { Vec::new().len; }\n}";
        let role = |text| role("rust", code, text);
        assert_eq!(role("#[derive(Debug)]"), Some(String));
        assert_eq!(role("fn"), Some(Keyword));
        assert_eq!(role("main"), Some(Function), "a name after `fn`, even before `<`");
        assert_eq!(role("'a"), Some(Tag));
        assert_eq!(role("// hi"), Some(Comment));
        assert_eq!(role("/* c */"), Some(Comment));
        assert_eq!(role("u8"), Some(Keyword));
        assert_eq!(role("0x1F"), Some(Value));
        assert_eq!(role("1.5e-3"), Some(Value));
        assert_eq!(role("println!"), Some(Property));
        assert_eq!(role("\"a\\\"b {}\""), Some(String), "an escaped quote does not end the string");
        assert_eq!(role("'c'"), Some(String));
        assert_eq!(role("0"), Some(Value), "`0..MAX` is two numbers' worth of range, not `0.`");
        assert_eq!(role("MAX"), Some(Tag));
        assert_eq!(role("Vec"), Some(Function));
        assert_eq!(role("new"), Some(Function));
        assert_eq!(role("len"), Some(Property));
        assert_eq!(role("x"), None);
        assert_eq!(role("{"), Some(Punctuation));
        assert_eq!(role("="), Some(Operator));
    }

    #[test]
    fn python() {
        let code = "def f(x):\n    \"\"\"Doc\n    string\"\"\"\n    return None if x else print('a # b')  # c";
        let role = |text| role("python", code, text);
        assert_eq!(role("def"), Some(Keyword));
        assert_eq!(role("f"), Some(Function));
        assert_eq!(role("\"\"\"Doc\n    string\"\"\""), Some(String), "triple quotes span lines");
        assert_eq!(role("None"), Some(Value));
        assert_eq!(role("'a # b'"), Some(String), "a # inside a string is not a comment");
        assert_eq!(role("# c"), Some(Comment));
    }

    #[test]
    fn javascript_and_typescript() {
        let code = "const s = `multi\nline ${x}`; class Foo extends Bar {} obj.prop.call(1)";
        assert_eq!(role("js", code, "const"), Some(Keyword));
        assert_eq!(role("js", code, "`multi\nline ${x}`"), Some(String));
        assert_eq!(role("js", code, "Foo"), Some(Function));
        assert_eq!(role("js", code, "prop"), Some(Property));
        assert_eq!(role("js", code, "call"), Some(Function));
        assert_eq!(role("js", "function (a) {}", "a"), None, "only a name right after `function` is defined");
        assert_eq!(role("js", "function go<T>() {}", "go"), Some(Function));
        assert_eq!(role("js", "let x: number", "number"), None);
        assert_eq!(role("ts", "let x: number", "number"), Some(Keyword));
    }

    #[test]
    fn json_keys_are_properties() {
        let code = "{ \"name\": \"notes\", \"n\": [1, -2.5, true, null] }";
        assert_eq!(
            tokens("json", code).into_iter().filter(|(_, t)| *t != Punctuation && *t != Operator).collect::<Vec<_>>(),
            [("\"name\"", Property), ("\"notes\"", String), ("\"n\"", Property), ("1", Value), ("2.5", Value), ("true", Value), ("null", Value)]
        );
    }

    #[test]
    fn c_family() {
        let code = "#include <stdio.h>\nint main(void) { char c = 'x'; return NULL; }";
        assert_eq!(role("c", code, "#include"), Some(Property));
        assert_eq!(role("c", code, "<stdio.h>"), Some(String));
        assert_eq!(role("c", "#define N 1\n#include \"a.h\"", "\"a.h\""), Some(String));
        assert_eq!(role("c", code, "int"), Some(Keyword));
        assert_eq!(role("c", code, "main"), Some(Function));
        assert_eq!(role("c", code, "'x'"), Some(String));
        assert_eq!(role("c", code, "NULL"), Some(Value));
        assert_eq!(role("cpp", "class A : public B { virtual ~A(); };", "virtual"), Some(Keyword));
        assert_eq!(role("c#", "public string Name { get; set; }", "string"), Some(Keyword));
        assert_eq!(role("java", "new ArrayList<String>()", "ArrayList"), Some(Function));
        assert_eq!(role("go", "func main() { defer f() }", "defer"), Some(Keyword));
    }

    #[test]
    fn shells() {
        let code = "echo \"n=$#\" ${HOME}/x $1 # done\nif [ -f a.txt ]; then cd ..; fi";
        let role = |text| role("bash", code, text);
        assert_eq!(role("echo"), Some(Function));
        assert_eq!(role("${HOME}"), Some(Important));
        assert_eq!(role("$1"), Some(Important));
        assert_eq!(role("# done"), Some(Comment));
        assert_eq!(role("if"), Some(Keyword));
        assert_eq!(role("txt"), None, "a file extension is not a property");
        let ps = "Get-ChildItem -Recurse | Where-Object { $_.Name } # x";
        assert_eq!(super::tests::role("powershell", ps, "Get-ChildItem"), Some(Function));
        assert_eq!(super::tests::role("ps1", ps, "$_"), Some(Important));
        assert_eq!(super::tests::role("ps1", "FOREACH ($x in $y) {}", "FOREACH"), Some(Keyword));
    }

    #[test]
    fn sql_is_case_insensitive() {
        let code = "SELECT name, count(*) from users WHERE id = 7 -- all";
        let role = |text| role("sql", code, text);
        assert_eq!(role("SELECT"), Some(Keyword));
        assert_eq!(role("from"), Some(Keyword));
        assert_eq!(role("count"), Some(Function));
        assert_eq!(role("7"), Some(Value));
        assert_eq!(role("-- all"), Some(Comment));
        assert_eq!(role("name"), None);
    }

    #[test]
    fn markup() {
        let code = "<!-- c -->\n<a href=\"x.html\" hidden>T &amp; U</a><br/>";
        assert_eq!(
            tokens("html", code).into_iter().filter(|(_, t)| *t != Punctuation).collect::<Vec<_>>(),
            [("<!-- c -->", Comment), ("a", Tag), ("href", String), ("\"x.html\"", Keyword), ("hidden", String), ("&amp;", Operator), ("a", Tag), ("br", Tag)]
        );
        assert_eq!(role("xml", "<?xml version=\"1.0\"?><n:a b=c/>", "<?xml version=\"1.0\"?>"), Some(Comment));
        assert_eq!(role("xml", "<n:a b=c/>", "c"), Some(Keyword));
    }

    #[test]
    fn css() {
        let code = "@media (min-width: 10px) {\n  a:hover, .b { color: #fff; margin: -1.5rem auto !important; background: url(\"x;y.png\") }\n} /* c */";
        let role = |text| role("css", code, text);
        assert_eq!(role("@media"), Some(Keyword));
        assert_eq!(role("a:hover, .b"), Some(String));
        assert_eq!(role("color"), Some(Property));
        assert_eq!(role("#fff"), Some(Value));
        assert_eq!(role("-1.5rem"), Some(Value));
        assert_eq!(role("!important"), Some(Important));
        assert_eq!(role("url"), Some(Function));
        assert_eq!(role("\"x;y.png\""), Some(String), "a ; inside a string does not end the declaration");
        assert_eq!(role("/* c */"), Some(Comment));
        assert_eq!(role("auto"), None);
    }

    #[test]
    fn yaml_and_toml() {
        let yaml = "# c\nname: notes # d\nlist:\n  - a: 1\n  - \"x: y\"\nurl: http://e.f\n---";
        assert_eq!(role("yaml", yaml, "# c"), Some(Comment));
        assert_eq!(role("yaml", yaml, "name"), Some(Keyword));
        assert_eq!(role("yaml", yaml, "# d"), Some(Comment));
        assert_eq!(role("yaml", yaml, "a"), Some(Keyword));
        assert_eq!(role("yaml", yaml, "-"), Some(Punctuation));
        assert_eq!(role("yaml", yaml, "\"x: y\""), Some(String), "a quoted colon is not a key");
        assert_eq!(role("yml", yaml, "url"), Some(Keyword));
        assert_eq!(role("yml", yaml, "---"), Some(Punctuation));

        let toml = "[package] # c\nname = \"notes\"\nedition=2021\nok = true ; x";
        assert_eq!(role("toml", toml, "[package]"), Some(Function));
        assert_eq!(role("toml", toml, "# c"), Some(Comment));
        assert_eq!(role("toml", toml, "name"), Some(Property));
        assert_eq!(role("toml", toml, "\"notes\""), Some(String));
        assert_eq!(role("toml", toml, "2021"), Some(Value));
        assert_eq!(role("ini", toml, "true"), Some(Value));
    }

    #[test]
    fn diff() {
        let code = "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-old\n+new\n same";
        assert_eq!(
            tokens("diff", code),
            [("--- a/f", Punctuation), ("+++ b/f", Punctuation), ("@@ -1 +1 @@", Important), ("-old", Tag), ("+new", String)]
        );
    }

    #[test]
    fn language_names() {
        for name in ["rust", "RS", " Rust ", "language-rust"] {
            assert_eq!(role(name, "fn", "fn"), Some(Keyword), "{name}");
        }
        for name in ["", "text", "nonsense", "rusty"] {
            assert!(highlight(name, "fn main() {}").is_empty(), "{name}");
        }
    }

    const ALL: [&str; 23] = [
        "rust", "python", "js", "ts", "json", "c", "cpp", "c#", "java", "kotlin", "go", "swift", "php", "ruby", "lua",
        "bash", "powershell", "sql", "html", "css", "yaml", "toml", "diff",
    ];

    #[test]
    fn tokens_are_always_in_order_and_sliceable() {
        let nasty = [
            "",
            "\"unterminated",
            "'",
            "'\\",
            "\"é\\é\" é 'ю' `я` /* é",
            "x = \"a\\",
            "<a b=\"é",
            "<",
            "&é;",
            "a { b: \"é",
            "@é é:é;",
            "é: é # é\n- é\n[é",
            "$",
            "${é",
            "#[é",
            "#!é",
            "1.e5 0x .5. 1..2 9é",
            "/*",
            "--[[ é",
            "- - - é: \"é",
            "é=é\né\r\n+é\n-é",
            "fn é(é: É) -> É { é!(\"é\") }\r\n// é\r\n",
        ];
        for language in ALL {
            assert!(lexer(language).is_some(), "{language}");
            for code in nasty {
                let mut at = 0;
                for (range, _) in highlight(language, code) {
                    assert!(range.start >= at && range.start < range.end, "{language}: {code:?}");
                    let _ = &code[range.clone()];
                    at = range.end;
                }
            }
        }
    }
}
