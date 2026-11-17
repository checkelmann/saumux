//! Small lexical highlighter. Character indices match the editor's cursor model.
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    C,
    Rust,
    Script,
    Python,
    Shell,
    Json,
    Toml,
}
impl Language {
    pub fn detect(path: Option<&Path>) -> Option<Self> {
        let path = path?;
        Some(
            match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
                "rs" => Self::Rust,
                "c" | "h" | "cc" | "cpp" | "hpp" | "java" | "go" | "cs" => Self::C,
                "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Self::Script,
                "py" | "pyw" => Self::Python,
                "sh" | "bash" | "zsh" => Self::Shell,
                "json" => Self::Json,
                "toml" => Self::Toml,
                _ => return None,
            },
        )
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Token {
    #[default]
    Plain,
    Keyword,
    String,
    Number,
    Comment,
}
#[derive(Clone, Copy, Default)]
enum State {
    #[default]
    Plain,
    Block,
    Quote(char, bool),
}

pub fn highlight(lines: &[Vec<char>], language: Language) -> Vec<Vec<Token>> {
    let mut state = State::Plain;
    lines
        .iter()
        .map(|line| lex(line, language, &mut state))
        .collect()
}
fn lex(line: &[char], lang: Language, state: &mut State) -> Vec<Token> {
    let mut out = vec![Token::Plain; line.len()];
    let mut i = 0;
    let hash = matches!(lang, Language::Python | Language::Shell | Language::Toml);
    let slash = matches!(lang, Language::C | Language::Rust | Language::Script);
    while i < line.len() {
        let start = i;
        let token = match *state {
            State::Block => {
                while i < line.len() {
                    if line[i..].starts_with(&['*', '/']) {
                        i += 2;
                        *state = State::Plain;
                        break;
                    }
                    i += 1;
                }
                Token::Comment
            }
            State::Quote(q, triple) => {
                while i < line.len() {
                    if line[i] == '\\'
                        && !(matches!(lang, Language::Toml | Language::Shell) && q == '\'')
                    {
                        i = (i + 2).min(line.len());
                    } else if line[i] == q && (!triple || line[i..].starts_with(&[q, q, q])) {
                        i += if triple { 3 } else { 1 };
                        *state = State::Plain;
                        break;
                    } else {
                        i += 1;
                    }
                }
                Token::String
            }
            State::Plain => {
                let c = line[i];
                if (hash && c == '#') || (slash && line[i..].starts_with(&['/', '/'])) {
                    i = line.len();
                    Token::Comment
                } else if slash && line[i..].starts_with(&['/', '*']) {
                    i += 2;
                    *state = State::Block;
                    Token::Comment
                } else if c == '"'
                    || (c == '\''
                        && lang != Language::Json
                        && (lang != Language::Rust
                            || line.get(i + 2) == Some(&'\'')
                            || (line.get(i + 1) == Some(&'\\') && line.get(i + 3) == Some(&'\''))))
                    || (c == '`' && matches!(lang, Language::Script | Language::Shell))
                {
                    let triple = matches!(lang, Language::Python | Language::Toml)
                        && line[i..].starts_with(&[c, c, c]);
                    i += if triple { 3 } else { 1 };
                    *state = State::Quote(c, triple);
                    Token::String
                } else if c.is_ascii_digit() && (i == 0 || !word(line[i - 1])) {
                    i += 1;
                    while i < line.len()
                        && (line[i].is_ascii_alphanumeric() || matches!(line[i], '_' | '.'))
                    {
                        i += 1;
                    }
                    Token::Number
                } else if word(c) {
                    i += 1;
                    while i < line.len() && word(line[i]) {
                        i += 1;
                    }
                    let value: String = line[start..i].iter().collect();
                    if keywords(lang).split_whitespace().any(|k| k == value) {
                        Token::Keyword
                    } else {
                        Token::Plain
                    }
                } else {
                    i += 1;
                    Token::Plain
                }
            }
        };
        out[start..i].fill(token);
    }
    // Ordinary quoted strings are line-local; multiline constructs retain state.
    if let State::Quote(q, false) = *state
        && q != '`'
        && !matches!(lang, Language::Shell | Language::Toml)
    {
        *state = State::Plain;
    }
    out
}
fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}
fn keywords(lang: Language) -> &'static str {
    match lang {
        Language::Rust => {
            "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while bool char str usize i32 u32 String Option Some None Result Ok Err"
        }
        Language::C => {
            "auto bool break case catch char class const continue default defer do double else enum extern false float for func if import int interface long namespace new nil null nullptr package private protected public return short signed sizeof static string struct switch template this throw true try typedef typename union unsigned using var virtual void volatile while"
        }
        Language::Script => {
            "as async await break case catch class const continue debugger default delete do else enum export extends false finally for from function if implements import in instanceof interface let new null of private protected public return static super switch this throw true try type typeof undefined var void while yield"
        }
        Language::Python => {
            "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield"
        }
        Language::Shell => {
            "case do done elif else esac fi for function if in then until while export local readonly return exit"
        }
        Language::Json | Language::Toml => "true false null inf nan",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tokens(text: &str, lang: Language) -> Vec<Vec<Token>> {
        highlight(
            &text
                .lines()
                .map(|s| s.chars().collect())
                .collect::<Vec<_>>(),
            lang,
        )
    }
    #[test]
    fn comments_strings_unicode_and_multiline_state() {
        let t = tokens(
            "let 界 = \"//hi\"; /* a\nend */ let n = 42; // bye",
            Language::Rust,
        );
        assert_eq!(t[0][0], Token::Keyword);
        assert_eq!(t[0][4], Token::Plain);
        assert_eq!(t[0][9], Token::String);
        assert_eq!(t[1][0], Token::Comment);
        assert_eq!(t[1][7], Token::Keyword);
        assert_eq!(t[1][15], Token::Number);
        assert_eq!(t[1].last(), Some(&Token::Comment));
        let t = tokens(
            "x = '''hello\n# still string\n''' # comment",
            Language::Python,
        );
        assert!(t[1].iter().all(|t| *t == Token::String));
        assert_eq!(t[2].last(), Some(&Token::Comment));
    }
    #[test]
    fn detection_and_rust_lifetimes() {
        assert_eq!(
            Language::detect(Some(Path::new("file.TOML"))),
            Some(Language::Toml)
        );
        assert_eq!(Language::detect(Some(Path::new("notes.txt"))), None);
        let t = tokens("fn f<'a>(x: &'a str) { let c = 'x'; }", Language::Rust);
        assert_eq!(t[0][6], Token::Plain);
        assert_eq!(t[0][31], Token::String);
    }
}
