use std::{
    cell::{Cell, RefCell},
    sync::LazyLock,
};

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    Word,
    QuotedWord,
    Separator,
    DollWord,
    NewLine,
    Semicolon,
    Andper,
    AndAnd,
    OrOr,
    Bang,
    Pipe,
    IoNumber,
    Less,
    Great,
    Dless,
    Dgreat,
    Lessand,
    Greatand,
    Lessgreat,
    Dlessdash,
    OpenRound,
    CloseRound,
    Andopen,
    LineContinuation,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token<'a> {
    pub t_type: TokenType,
    /// Slice of the token over the original string
    pub value: &'a str,
    /// Offset from the start of the string to the beginning of the token
    pub start: usize,
    /// Equal to the offset + lenght of the token
    pub end: usize,
}

#[derive(Debug)]
pub struct Specification {
    regex: Regex,
    spec_type: TokenType,
}

#[derive(Debug, Clone)]
pub struct UnbufferedTokenizer<'a> {
    input: Cell<&'a str>,
    string_offset: Cell<usize>,
}

#[derive(Debug, Clone)]
struct TokState<'a> {
    buffered_input: &'a [String],
    inner_tokenizer: UnbufferedTokenizer<'a>,
}

#[derive(Debug, Clone)]
pub struct BufTokenizer<'a> {
    state: RefCell<TokState<'a>>,
    prev_state: RefCell<Option<TokState<'a>>>,
}

pub trait Tokenizer<'a>: Iterator<Item = Token<'a>> + Clone {
    fn next_token(&self) -> Option<Token<'a>>;

    fn merge(&self, tok: Self);

    fn peek(&self) -> Option<Token<'a>> {
        self.clone().next_token()
    }

    fn next_is_eof(&self) -> bool {
        match self.peek() {
            Some(token) => token.t_type == TokenType::Eof,
            None => false,
        }
    }

    fn next_if_type(&self, t_type: TokenType) -> Option<Token<'a>> {
        match self.peek() {
            Some(t) if t.t_type == t_type => self.next_token(),
            _ => None,
        }
    }
}

static SPECS: LazyLock<Vec<Specification>> = LazyLock::new(|| {
    vec![
        // Separators
        spec(r"^( +)", TokenType::Separator),
        spec(r"^(\\)\r?\n$", TokenType::LineContinuation),
        // Subshell
        spec(r"^(\()", TokenType::OpenRound),
        spec(r"^(\))", TokenType::CloseRound),
        // Command substitution
        spec(r"^(\$\()", TokenType::Andopen),
        spec(
            r"^(\$((?:[\w\-\/.=]+)|(?:\$)|(?:!)|(?:\?)))",
            TokenType::DollWord,
        ),
        // List separators
        spec(r"^(;)", TokenType::Semicolon),
        spec(r"^(&&)", TokenType::AndAnd),
        spec(r"^(\|\|)", TokenType::OrOr),
        spec(r"^(&)", TokenType::Andper),
        // New line
        spec(r"^(\r?\n)", TokenType::NewLine),
        // IO Number:
        // This number must attached to the redirect operators beginning.
        // Must have greater precedence than number otherwise it will be
        // matched before this one.
        spec(r"^(\d+)(?:<|>)", TokenType::IoNumber),
        // Word kinds
        // Match any normal character + any quoted character
        //
        // For unicode characters read the re2 docs:
        // https://github.com/google/re2/wiki/syntax
        //
        // Unicode character class names:
        // - L: match unicode literal
        // - Nd: match unicode numbers
        // - So: other symbol
        spec(r"^((?:[\p{L}\p{Nd}\p{So}=\-\/.]|\\.)+)", TokenType::Word),
        // Quoatations
        spec(r"^('[^']*')", TokenType::QuotedWord),
        // Sepcification::new(r"^("[^']")", TokenType::string),

        // Bang
        spec(r"^(!)", TokenType::Bang),
        // Redirections
        spec(r"^(\|)", TokenType::Pipe),
        spec(r"^(<>)", TokenType::Lessgreat),
        spec(r"^(<&)", TokenType::Lessand),
        spec(r"^(>&)", TokenType::Greatand),
        spec(r"^(>>)", TokenType::Dgreat),
        spec(r"^(>)", TokenType::Great),
        spec(r"^(<<)", TokenType::Dless),
        spec(r"^(<)", TokenType::Less),
        // EOF
        spec(r"^(\z)", TokenType::Eof),
    ]
});

fn spec(pattern: &str, spec_type: TokenType) -> Specification {
    Specification {
        regex: Regex::new(pattern)
            .unwrap_or_else(|e| panic!("pattern '{}' did not compile! {:#?}", pattern, e)),
        spec_type,
    }
}

impl<'a> Token<'a> {
    pub fn text(&self) -> String {
        match self.t_type {
            TokenType::Word => self.value.chars().filter(|c| *c != '\\').collect(),
            // Remove the quotes
            TokenType::QuotedWord => self.value[1..self.value.len() - 1].to_string(),
            _ => self.value.to_string(),
        }
    }
}

impl<'a> UnbufferedTokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input: Cell::new(input),
            string_offset: Cell::new(0),
        }
    }
}

impl<'a> Tokenizer<'a> for UnbufferedTokenizer<'a> {
    fn next_token(&self) -> Option<Token<'a>> {
        for spec in SPECS.iter() {
            let Some(captures) = spec.regex.captures(self.input.get()) else {
                continue;
            };

            // let (_, [match_group]) = captures.extract();
            let match_group = captures.get(1).unwrap().as_str();

            let token = Token {
                t_type: spec.spec_type,
                value: match_group,
                start: self.string_offset.get(),
                end: self.string_offset.get() + match_group.len(),
            };

            self.input.update(|input| &input[match_group.len()..]);
            self.string_offset
                .update(|offset| offset + match_group.len());

            if token.t_type == TokenType::Separator {
                continue;
            }

            return Some(token);
        }

        None
    }

    fn merge(&self, tok: Self) {
        self.string_offset.set(tok.string_offset.get());
        self.input.set(tok.input.get());
    }
}

impl<'a> Iterator for UnbufferedTokenizer<'a> {
    type Item = Token<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.next_token()
    }
}

impl<'a> BufTokenizer<'a> {
    fn new(state: TokState<'a>) -> Self {
        Self {
            state: RefCell::new(state),
            prev_state: RefCell::new(None),
        }
    }

    pub fn from_buffer(buffer: &'a [String]) -> Self {
        Self {
            state: RefCell::new(TokState {
                buffered_input: buffer,
                inner_tokenizer: UnbufferedTokenizer {
                    input: Cell::new(""),
                    string_offset: Cell::new(0),
                },
            }),
            prev_state: RefCell::new(None),
        }
    }

    /// Advances the `buffered_input` and stores the leftover string
    /// into a new [UnbufferedTokenizer]. The previous tokenizer must
    /// be exausted before calling this method.
    ///
    /// Returns `false` if the buffer was empty.
    fn advance_buffer(&self) -> bool {
        let mut state = self.state.borrow_mut();
        if state.buffered_input.is_empty() {
            return false;
        }

        assert!(
            matches!(
                state.inner_tokenizer.peek(),
                Some(Token {
                    t_type: TokenType::Eof,
                    ..
                })
            ),
            "The inner tokenizer must be empty before advancing the line buffer!"
        );

        state.inner_tokenizer = UnbufferedTokenizer::new(&state.buffered_input[0]);
        state.buffered_input = &state.buffered_input[1..];

        true
    }

    pub fn prev(&self) -> Option<BufTokenizer<'a>> {
        let state_ref = self.prev_state.borrow();
        let state = state_ref.as_ref()?;

        Some(BufTokenizer::new(state.clone()))
    }
}

impl<'a> Tokenizer<'a> for BufTokenizer<'a> {
    fn next_token(&self) -> Option<Token<'a>> {
        *self.prev_state.borrow_mut() = Some(self.state.borrow().clone());

        let mut token = self.state.borrow().inner_tokenizer.next_token();

        while let Some(t) = &token
            && t.t_type == TokenType::Eof
        {
            if !self.advance_buffer() {
                break;
            }

            token = self.state.borrow().inner_tokenizer.next_token();
        }

        token
    }

    fn merge(&self, tok: Self) {
        self.state.swap(&tok.state);
        self.prev_state.swap(&tok.prev_state);
    }
}

impl<'a> Iterator for BufTokenizer<'a> {
    type Item = Token<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.next_token()
    }
}
