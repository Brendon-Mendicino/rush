use std::{
    cell::{Cell, RefCell},
    sync::LazyLock,
};

use regex::Regex;

/// The kinds of tokens. Most are the operators of POSIX §2.3/§2.10.1; a word is
/// not one token but a run of glued parts (`Literal`, the quotes and their text,
/// `Param`, `Andopen`…`CloseRound`, `Backquoted`), which the parser joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    /// Unquoted text of a word: `\x` escapes `x`.
    Literal,
    /// Text between single quotes, taken as it is.
    SqText,
    /// Text between double quotes: `\` escapes only `$` `` ` `` `"` `\` and
    /// newline.
    DqText,
    /// `$name`, `$1`, `$?`...: a parameter without braces, complete in itself.
    Param,
    /// `${`: a parameter expansion, which the parser reads up to its `}`
    /// (`ParamName`, then `ParamOp` and a word): the braces can hold quotes,
    /// `$(...)` and other `${...}`.
    ParamOpen,
    /// `#` right after `${`, before a parameter: its length (`${#x}`).
    ParamHash,
    /// The parameter inside `${...}`: a name, digits, or a special parameter.
    ParamName,
    /// The operator after the parameter: `:-` `-` `:=` `=` `:?` `?` `:+` `+`
    /// `%` `%%` `#` `##`.
    ParamOp,
    /// `}`, closing a `${`.
    CloseBrace,
    /// `'`, opening or closing a single-quoted part.
    SQuote,
    /// `"`, opening or closing a double-quoted part.
    DQuote,
    /// `` `...` ``: a command substitution, the old way.
    Backquoted,
    NewLine,
    Semicolon,
    Dsemi,
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
    Clobber,
    OpenRound,
    CloseRound,
    Andopen,
    LineContinuation,
    Eof,
}

/// Which token rules apply: inside quotes most characters lose their meaning,
/// and inside `${...}` the parser asks for each piece in turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    SingleQuoted,
    DoubleQuoted,
    /// Right after `${`: `ParamHash`, `ParamName`, or `CloseBrace`.
    ParamName,
    /// After the parameter of a `${`: `ParamOp` or `CloseBrace`.
    ParamOp,
    /// The word after the operator, outside double quotes, up to the `}`: like
    /// `Normal`, but blanks are part of it and only `}` ends it.
    ParamWord,
    /// The same inside double quotes (`"${x:-...}"`): `'` is an ordinary
    /// character there, and `\` escapes like between double quotes.
    ParamWordDq,
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
    /// Right after the previous token, with no blank between them on the same
    /// line: the parts of one word are glued (`a'b'$(c)`).
    pub glued: bool,
}

#[derive(Debug)]
pub struct Specification {
    regex: Regex,
    spec_type: TokenType,
}

#[derive(Debug, Clone)]
pub struct UnbufferedTokenizer<'a> {
    /// The whole line, to look behind the current token (comments).
    line: &'a str,
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
    /// The next token, with the rules of `mode`.
    fn next_token_in(&self, mode: Mode) -> Option<Token<'a>>;

    fn merge(&self, tok: Self);

    fn next_token(&self) -> Option<Token<'a>> {
        self.next_token_in(Mode::Normal)
    }

    fn peek_in(&self, mode: Mode) -> Option<Token<'a>> {
        self.clone().next_token_in(mode)
    }

    fn peek(&self) -> Option<Token<'a>> {
        self.peek_in(Mode::Normal)
    }

    fn next_is_eof(&self) -> bool {
        match self.peek() {
            Some(token) => token.t_type == TokenType::Eof,
            None => false,
        }
    }

    fn next_if_type_in(&self, mode: Mode, t_type: TokenType) -> Option<Token<'a>> {
        match self.peek_in(mode) {
            Some(t) if t.t_type == t_type => self.next_token_in(mode),
            _ => None,
        }
    }

    fn next_if_type(&self, t_type: TokenType) -> Option<Token<'a>> {
        self.next_if_type_in(Mode::Normal, t_type)
    }
}

/// A variable name: the longest one, with Unicode letters like
/// `syntax::is_valid_name`.
const NAME: &str = r"[\p{Alphabetic}_][\p{Alphabetic}\p{N}_]*";

/// The special parameters but `0` (§2.5.2).
const SPECIAL: &str = r"[@*#?$!\-]";

/// `$name`, `$` and one digit, or `$` and a special parameter. With braces it
/// is a `ParamOpen`, which the parser reads.
static PARAM: LazyLock<String> = LazyLock::new(|| format!(r"^(\$(?:{NAME}|[0-9]|{SPECIAL}))"));

/// `` `...` `` with its backslash escapes, on one line.
const BACKQUOTED: &str = r"^(`(?:[^`\\]|\\.)*`)";

/// Outside quotes (POSIX §2.3). Blanks are skipped, and so are comments, which
/// `next_token_in` handles before these.
static NORMAL: LazyLock<Vec<Specification>> = LazyLock::new(|| {
    vec![
        spec(r"^(\\)\r?\n$", TokenType::LineContinuation),
        // Operators, the longest first.
        spec(r"^(&&)", TokenType::AndAnd),
        spec(r"^(\|\|)", TokenType::OrOr),
        spec(r"^(;;)", TokenType::Dsemi),
        spec(r"^(;)", TokenType::Semicolon),
        spec(r"^(&)", TokenType::Andper),
        spec(r"^(\|)", TokenType::Pipe),
        spec(r"^(\()", TokenType::OpenRound),
        spec(r"^(\))", TokenType::CloseRound),
        spec(r"^(<<-)", TokenType::Dlessdash),
        spec(r"^(<<)", TokenType::Dless),
        spec(r"^(<>)", TokenType::Lessgreat),
        spec(r"^(<&)", TokenType::Lessand),
        spec(r"^(<)", TokenType::Less),
        spec(r"^(>>)", TokenType::Dgreat),
        spec(r"^(>&)", TokenType::Greatand),
        spec(r"^(>\|)", TokenType::Clobber),
        spec(r"^(>)", TokenType::Great),
        spec(r"^(\r?\n)", TokenType::NewLine),
        // Digits right before `<` or `>`: the fd of a redirection.
        spec(r"^(\d+)(?:<|>)", TokenType::IoNumber),
        // `!` is a reserved word only when it is a word of its own.
        spec(r"^(!)(?:[ \t\r\n]|\z)", TokenType::Bang),
        // Word parts.
        spec(r"^(\$\()", TokenType::Andopen),
        spec(r"^(\$\{)", TokenType::ParamOpen),
        spec(&PARAM, TokenType::Param),
        spec(r"^(')", TokenType::SQuote),
        spec(r#"^(")"#, TokenType::DQuote),
        spec(BACKQUOTED, TokenType::Backquoted),
        // Any character but a blank, a newline, an operator or a quoting
        // character, or one escaped by `\`.
        spec(
            r#"^((?:[^ \t\r\n|&;<>()$`'"\\]|\\[\s\S])+)"#,
            TokenType::Literal,
        ),
        // A `$` that starts nothing, a `\` at the very end.
        spec(r"^(\$)", TokenType::Literal),
        spec(r"^(\\)", TokenType::Literal),
        spec(r"^(\z)", TokenType::Eof),
    ]
});

/// Between single quotes: everything up to the next `'`, newlines included.
static SINGLE_QUOTED: LazyLock<Vec<Specification>> = LazyLock::new(|| {
    vec![
        spec(r"^(')", TokenType::SQuote),
        spec(r"^([^']+)", TokenType::SqText),
        spec(r"^(\z)", TokenType::Eof),
    ]
});

/// Between double quotes: `$` and `` ` `` keep their meaning, `\` escapes only
/// some characters.
static DOUBLE_QUOTED: LazyLock<Vec<Specification>> = LazyLock::new(|| {
    vec![
        spec(r#"^(")"#, TokenType::DQuote),
        spec(r"^(\$\()", TokenType::Andopen),
        spec(r"^(\$\{)", TokenType::ParamOpen),
        spec(&PARAM, TokenType::Param),
        spec(BACKQUOTED, TokenType::Backquoted),
        spec(r#"^((?:[^"\\$`]|\\[\s\S])+)"#, TokenType::DqText),
        spec(r"^(\$|\\)", TokenType::DqText),
        spec(r"^(\z)", TokenType::Eof),
    ]
});

/// Right after `${`. A `#` before the start of a parameter is the length
/// (`${#x}`, `${##}`); otherwise it is the parameter `#` (`${#}`).
static PARAM_NAME: LazyLock<Vec<Specification>> = LazyLock::new(|| {
    vec![
        spec(
            &format!(r"^(#)(?:{NAME}|[0-9]|{SPECIAL})"),
            TokenType::ParamHash,
        ),
        spec(
            &format!(r"^({NAME}|[0-9]+|{SPECIAL})"),
            TokenType::ParamName,
        ),
        spec(r"^(\})", TokenType::CloseBrace),
        spec(r"^(\z)", TokenType::Eof),
    ]
});

/// After the parameter of a `${`: an operator, the longest first, or the end.
static PARAM_OP: LazyLock<Vec<Specification>> = LazyLock::new(|| {
    vec![
        spec(r"^(:[-=?+]|[-=?+]|%%|%|##|#)", TokenType::ParamOp),
        spec(r"^(\})", TokenType::CloseBrace),
        spec(r"^(\z)", TokenType::Eof),
    ]
});

/// The word after the operator of a `${`, outside double quotes: quotes,
/// parameters and command substitutions as in `Normal`, every other character
/// (blanks too) is text, `}` ends it.
static PARAM_WORD: LazyLock<Vec<Specification>> = LazyLock::new(|| {
    vec![
        spec(r"^(\})", TokenType::CloseBrace),
        spec(r"^(\$\()", TokenType::Andopen),
        spec(r"^(\$\{)", TokenType::ParamOpen),
        spec(&PARAM, TokenType::Param),
        spec(r"^(')", TokenType::SQuote),
        spec(r#"^(")"#, TokenType::DQuote),
        spec(BACKQUOTED, TokenType::Backquoted),
        spec(r#"^((?:[^}$`'"\\]|\\[\s\S])+)"#, TokenType::Literal),
        spec(r"^(\$|\\)", TokenType::Literal),
        spec(r"^(\z)", TokenType::Eof),
    ]
});

/// The same inside double quotes: `'` is text, `"` opens a nested quoted part.
// TODO: `\}` keeps its backslash here (`DqText` escapes), bash drops it.
static PARAM_WORD_DQ: LazyLock<Vec<Specification>> = LazyLock::new(|| {
    vec![
        spec(r"^(\})", TokenType::CloseBrace),
        spec(r"^(\$\()", TokenType::Andopen),
        spec(r"^(\$\{)", TokenType::ParamOpen),
        spec(&PARAM, TokenType::Param),
        spec(r#"^(")"#, TokenType::DQuote),
        spec(BACKQUOTED, TokenType::Backquoted),
        spec(r#"^((?:[^}$`"\\]|\\[\s\S])+)"#, TokenType::DqText),
        spec(r"^(\$|\\)", TokenType::DqText),
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
    /// The text of the token after quote removal (§2.2).
    pub fn text(&self) -> String {
        match self.t_type {
            TokenType::Literal => unescape(self.value, |_| true),
            TokenType::DqText => {
                unescape(self.value, |c| matches!(c, '$' | '`' | '"' | '\\' | '\n'))
            }
            _ => self.value.to_string(),
        }
    }
}

/// Remove the backslashes that escape a character `escapes` accepts; a
/// backslash before a newline goes away with the newline (line continuation).
fn unescape(value: &str, escapes: impl Fn(char) -> bool) -> String {
    let mut text = String::with_capacity(value.len());
    let mut chars = value.chars();

    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }

        match chars.clone().next() {
            Some(next) if escapes(next) => {
                chars.next();
                if next != '\n' {
                    text.push(next);
                }
            }
            _ => text.push(c),
        }
    }

    text
}

impl<'a> UnbufferedTokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            line: input,
            input: Cell::new(input),
            string_offset: Cell::new(0),
        }
    }

    /// A `#` here starts a comment: it is at the start of a token, not inside a
    /// word (`a#b`).
    fn at_token_start(&self) -> bool {
        let offset = self.string_offset.get();
        self.line[..offset].chars().next_back().is_none_or(|c| {
            matches!(
                c,
                ' ' | '\t' | '\r' | '\n' | ';' | '&' | '|' | '(' | ')' | '<' | '>'
            )
        })
    }

    /// Skip the blanks and the comments before the next token. Returns whether
    /// there was any.
    fn skip_blanks(&self) -> bool {
        let input = self.input.get();
        let mut skip = input.len() - input.trim_start_matches([' ', '\t']).len();

        if input[skip..].starts_with('#') && (skip > 0 || self.at_token_start()) {
            let rest = &input[skip..];
            skip += rest.find(['\r', '\n']).unwrap_or(rest.len());
        }

        self.advance(skip);
        skip > 0
    }

    fn advance(&self, len: usize) {
        self.input.update(|input| &input[len..]);
        self.string_offset.update(|offset| offset + len);
    }
}

impl<'a> Tokenizer<'a> for UnbufferedTokenizer<'a> {
    fn next_token_in(&self, mode: Mode) -> Option<Token<'a>> {
        let specs = match mode {
            Mode::Normal => &NORMAL,
            Mode::SingleQuoted => &SINGLE_QUOTED,
            Mode::DoubleQuoted => &DOUBLE_QUOTED,
            Mode::ParamName => &PARAM_NAME,
            Mode::ParamOp => &PARAM_OP,
            Mode::ParamWord => &PARAM_WORD,
            Mode::ParamWordDq => &PARAM_WORD_DQ,
        };

        let blank = mode == Mode::Normal && self.skip_blanks();
        let glued = !blank && self.string_offset.get() > 0;

        for spec in specs.iter() {
            let Some(captures) = spec.regex.captures(self.input.get()) else {
                continue;
            };

            let match_group = captures.get(1).unwrap().as_str();

            let token = Token {
                t_type: spec.spec_type,
                value: match_group,
                start: self.string_offset.get(),
                end: self.string_offset.get() + match_group.len(),
                glued,
            };

            self.advance(match_group.len());

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
                inner_tokenizer: UnbufferedTokenizer::new(""),
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
    /// At the end of a line the next one is read, in any mode: a quote can span
    /// lines.
    fn next_token_in(&self, mode: Mode) -> Option<Token<'a>> {
        *self.prev_state.borrow_mut() = Some(self.state.borrow().clone());

        let mut token = self.state.borrow().inner_tokenizer.next_token_in(mode);

        while let Some(t) = &token
            && t.t_type == TokenType::Eof
        {
            if !self.advance_buffer() {
                break;
            }

            token = self.state.borrow().inner_tokenizer.next_token_in(mode);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The tokens of `input` outside quotes, up to the end.
    fn tokens(input: &str) -> Vec<(TokenType, &str)> {
        let tok = UnbufferedTokenizer::new(input);
        let mut tokens = Vec::new();
        while let Some(token) = tok.next_token() {
            if token.t_type == TokenType::Eof {
                break;
            }
            tokens.push((token.t_type, token.value));
        }
        tokens
    }

    #[test]
    fn blanks_are_spaces_and_tabs() {
        use TokenType::*;
        assert_eq!(tokens("a \t b"), [(Literal, "a"), (Literal, "b")]);
    }

    #[test]
    fn operators_are_the_longest_match() {
        use TokenType::*;
        assert_eq!(
            tokens("a;;b;c&&d&e||f|g"),
            [
                (Literal, "a"),
                (Dsemi, ";;"),
                (Literal, "b"),
                (Semicolon, ";"),
                (Literal, "c"),
                (AndAnd, "&&"),
                (Literal, "d"),
                (Andper, "&"),
                (Literal, "e"),
                (OrOr, "||"),
                (Literal, "f"),
                (Pipe, "|"),
                (Literal, "g"),
            ]
        );
        assert_eq!(
            tokens("<<- << <> <& < >> >& >| >"),
            [
                (Dlessdash, "<<-"),
                (Dless, "<<"),
                (Lessgreat, "<>"),
                (Lessand, "<&"),
                (Less, "<"),
                (Dgreat, ">>"),
                (Greatand, ">&"),
                (Clobber, ">|"),
                (Great, ">"),
            ]
        );
    }

    #[test]
    fn a_comment_starts_only_a_token() {
        use TokenType::*;
        assert_eq!(tokens("a # b c"), [(Literal, "a")]);
        assert_eq!(tokens("#x\n"), [(NewLine, "\n")]);
        assert_eq!(tokens("a;#b"), [(Literal, "a"), (Semicolon, ";")]);
        assert_eq!(tokens("a#b"), [(Literal, "a#b")]);
        // After a quote or a parameter the `#` is still inside the word.
        assert_eq!(
            tokens("$x#b \"\"#c"),
            [
                (Param, "$x"),
                (Literal, "#b"),
                (DQuote, "\""),
                (DQuote, "\""),
                (Literal, "#c")
            ]
        );
    }

    #[test]
    fn a_param_ends_with_its_name() {
        use TokenType::*;
        assert_eq!(tokens("$HOME/x"), [(Param, "$HOME"), (Literal, "/x")]);
        assert_eq!(tokens("$1x"), [(Param, "$1"), (Literal, "x")]);
        assert_eq!(tokens("$?$$"), [(Param, "$?"), (Param, "$$")]);
        assert_eq!(tokens("$città."), [(Param, "$città"), (Literal, ".")]);
        assert_eq!(tokens("a$"), [(Literal, "a"), (Literal, "$")]);
        // With braces only the opening is a token: the parser reads the rest.
        assert_eq!(tokens("${a}b"), [(ParamOpen, "${"), (Literal, "a}b")]);
    }

    /// The tokens of `input` in `mode`, up to the end of the input.
    fn tokens_in(mode: Mode, input: &str) -> Vec<(TokenType, &str)> {
        let tok = UnbufferedTokenizer::new(input);
        std::iter::from_fn(|| tok.next_token_in(mode))
            .take_while(|token| token.t_type != TokenType::Eof)
            .map(|token| (token.t_type, token.value))
            .collect()
    }

    #[test]
    fn inside_braces_the_parser_picks_each_piece() {
        use TokenType::*;

        assert_eq!(
            tokens_in(Mode::DoubleQuoted, "${x"),
            [(ParamOpen, "${"), (DqText, "x")]
        );

        // The name: a `#` before a parameter is the length.
        assert_eq!(
            tokens_in(Mode::ParamName, "#x"),
            [(ParamHash, "#"), (ParamName, "x")]
        );
        assert_eq!(
            tokens_in(Mode::ParamName, "#}"),
            [(ParamName, "#"), (CloseBrace, "}")]
        );
        assert_eq!(
            tokens_in(Mode::ParamName, "##"),
            [(ParamHash, "#"), (ParamName, "#")]
        );
        assert_eq!(
            tokens_in(Mode::ParamName, "10}"),
            [(ParamName, "10"), (CloseBrace, "}")]
        );
        assert_eq!(tokens_in(Mode::ParamName, "città"), [(ParamName, "città")]);

        // The operator, the longest first.
        for op in [
            ":-", ":=", ":?", ":+", "-", "=", "?", "+", "%%", "%", "##", "#",
        ] {
            assert_eq!(tokens_in(Mode::ParamOp, op), [(ParamOp, op)], "{op}");
        }
        assert_eq!(tokens_in(Mode::ParamOp, "}"), [(CloseBrace, "}")]);
        assert!(tokens_in(Mode::ParamOp, " b").is_empty(), "no operator");
    }

    #[test]
    fn the_word_of_a_brace_keeps_blanks_and_ends_at_the_brace() {
        use TokenType::*;

        assert_eq!(
            tokens_in(Mode::ParamWord, "a b\\}c}"),
            [(Literal, "a b\\}c"), (CloseBrace, "}")]
        );
        assert_eq!(
            tokens_in(Mode::ParamWord, "'}'$x${"),
            [
                (SQuote, "'"),
                (CloseBrace, "}"),
                (SQuote, "'"),
                (Param, "$x"),
                (ParamOpen, "${")
            ]
        );
        // Inside double quotes a `'` is text.
        assert_eq!(
            tokens_in(Mode::ParamWordDq, "'a'\"}"),
            [(DqText, "'a'"), (DQuote, "\""), (CloseBrace, "}")]
        );
    }

    #[test]
    fn bang_only_as_a_word_of_its_own() {
        use TokenType::*;
        assert_eq!(tokens("! a"), [(Bang, "!"), (Literal, "a")]);
        assert_eq!(tokens("!a"), [(Literal, "!a")]);
    }

    #[test]
    fn words_take_every_other_character() {
        use TokenType::*;
        assert_eq!(tokens("*?[x]~{}€=@%"), [(Literal, "*?[x]~{}€=@%")]);
        assert_eq!(tokens(r"a\ b\;c"), [(Literal, r"a\ b\;c")]);
    }

    #[test]
    fn glued_parts() {
        let tok = UnbufferedTokenizer::new("a'b' c$(d)");
        let glued: Vec<_> = std::iter::from_fn(|| {
            let token = tok.next_token()?;
            if token.t_type == TokenType::SQuote {
                // The quoted text is read in its own mode.
                tok.next_token_in(Mode::SingleQuoted);
                tok.next_token_in(Mode::SingleQuoted);
            }
            (token.t_type != TokenType::Eof).then_some((token.value, token.glued))
        })
        .collect();

        assert_eq!(
            glued,
            [
                ("a", false),
                ("'", true),
                ("c", false),
                ("$(", true),
                ("d", true),
                (")", true)
            ]
        );
    }

    #[test]
    fn quoted_modes() {
        let tok = UnbufferedTokenizer::new("'a \" $b'\"c $d `e` \\\" \\x\"");
        assert_eq!(tok.next_token().unwrap().t_type, TokenType::SQuote);
        let sq = tok.next_token_in(Mode::SingleQuoted).unwrap();
        assert_eq!(
            (sq.t_type, sq.text().as_str()),
            (TokenType::SqText, "a \" $b")
        );
        assert_eq!(
            tok.next_token_in(Mode::SingleQuoted).unwrap().t_type,
            TokenType::SQuote
        );

        assert_eq!(tok.next_token().unwrap().t_type, TokenType::DQuote);
        let parts: Vec<_> = std::iter::from_fn(|| tok.next_token_in(Mode::DoubleQuoted))
            .take_while(|token| token.t_type != TokenType::DQuote)
            .map(|token| (token.t_type, token.text()))
            .collect();
        assert_eq!(
            parts,
            [
                (TokenType::DqText, "c ".to_string()),
                (TokenType::Param, "$d".to_string()),
                (TokenType::DqText, " ".to_string()),
                (TokenType::Backquoted, "`e`".to_string()),
                (TokenType::DqText, " \" \\x".to_string()),
            ]
        );
    }

    #[test]
    fn a_quote_spans_buffered_lines() {
        let lines = vec!["'a\n".to_string(), "b'\n".to_string()];
        let tok = BufTokenizer::from_buffer(&lines);

        assert_eq!(tok.next_token().unwrap().t_type, TokenType::SQuote);
        assert_eq!(tok.next_token_in(Mode::SingleQuoted).unwrap().value, "a\n");
        assert_eq!(tok.next_token_in(Mode::SingleQuoted).unwrap().value, "b");
        assert_eq!(
            tok.next_token_in(Mode::SingleQuoted).unwrap().t_type,
            TokenType::SQuote
        );
    }

    #[test]
    fn escapes_depend_on_the_quoting() {
        let token = |t_type, value| Token {
            t_type,
            value,
            start: 0,
            end: 0,
            glued: false,
        };

        assert_eq!(token(TokenType::Literal, r"a\\b\ c\$").text(), r"a\b c$");
        assert_eq!(token(TokenType::Literal, "a\\\nb").text(), "ab");
        assert_eq!(
            token(TokenType::DqText, r#"\$\`\"\\\a"#).text(),
            r#"$`"\\a"#
        );
        assert_eq!(token(TokenType::SqText, r"\a\\").text(), r"\a\\");
    }
}
