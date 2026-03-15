use crate::parser::source_map::SourceId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub index: usize,
    pub source_id: SourceId,
    pub len: usize,
}

impl Span {
    pub fn new(index: usize, source_id: SourceId, len: usize) -> Span {
        Span {
            index,
            source_id,
            len,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShToken {
    pub kind: ShTokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShTokenKind {
    Eof,
    NewLine,
    SemiColon,
    BackgroundExec,
    And,
    Or,
    Pipe,
    Eq,
    Redir,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comment,
    Word(WordKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordKind {
    Word,
    Name,
    Path,
}

impl ShToken {
    pub fn new(kind: ShTokenKind, span: Span) -> ShToken {
        ShToken { kind, span }
    }

    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        let start = char_index_to_byte_offset(src, self.span.index);
        let end = char_index_to_byte_offset(src, self.span.index + self.span.len);
        &src[start..end]
    }
}

fn char_index_to_byte_offset(src: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    src.char_indices()
        .nth(char_index)
        .map(|(i, _)| i)
        .unwrap_or(src.len())
}
