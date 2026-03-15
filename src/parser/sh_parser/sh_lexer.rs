#![allow(dead_code)]

use super::sh_token::{ShToken, ShTokenKind, Span, WordKind};
use crate::parser::source_map::SourceId;
use std::{collections::VecDeque, result::Result};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LexerError {
    #[error("Unexpected EOF: $0")]
    UnexpectedEof(Span),

    #[error("Unknown lexer error")]
    Unknown,
}

pub struct Lexer {
    input: Vec<char>,
    pub position: usize,
    source_id: SourceId,
    pending: VecDeque<ShToken>,
    finished: bool,
}

impl Iterator for Lexer {
    type Item = Result<ShToken, LexerError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        let tok = self.next_token();

        if matches!(
            tok,
            Ok(ShToken {
                kind: ShTokenKind::Eof,
                ..
            })
        ) {
            self.finished = true;
        }

        Some(tok)
    }
}

impl Lexer {
    const SEP_CHARS: &str = ";&|<>() \n\t";

    pub fn new(input: Vec<char>, source_id: SourceId) -> Lexer {
        Lexer {
            input,
            source_id,
            position: 0,
            pending: VecDeque::new(),
            finished: false,
        }
    }

    pub fn read_char(&mut self) {
        self.position += 1;
    }

    fn peek_char(&self) -> Option<&char> {
        self.input.get(self.position + 1)
    }

    fn cmp_next_char(&self, char: char) -> bool {
        return if self.position + 1 >= self.input.len() {
            false
        } else {
            self.input[self.position + 1] == char
        };
    }

    fn eat_ws(&mut self) {
        while self.position < self.input.len() && matches!(self.input[self.position], ' ' | '\t') {
            self.read_char();
        }
    }

    fn eat_line_continuation(&mut self) {
        while self.position + 1 < self.input.len()
            && self.input[self.position] == '\\'
            && self.input[self.position + 1] == '\n'
        {
            self.position += 2;
            self.eat_ws();
        }
    }

    pub fn next_token(&mut self) -> Result<ShToken, LexerError> {
        if let Some(tok) = self.pending.pop_front() {
            return Ok(tok);
        }
        if self.input.len() <= self.position {
            let span = Span::new(self.position, self.source_id, 0);
            return Ok(ShToken::new(ShTokenKind::Eof, span));
        }
        self.eat_ws();
        self.eat_line_continuation();
        self.eat_ws();
        if self.input.len() <= self.position {
            let span = Span::new(self.position, self.source_id, 0);
            return Ok(ShToken::new(ShTokenKind::Eof, span));
        }
        let ch = self.input[self.position];
        let tok = match ch {
            '\n' => ShToken::new(
                ShTokenKind::NewLine,
                Span::new(self.position, self.source_id, 1),
            ),

            ';' => ShToken::new(
                ShTokenKind::SemiColon,
                Span::new(self.position, self.source_id, 1),
            ),

            '&' => {
                if self.cmp_next_char('&') {
                    self.read_char();
                    ShToken::new(
                        ShTokenKind::And,
                        Span::new(self.position - 1, self.source_id, 2),
                    )
                } else {
                    ShToken::new(
                        ShTokenKind::BackgroundExec,
                        Span::new(self.position, self.source_id, 1),
                    )
                }
            }

            '|' => {
                if self.position + 1 < self.input.len() && self.input[self.position + 1] == '|' {
                    self.read_char();
                    ShToken::new(
                        ShTokenKind::Or,
                        Span::new(self.position - 1, self.source_id, 2),
                    )
                } else {
                    self.read_char();
                    ShToken::new(
                        ShTokenKind::Pipe,
                        Span::new(self.position - 1, self.source_id, 1),
                    )
                }
            }

            '=' => ShToken::new(ShTokenKind::Eq, Span::new(self.position, self.source_id, 1)),
            '(' => ShToken::new(
                ShTokenKind::LParen,
                Span::new(self.position, self.source_id, 1),
            ),
            ')' => ShToken::new(
                ShTokenKind::RParen,
                Span::new(self.position, self.source_id, 1),
            ),
            '{' => ShToken::new(
                ShTokenKind::LBrace,
                Span::new(self.position, self.source_id, 1),
            ),
            '}' => ShToken::new(
                ShTokenKind::RBrace,
                Span::new(self.position, self.source_id, 1),
            ),

            '>' => {
                if self.cmp_next_char('>') || self.cmp_next_char('&') || self.cmp_next_char('|') {
                    self.read_char();
                    ShToken::new(
                        ShTokenKind::Redir,
                        Span::new(self.position - 1, self.source_id, 2),
                    )
                } else {
                    ShToken::new(
                        ShTokenKind::Redir,
                        Span::new(self.position, self.source_id, 1),
                    )
                }
            }

            '<' => {
                let start = self.position;
                if self.cmp_next_char('&') || self.cmp_next_char('>') {
                    self.read_char();
                    ShToken::new(ShTokenKind::Redir, Span::new(start, self.source_id, 2))
                } else if self.cmp_next_char('<') {
                    let mut strip_tabs = false;
                    self.read_char();
                    if self.cmp_next_char('-') {
                        self.read_char();
                        strip_tabs = true;
                    }
                    let op_len = if strip_tabs { 3 } else { 2 };
                    ShToken::new(ShTokenKind::Redir, Span::new(start, self.source_id, op_len))
                } else {
                    ShToken::new(
                        ShTokenKind::Redir,
                        Span::new(self.position, self.source_id, 1),
                    )
                }
            }

            '#' => {
                let start = self.position;
                self.read_char();

                while self.position < self.input.len() && self.input[self.position] != '\n' {
                    self.read_char();
                }

                ShToken::new(
                    ShTokenKind::Comment,
                    Span::new(start, self.source_id, self.position - start),
                )
            }

            '0'..='9' => {
                let start = self.position;
                while self
                    .peek_char()
                    .is_some_and(|c| !Lexer::SEP_CHARS.contains(*c))
                {
                    self.read_char();
                }

                let s = self.input[start..=self.position].iter().collect::<String>();

                ShToken::new(
                    classify_word(&s),
                    Span::new(start, self.source_id, self.position - start + 1),
                )
            }

            '"' | '\'' | '`' => {
                let quote = ch;
                let start = self.position;
                self.read_char();
                while self.position < self.input.len() && self.input[self.position] != quote {
                    if quote != '\''
                        && self.input[self.position] == '\\'
                        && self.position + 1 < self.input.len()
                    {
                        self.position += 2;
                        continue;
                    }
                    self.read_char();
                }
                if self.position >= self.input.len() {
                    return Err(LexerError::UnexpectedEof(Span::new(
                        start,
                        self.source_id,
                        self.position.saturating_sub(start),
                    )));
                }

                ShToken::new(
                    ShTokenKind::Word(WordKind::Word),
                    Span::new(start, self.source_id, self.position - start + 1),
                )
            }

            _ => {
                let start = self.position;
                while self
                    .peek_char()
                    .is_some_and(|c| !Lexer::SEP_CHARS.contains(*c))
                {
                    self.read_char();
                }

                let str: String = self.input[start..=self.position].iter().collect();
                let kind = classify_word(&str);

                ShToken::new(
                    kind,
                    Span::new(start, self.source_id, self.position - start + 1),
                )
            }
        };

        self.read_char();

        Ok(tok)
    }
}

pub fn is_name(s: &str) -> bool {
    let mut chars = s.chars();

    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }

    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn is_path(s: &str) -> bool {
    s.starts_with('/') || s.starts_with("./") || s.starts_with("../") || s.contains('/')
}

pub fn classify_word(s: &str) -> ShTokenKind {
    if is_name(s) {
        ShTokenKind::Word(WordKind::Name)
    } else if is_path(s) {
        ShTokenKind::Word(WordKind::Path)
    } else {
        ShTokenKind::Word(WordKind::Word)
    }
}

#[cfg(test)]
mod lexer_test {

    use crate::parser::sh_parser::{
        sh_lexer::Lexer,
        sh_token::{ShTokenKind, WordKind},
    };
    use crate::parser::source_map::SourceId;

    #[test]
    fn test() {
        let program = "echo $(pwd)";
        let id = SourceId(0);
        let mut lexer = Lexer::new(program.chars().collect(), id);
        loop {
            match lexer.next_token() {
                Ok(token) => {
                    if token.kind == ShTokenKind::Eof {
                        break;
                    }

                    println!("{:?}: {}", token, token.text(program));
                    // assert_eq!(token.kind, expected[i]);
                }

                Err(err) => {
                    eprintln!("{:?}", err);
                    break;
                }
            }
        }
    }

    #[test]
    fn lex_basic() {
        let program = "echo   hello  world 1
echo \"hello world\"
ls | fzf
./gradlew check
../gradlew test --all | fzf
/bin/rm -rf ./tmp
npm run test > test.txt
echo \"done\" >> test.txt
RUST_BACKTRACE=1 cargo test -- --nocapture && yes
docker compose&
echo 2'b'; echo hello!
";
        let expected = vec![
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Pipe,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Path),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Path),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Pipe,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Path),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Path),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Redir,
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Redir,
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::And,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::BackgroundExec,
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::SemiColon,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::NewLine,
        ];
        let mut lexer = Lexer::new(program.chars().collect(), SourceId(0));
        let mut i = 0;
        loop {
            match lexer.next_token() {
                Ok(token) => {
                    if token.kind == ShTokenKind::Eof {
                        break;
                    }

                    println!("{:?}: {}", token, token.text(program));
                    assert_eq!(token.kind, expected[i]);
                }

                Err(err) => {
                    eprintln!("{:?}", err);
                    break;
                }
            }

            i += 1;
        }
    }

    #[test]
    fn test_if() {
        let program = "
x=5

if [ \"$x\" -gt 3 ]; then
    echo \"x is greater than 3\"
elif [ \"$x\" -eq 3 ]; then
    echo \"x is equal to 3\"
else
    echo \"x is less than 3\"
fi
";
        let expected = vec![
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::NewLine,
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::SemiColon,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::SemiColon,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
        ];
        let mut lexer = Lexer::new(program.chars().collect(), SourceId(0));
        let mut i = 0;
        loop {
            match lexer.next_token() {
                Ok(token) => {
                    if token.kind == ShTokenKind::Eof {
                        break;
                    }

                    print!("ShTokenKind::{:?},", token.kind);
                    println!("{:?}", token.text(program));
                    assert_eq!(token.kind, expected[i]);
                }

                Err(err) => {
                    eprintln!("{:?}", err);
                    break;
                }
            }

            i += 1;
        }
    }

    #[test]
    fn test_for() {
        let program = "#!/bin/sh

i=1
for i in 1 2 3 4 5
do
    echo \"i = $i\"
done
";
        let mut lexer = Lexer::new(program.chars().collect(), SourceId(0));
        let mut _i = 0;
        let _expected = vec![
            ShTokenKind::Comment,
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
        ];

        loop {
            match lexer.next_token() {
                Ok(token) => {
                    if token.kind == ShTokenKind::Eof {
                        break;
                    }

                    println!("ShTokenKind::{:?},", token.kind);
                    //assert_eq!(token.kind, expected[i]);
                }

                Err(err) => {
                    eprintln!("{:?}", err);
                    break;
                }
            }

            _i += 1;
        }
    }

    #[test]
    fn test_heredoc() {
        let program = "cat <<EOF
hello
EOF
echo done
";
        let mut lexer = Lexer::new(program.chars().collect(), SourceId(0));
        let mut _i = 0;
        let _expected = vec![
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Redir,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::Eof,
        ];

        loop {
            match lexer.next_token() {
                Ok(token) => {
                    println!("ShTokenKind::{:?},", token.kind);
                    if token.kind == ShTokenKind::Eof {
                        break;
                    }
                    //assert_eq!(token.kind, expected[i]);
                }
                Err(err) => {
                    eprintln!("{:?}", err);
                    break;
                }
            }
            _i += 1;
        }
    }

    #[test]
    fn test_function_def() {
        let program = "foo() {
  echo hi
}
";
        let mut lexer = Lexer::new(program.chars().collect(), SourceId(0));
        let mut i = 0;
        let expected = vec![
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::LParen,
            ShTokenKind::RParen,
            ShTokenKind::LBrace,
            ShTokenKind::NewLine,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
            ShTokenKind::RBrace,
            ShTokenKind::NewLine,
        ];

        loop {
            match lexer.next_token() {
                Ok(token) => {
                    if token.kind == ShTokenKind::Eof {
                        break;
                    }
                    println!("{:?}", token.text(program));
                    assert_eq!(token.kind, expected[i]);
                }
                Err(err) => {
                    eprintln!("{:?}", err);
                    break;
                }
            }
            i += 1;
        }
    }

    #[test]
    fn test_redir_ops_with_io_number() {
        let program = "2>out 1>>log 0<in 3<>rw 4>|clob 5>&2 6<&1 7>-";
        let expected = vec![
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Redir,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Redir,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Redir,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Redir,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Redir,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Redir,
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Redir,
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Word(WordKind::Word),
            ShTokenKind::Redir,
            ShTokenKind::Word(WordKind::Word),
        ];
        let mut lexer = Lexer::new(program.chars().collect(), SourceId(0));
        let mut i = 0;
        loop {
            match lexer.next_token() {
                Ok(token) => {
                    if token.kind == ShTokenKind::Eof {
                        break;
                    }
                    assert_eq!(token.kind, expected[i]);
                }
                Err(err) => {
                    eprintln!("{:?}", err);
                    break;
                }
            }
            i += 1;
        }
    }

    #[test]
    fn line_continuation_backslash_newline_is_ignored() {
        let program = "echo hello \\\n  | cat\n";
        let expected = vec![
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::Pipe,
            ShTokenKind::Word(WordKind::Name),
            ShTokenKind::NewLine,
        ];
        let mut lexer = Lexer::new(program.chars().collect(), SourceId(0));
        let mut i = 0;
        loop {
            match lexer.next_token() {
                Ok(token) => {
                    if token.kind == ShTokenKind::Eof {
                        break;
                    }
                    assert_eq!(token.kind, expected[i]);
                }
                Err(err) => {
                    eprintln!("{:?}", err);
                    break;
                }
            }
            i += 1;
        }
    }

    #[test]
    fn escaped_quote_inside_double_quoted_word_stays_single_token() {
        let program = r#"jq -r ".assets[] | select(.name == \"$APK_NAME\") | .id""#;
        let tokens: Vec<_> = Lexer::new(program.chars().collect(), SourceId(0))
            .map(|it| it.unwrap())
            .collect();
        let words: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t.kind, ShTokenKind::Word(_)))
            .map(|t| t.text(program).to_string())
            .collect();
        assert_eq!(
            words,
            vec![
                "jq",
                "-r",
                r#"".assets[] | select(.name == \"$APK_NAME\") | .id""#
            ]
        );
        assert!(
            !tokens.iter().any(|t| t.kind == ShTokenKind::Pipe),
            "escaped quote in double quotes must not terminate token early"
        );
    }
}
