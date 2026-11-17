use std::{fmt, vec};

use crate::tokenizer::{Mode, Token, TokenType, Tokenizer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenKind {
    Read,
    Replace,
    Append,
    Rw,
}

/// `name=value` before a command (rule 7b): the name, and the value, a word
/// of its own (`X=$(cmd)`, `X=a'b c'`).
#[derive(Debug, Clone)]
pub struct AssignmentWord<'a> {
    pub key: &'a str,
    pub value: Word<'a>,
}

#[derive(Debug, Clone)]
pub struct FileRedirect<'a> {
    pub redirect_fd: i32,
    pub file_kind: OpenKind,
    /// Expanded when the command runs (rule 2).
    pub filename: Word<'a>,
}

/// `n<&word` / `n>&word`: duplicate the fd `word` expands to, or close `n` when
/// it is `-`. Resolved when the command runs, after the expansion.
#[derive(Debug, Clone)]
pub struct DupRedirect<'a> {
    pub fd: i32,
    /// `>&` (the default fd is 1) rather than `<&` (0).
    pub output: bool,
    pub target: Word<'a>,
}

#[derive(Debug, Clone)]
pub enum Redirect<'a> {
    FileRedirect(FileRedirect<'a>),
    Dup(DupRedirect<'a>),
}

/// One piece of a word (§2.2, §2.6). The pieces are glued, without blanks:
/// `a'b c'"$x"$(d)` is one word of four parts.
#[derive(Debug, Clone)]
pub enum WordPart<'a> {
    /// Unquoted text (`TokenType::Literal`, or the digits of a glued
    /// `IoNumber`).
    Literal(Token<'a>),
    /// The text between single quotes (`TokenType::SqText`): none for `''`,
    /// one token per line when it spans lines.
    SingleQuoted(Vec<Token<'a>>),
    /// Between double quotes: `DqText`, `Param`, `CmdSub` and `Backquoted`.
    DoubleQuoted(Vec<WordPart<'a>>),
    /// `$name`, `$1`, `$?`, `${name}`, `${x:-word}`...: parsed with the word,
    /// so a `${...}` that isn't one is a syntax error, also where it would
    /// never run.
    Param(ParamExpansion<'a>),
    CmdSub(CmdSub<'a>),
    /// `` `...` ``, the token with its backquotes.
    Backquoted(Token<'a>),
}

/// A parameter (§2.5): what `$name`, `$1`, `${10}`, `$?`... name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parameter<'a> {
    /// A variable.
    Name(&'a str),
    /// `$1`, `${10}`: from 1.
    Positional(usize),
    Special(Special),
}

/// The special parameters (§2.5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Special {
    /// `@`: the positional parameters, a field each.
    At,
    /// `*`: the positional parameters, joined.
    Star,
    /// `#`: how many positional parameters.
    Hash,
    /// `?`: the status of the most recent pipeline.
    Question,
    /// `-`: the option flags.
    Hyphen,
    /// `$`: the pid of the shell.
    Dollar,
    /// `!`: the pid of the most recent background command.
    Bang,
    /// `0`: the name of the shell or of the script.
    Zero,
}

/// A parameter expansion (§2.6.2): `$x`, or `${...}` with what is inside.
#[derive(Debug, Clone)]
pub struct ParamExpansion<'a> {
    pub param: Parameter<'a>,
    /// Written `${x}` rather than `$x`, for printing it back.
    pub braced: bool,
    /// What to do with the value; `None` is the value itself.
    pub op: Option<ParamOp<'a>>,
}

/// The operations of `${...}` (§2.6.2). `colon` (`${x:-w}` rather than
/// `${x-w}`): a null value counts as unset. The words are expanded only when
/// they are used.
#[derive(Debug, Clone)]
pub enum ParamOp<'a> {
    /// `${#x}`: the length of the value.
    Length,
    /// `${x-word}`: `word` when `x` is unset.
    Default { colon: bool, word: Word<'a> },
    /// `${x=word}`: the same, and `x` is assigned `word`.
    Assign { colon: bool, word: Word<'a> },
    /// `${x?word}`: an error, with `word` as its message, when `x` is unset.
    Error { colon: bool, word: Word<'a> },
    /// `${x+word}`: `word` when `x` is set, nothing otherwise.
    Alternative { colon: bool, word: Word<'a> },
    /// `${x%pattern}`, `${x%%pattern}`: the value without the smallest, or the
    /// largest, suffix the pattern matches.
    RemoveSuffix { largest: bool, pattern: Word<'a> },
    /// `${x#pattern}`, `${x##pattern}`: the same with a prefix.
    RemovePrefix { largest: bool, pattern: Word<'a> },
}

/// A word: its parts. At least one, except the word of a `${x:-}`.
#[derive(Debug, Clone)]
pub struct Word<'a> {
    pub parts: Vec<WordPart<'a>>,
}

#[derive(Debug, Clone)]
pub struct SimpleAssignment<'a> {
    pub redirections: Vec<Redirect<'a>>,
    pub envs: Vec<AssignmentWord<'a>>,
}

#[derive(Debug, Clone)]
pub struct UnsubCommand<'a> {
    pub program: Word<'a>,
    pub arguments: Vec<Word<'a>>,
    pub redirections: Vec<Redirect<'a>>,
    pub envs: Vec<AssignmentWord<'a>>,
}

#[derive(Debug, Clone)]
pub enum Command<'a> {
    SimpleAssignment(SimpleAssignment<'a>),
    UnsubCommand(UnsubCommand<'a>),
    /// `compound_command redirect_list`: the redirections apply to the whole
    /// compound command.
    Compound {
        command: CompoundCommand<'a>,
        redirections: Vec<Redirect<'a>>,
    },
}

/// The compound commands parsed so far (§2.9.4).
#[derive(Debug, Clone)]
pub enum CompoundCommand<'a> {
    Subshell(Subshell<'a>),
    For(ForClause<'a>),
    Case(CaseClause<'a>),
    If(IfClause<'a>),
    While(WhileClause<'a>),
    Until(UntilClause<'a>),
}

#[derive(Debug, Clone)]
pub enum OpList<'a> {
    AndList(AndList<'a>),
    OrList(OrList<'a>),
    Pipeline(Pipeline<'a>),
}

#[derive(Debug, Clone)]
pub enum List<'a> {
    SequentialList(SequentialList<'a>),
    AsyncList(AsyncList<'a>),
}

#[derive(Debug, Clone)]
pub struct AndList<'a> {
    pub left: Box<OpList<'a>>,
    pub right: Box<OpList<'a>>,
}

#[derive(Debug, Clone)]
pub struct OrList<'a> {
    pub left: Box<OpList<'a>>,
    pub right: Box<OpList<'a>>,
}

#[derive(Debug, Clone, Default)]
pub struct Pipeline<'a> {
    pub cmds: Vec<Command<'a>>,
    pub negated: bool,
}

#[derive(Debug, Clone)]
pub struct SequentialList<'a> {
    pub left: Option<Box<List<'a>>>,
    pub right: Box<OpList<'a>>,
}

#[derive(Debug, Clone)]
pub struct AsyncList<'a> {
    pub left: Option<Box<List<'a>>>,
    pub right: Box<OpList<'a>>,
}

#[derive(Debug, Clone)]
pub struct Subshell<'a> {
    pub open_round: Token<'a>,
    pub close_round: Token<'a>,
    pub seq_list: Box<List<'a>>,
}

#[derive(Debug, Clone)]
pub struct IfClause<'a> {
    pub condition: List<'a>,
    pub body: List<'a>,
    pub else_part: Option<Box<ElsePart<'a>>>,
}

#[derive(Debug, Clone)]
pub struct ElifClause<'a> {
    pub condition: List<'a>,
    pub body: List<'a>,
    pub else_part: Option<Box<ElsePart<'a>>>,
}

#[derive(Debug, Clone)]
pub struct ElseClause<'a> {
    pub body: List<'a>,
}

#[derive(Debug, Clone)]
pub enum ElsePart<'a> {
    Elif(ElifClause<'a>),
    Else(ElseClause<'a>),
}

/// `for name [in word...]; do body; done` (§2.9.4.2).
#[derive(Debug, Clone)]
pub struct ForClause<'a> {
    /// The loop variable: a valid name (rule 5).
    pub name: &'a str,
    /// The words after `in`. `None` without `in`: the loop goes over the
    /// positional parameters (`"$@"`). Empty with `in` and no word
    /// (`for x in; do`): the body never runs.
    pub wordlist: Option<Vec<Word<'a>>>,
    pub body: List<'a>,
}

/// `case word in pattern|pattern) list;; ... esac` (§2.9.4.3).
#[derive(Debug, Clone)]
pub struct CaseClause<'a> {
    pub word: Word<'a>,
    pub items: Vec<CaseItem<'a>>,
}

/// `(pattern|pattern) list ;;`: the `(` and the `;;` of the last item are
/// optional, and not kept.
#[derive(Debug, Clone)]
pub struct CaseItem<'a> {
    /// At least one.
    pub patterns: Vec<Word<'a>>,
    /// `None` for an empty body: `a) ;;`.
    pub body: Option<List<'a>>,
}

/// `while condition; do body; done` (§2.9.4.4).
#[derive(Debug, Clone)]
pub struct WhileClause<'a> {
    pub condition: List<'a>,
    pub body: List<'a>,
}

/// `until condition; do body; done` (§2.9.4.5).
#[derive(Debug, Clone)]
pub struct UntilClause<'a> {
    pub condition: List<'a>,
    pub body: List<'a>,
}

#[derive(Debug, Clone, Default)]
pub struct CompleteCommands<'a> {
    pub commands: Vec<List<'a>>,
}

#[derive(Debug, Clone, Default)]
pub struct ThisProgram<'a> {
    pub child: CompleteCommands<'a>,
}

// ------------------------------------
// Command
// ------------------------------------

#[derive(Debug, Clone)]
pub enum CmdSuf<'a> {
    Word(Word<'a>),
    Redirect(Redirect<'a>),
}

#[derive(Debug, Clone)]
pub enum CmdPre<'a> {
    AssignmentWord(AssignmentWord<'a>),
    Redirect(Redirect<'a>),
}

// ------------------------------------
// Substitutions
// ------------------------------------

#[derive(Debug, Clone)]
pub struct CmdSub<'a> {
    pub and_open: Token<'a>,
    pub close_round: Token<'a>,
    pub seq_list: Box<List<'a>>,
}

// ---------------------------
// SyntaxTree
// ---------------------------

#[derive(Debug, Clone)]
pub struct SyntaxTree {}

/// The reserved words (§2.4), named like the grammar's terminals (§2.10.2).
/// Recognized only unquoted, as a whole word, where a command starts or where
/// the grammar expects one of them (`then`, `fi`...).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservedWord {
    /// `!`
    Bang,
    /// `{`
    Lbrace,
    /// `}`
    Rbrace,
    Case,
    Do,
    Done,
    Elif,
    Else,
    Esac,
    Fi,
    For,
    If,
    In,
    Then,
    Until,
    While,
}

/// A shell variable name: a letter or `_`, then letters, digits and `_`. Letters
/// and digits are Unicode ones, not only ASCII as in POSIX (XBD §3.235): names
/// and values are UTF-8 (`città=Roma`).
pub fn is_valid_name(name: &str) -> bool {
    // TODO: add error from this function
    let mut chars = name.chars();

    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

impl ReservedWord {
    /// The reserved word spelled `text`, if any.
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "!" => Self::Bang,
            "{" => Self::Lbrace,
            "}" => Self::Rbrace,
            "case" => Self::Case,
            "do" => Self::Do,
            "done" => Self::Done,
            "elif" => Self::Elif,
            "else" => Self::Else,
            "esac" => Self::Esac,
            "fi" => Self::Fi,
            "for" => Self::For,
            "if" => Self::If,
            "in" => Self::In,
            "then" => Self::Then,
            "until" => Self::Until,
            "while" => Self::While,
            _ => return None,
        })
    }
}

impl<'a> Parameter<'a> {
    /// The parameter named `text`, the name already lexed (`x`, `10`, `?`):
    /// `None` when it is none, or for digits too big to be a position.
    pub fn from_name(text: &'a str) -> Option<Self> {
        if let Some(special) = Special::from_name(text) {
            return Some(Self::Special(special));
        }
        if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
            return match text.parse().ok()? {
                0 => Some(Self::Special(Special::Zero)),
                n => Some(Self::Positional(n)),
            };
        }
        is_valid_name(text).then_some(Self::Name(text))
    }
}

impl Special {
    const ALL: [Self; 8] = [
        Self::At,
        Self::Star,
        Self::Hash,
        Self::Question,
        Self::Hyphen,
        Self::Dollar,
        Self::Bang,
        Self::Zero,
    ];

    /// How it is written after `$`.
    pub fn name(self) -> &'static str {
        match self {
            Self::At => "@",
            Self::Star => "*",
            Self::Hash => "#",
            Self::Question => "?",
            Self::Hyphen => "-",
            Self::Dollar => "$",
            Self::Bang => "!",
            Self::Zero => "0",
        }
    }

    fn from_name(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|special| special.name() == text)
    }
}

impl<'a> ParamOp<'a> {
    /// The operation of the operator `op` (`:-`, `%%`...), with its word.
    fn new(op: &str, word: Word<'a>) -> Option<Self> {
        let (colon, op) = match op.strip_prefix(':') {
            Some(op) => (true, op),
            None => (false, op),
        };

        Some(match (op, colon) {
            ("-", _) => Self::Default { colon, word },
            ("=", _) => Self::Assign { colon, word },
            ("?", _) => Self::Error { colon, word },
            ("+", _) => Self::Alternative { colon, word },
            ("%", false) => Self::RemoveSuffix {
                largest: false,
                pattern: word,
            },
            ("%%", false) => Self::RemoveSuffix {
                largest: true,
                pattern: word,
            },
            ("#", false) => Self::RemovePrefix {
                largest: false,
                pattern: word,
            },
            ("##", false) => Self::RemovePrefix {
                largest: true,
                pattern: word,
            },
            _ => return None,
        })
    }

    /// The operator as written, and its word; `None` for the length.
    fn operator(&self) -> Option<(&'static str, &Word<'a>)> {
        let pick = |flag: bool, with: &'static str, without: &'static str| {
            if flag { with } else { without }
        };

        Some(match self {
            Self::Length => return None,
            Self::Default { colon, word } => (pick(*colon, ":-", "-"), word),
            Self::Assign { colon, word } => (pick(*colon, ":=", "="), word),
            Self::Error { colon, word } => (pick(*colon, ":?", "?"), word),
            Self::Alternative { colon, word } => (pick(*colon, ":+", "+"), word),
            Self::RemoveSuffix { largest, pattern } => (pick(*largest, "%%", "%"), pattern),
            Self::RemovePrefix { largest, pattern } => (pick(*largest, "##", "#"), pattern),
        })
    }
}

/// The reserved word `word` is, if any: one unquoted literal with exactly its
/// text. `'if'`, `\if` and `if'x'` are ordinary words.
fn reserved(word: &Word<'_>) -> Option<ReservedWord> {
    unquoted_literal(word).and_then(ReservedWord::parse)
}

/// The text of `word` when it is one unquoted literal: what a reserved word or
/// the name of a `for` must be.
fn unquoted_literal<'a>(word: &Word<'a>) -> Option<&'a str> {
    let [WordPart::Literal(token)] = word.parts.as_slice() else {
        return None;
    };

    (token.t_type == TokenType::Literal).then_some(token.value)
}

// ------------------------------------
// Command text
// ------------------------------------
//
// The AST printed back as shell code, like bash does for `jobs`: the words keep
// their raw text (quotes, `$`, backslashes), the blanks are normalized.

/// Write `items` separated by `sep`.
fn join<T: fmt::Display>(f: &mut fmt::Formatter<'_>, items: &[T], sep: &str) -> fmt::Result {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(sep)?;
        }
        write!(f, "{item}")?;
    }
    Ok(())
}

impl fmt::Display for AssignmentWord<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}", self.key, self.value)
    }
}

/// The raw text: quotes, `$`, backslashes and backquotes as they were written.
impl fmt::Display for WordPart<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WordPart::Param(param) => write!(f, "{param}"),
            WordPart::Literal(token) | WordPart::Backquoted(token) => f.write_str(token.value),
            WordPart::SingleQuoted(texts) => {
                f.write_str("'")?;
                for text in texts {
                    f.write_str(text.value)?;
                }
                f.write_str("'")
            }
            WordPart::DoubleQuoted(parts) => {
                f.write_str("\"")?;
                for part in parts {
                    write!(f, "{part}")?;
                }
                f.write_str("\"")
            }
            WordPart::CmdSub(cmd) => write!(f, "{cmd}"),
        }
    }
}

impl fmt::Display for Parameter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Parameter::Name(name) => f.write_str(name),
            Parameter::Positional(n) => write!(f, "{n}"),
            Parameter::Special(special) => f.write_str(special.name()),
        }
    }
}

impl fmt::Display for ParamExpansion<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let param = &self.param;
        match &self.op {
            _ if !self.braced => write!(f, "${param}"),
            None => write!(f, "${{{param}}}"),
            Some(ParamOp::Length) => write!(f, "${{#{param}}}"),
            Some(op) => {
                let (operator, word) = op.operator().expect("not the length");
                write!(f, "${{{param}{operator}{word}}}")
            }
        }
    }
}

impl fmt::Display for Word<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for part in &self.parts {
            write!(f, "{part}")?;
        }
        Ok(())
    }
}

impl fmt::Display for CmdSub<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "$({})", self.seq_list)
    }
}

/// The fd is left out when it is the operator's default.
impl fmt::Display for Redirect<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (fd, default, op, target) = match self {
            Redirect::FileRedirect(file) => {
                let (op, default) = match file.file_kind {
                    OpenKind::Read => ("<", 0),
                    OpenKind::Replace => (">", 1),
                    OpenKind::Append => (">>", 1),
                    OpenKind::Rw => ("<>", 0),
                };
                (file.redirect_fd, default, op, file.filename.to_string())
            }
            Redirect::Dup(dup) if dup.output => (dup.fd, 1, ">&", dup.target.to_string()),
            Redirect::Dup(dup) => (dup.fd, 0, "<&", dup.target.to_string()),
        };

        if fd != default {
            write!(f, "{fd}")?;
        }
        write!(f, "{op}{target}")
    }
}

/// Either part can be empty (`>f` alone is an assignment of nothing).
impl fmt::Display for SimpleAssignment<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let envs = self.envs.iter().map(ToString::to_string);
        let redirections = self.redirections.iter().map(ToString::to_string);

        join(f, &envs.chain(redirections).collect::<Vec<_>>(), " ")
    }
}

impl fmt::Display for UnsubCommand<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for env in &self.envs {
            write!(f, "{env} ")?;
        }
        write!(f, "{}", self.program)?;
        for argument in &self.arguments {
            write!(f, " {argument}")?;
        }
        for redirect in &self.redirections {
            write!(f, " {redirect}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Subshell<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({})", self.seq_list)
    }
}

/// Write a compound list followed by its separator: `; `, or only a blank after
/// an asynchronous list, which already ends with its `&`.
fn write_terminated(f: &mut fmt::Formatter<'_>, list: &List<'_>) -> fmt::Result {
    match list {
        List::AsyncList(_) => write!(f, "{list} "),
        List::SequentialList(_) => write!(f, "{list}; "),
    }
}

impl fmt::Display for IfClause<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("if ")?;
        write_terminated(f, &self.condition)?;
        f.write_str("then ")?;
        write_terminated(f, &self.body)?;
        if let Some(else_part) = &self.else_part {
            write!(f, "{else_part}")?;
        }
        f.write_str("fi")
    }
}

/// Followed by a blank, ready for the next `elif`, `else` or `fi`.
impl fmt::Display for ElsePart<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElsePart::Elif(elif) => {
                f.write_str("elif ")?;
                write_terminated(f, &elif.condition)?;
                f.write_str("then ")?;
                write_terminated(f, &elif.body)?;
                if let Some(else_part) = &elif.else_part {
                    write!(f, "{else_part}")?;
                }
                Ok(())
            }
            ElsePart::Else(els) => {
                f.write_str("else ")?;
                write_terminated(f, &els.body)
            }
        }
    }
}

/// `for x do` prints as `for x; do`.
impl fmt::Display for ForClause<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "for {}", self.name)?;
        if let Some(words) = &self.wordlist {
            f.write_str(" in")?;
            for word in words {
                write!(f, " {word}")?;
            }
        }
        f.write_str("; do ")?;
        write_terminated(f, &self.body)?;
        f.write_str("done")
    }
}

impl fmt::Display for CaseClause<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "case {} in ", self.word)?;
        for item in &self.items {
            write!(f, "{item} ")?;
        }
        f.write_str("esac")
    }
}

/// Always with its `;;`. The `(` only when the first pattern is `esac`, which
/// would end the case without it (rule 4).
impl fmt::Display for CaseItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.patterns.first().and_then(reserved) == Some(ReservedWord::Esac) {
            f.write_str("(")?;
        }
        join(f, &self.patterns, "|")?;
        f.write_str(") ")?;
        if let Some(body) = &self.body {
            write!(f, "{body}")?;
        }
        f.write_str(";;")
    }
}

impl fmt::Display for WhileClause<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("while ")?;
        write_terminated(f, &self.condition)?;
        f.write_str("do ")?;
        write_terminated(f, &self.body)?;
        f.write_str("done")
    }
}

impl fmt::Display for UntilClause<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("until ")?;
        write_terminated(f, &self.condition)?;
        f.write_str("do ")?;
        write_terminated(f, &self.body)?;
        f.write_str("done")
    }
}

impl fmt::Display for CompoundCommand<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompoundCommand::Subshell(subshell) => write!(f, "{subshell}"),
            CompoundCommand::For(clause) => write!(f, "{clause}"),
            CompoundCommand::Case(clause) => write!(f, "{clause}"),
            CompoundCommand::While(clause) => write!(f, "{clause}"),
            CompoundCommand::Until(clause) => write!(f, "{clause}"),
            CompoundCommand::If(clause) => write!(f, "{clause}"),
        }
    }
}

impl fmt::Display for Command<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::SimpleAssignment(assignment) => write!(f, "{assignment}"),
            Command::UnsubCommand(command) => write!(f, "{command}"),
            Command::Compound {
                command,
                redirections,
            } => {
                write!(f, "{command}")?;
                for redirect in redirections {
                    write!(f, " {redirect}")?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for Pipeline<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.negated {
            f.write_str("! ")?;
        }
        join(f, &self.cmds, " | ")
    }
}

impl fmt::Display for OpList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpList::AndList(and) => write!(f, "{} && {}", and.left, and.right),
            OpList::OrList(or) => write!(f, "{} || {}", or.left, or.right),
            OpList::Pipeline(pipeline) => write!(f, "{pipeline}"),
        }
    }
}

/// Each element is followed by its separator: `;` between two elements, `&`
/// after an asynchronous one (`a & b; c &`).
impl fmt::Display for List<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (left, right, is_async) = match self {
            List::SequentialList(seq) => (&seq.left, &seq.right, false),
            List::AsyncList(asy) => (&asy.left, &asy.right, true),
        };

        if let Some(left) = left {
            match **left {
                // It already ends with its `&`.
                List::AsyncList(_) => write!(f, "{left} ")?,
                List::SequentialList(_) => write!(f, "{left}; ")?,
            }
        }
        write!(f, "{right}")?;

        if is_async {
            f.write_str(" &")?;
        }
        Ok(())
    }
}

// ------------------------------------
// SimpleCommand
// ------------------------------------

impl<'a> From<SequentialList<'a>> for AsyncList<'a> {
    fn from(value: SequentialList<'a>) -> Self {
        Self {
            left: value.left,
            right: value.right,
        }
    }
}

impl SyntaxTree {
    /// BNF:
    ///
    /// ```
    /// cmd_substitution ::= ANDOPEN compound_list CLOSE_ROUND
    ///                    ;
    /// ```
    pub fn cmdsub<'a, T>(tok: &T) -> Option<CmdSub<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        let and_open = sub_tok.next_if_type(TokenType::Andopen)?;

        let compound_list = Self::compound_list(&sub_tok)?;

        let close_round = sub_tok.next_if_type(TokenType::CloseRound)?;

        tok.merge(sub_tok);

        Some(CmdSub {
            and_open,
            close_round,
            seq_list: Box::new(compound_list),
        })
    }

    /// BNF:
    ///
    /// ```
    /// program ::= linebreak
    ///           | linebreak complete_commands linebreak
    ///           ;
    /// ```
    pub fn program<'a, T>(tok: &T) -> Option<ThisProgram<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        Self::linebreak(&sub_tok);

        if sub_tok.next_is_eof() {
            tok.merge(sub_tok);
            return Some(ThisProgram::default());
        }

        let complete_commands = Self::complete_commands(&sub_tok)?;

        Self::linebreak(&sub_tok);

        tok.merge(sub_tok);

        Some(ThisProgram {
            child: complete_commands,
        })
    }

    /// BNF:
    ///
    /// ```
    /// complete_commands ::= complete_command
    ///                     | complete_commands newline_list complete_command
    ///                     ;
    /// ```
    pub fn complete_commands<'a, T>(tok: &T) -> Option<CompleteCommands<'a>>
    where
        T: Tokenizer<'a>,
    {
        let complete_command = Self::complete_command(tok)?;

        let mut retval = CompleteCommands {
            commands: vec![complete_command],
        };

        loop {
            let sub_tok = tok.clone();

            if !Self::newline_list(&sub_tok) {
                break;
            }

            let Some(complete_command) = Self::complete_command(&sub_tok) else {
                break;
            };

            retval.commands.push(complete_command);

            tok.merge(sub_tok);
        }

        Some(retval)
    }

    /// BNF:
    ///
    /// ```
    /// complete_command ::= list
    ///                    | list separator_op
    ///                    ;
    /// ```
    pub fn complete_command<'a, T>(tok: &T) -> Option<List<'a>>
    where
        T: Tokenizer<'a>,
    {
        let list = Self::list(tok)?;

        // Only a final `&` runs the last and-or list in the background; a `;`
        // ends it like a newline.
        match Self::separator_op(tok) {
            Some(sep) if sep.t_type == TokenType::Andper => Some(List::AsyncList(list.into())),
            _ => Some(List::SequentialList(list)),
        }
    }

    /// BNF:
    ///
    /// ```
    /// list ::=                   and_or
    ///        | list separator_op and_or
    ///        ;
    /// ```
    pub fn list<'a, T>(tok: &T) -> Option<SequentialList<'a>>
    where
        T: Tokenizer<'a>,
    {
        let first_op_list = Self::and_or(tok)?;

        let mut retval = SequentialList {
            left: None,
            right: Box::new(first_op_list),
        };
        // Check if there are other sequential list to concatenate to the return value
        loop {
            // If the next token is not what we expect don't advance the tokenizer
            // sequence
            let sub_tok = tok.clone();

            let Some(sep) = Self::separator_op(&sub_tok) else {
                break;
            };

            let Some(next_op_list) = Self::and_or(&sub_tok) else {
                break;
            };

            let rotated_list = if sep.t_type == TokenType::Andper {
                SequentialList {
                    left: Some(Box::new(List::AsyncList(retval.into()))),
                    right: Box::new(next_op_list),
                }
            } else {
                SequentialList {
                    left: Some(Box::new(List::SequentialList(retval))),
                    right: Box::new(next_op_list),
                }
            };

            retval = rotated_list;

            tok.merge(sub_tok);
        }

        Some(retval)
    }

    /// BNF:
    ///
    /// ```
    /// and_or ::=                         pipeline
    ///          | and_or AND_IF linebreak pipeline
    ///          | and_or OR_IF  linebreak pipeline
    ///          ;
    /// ```
    pub fn and_or<'a, T>(tok: &T) -> Option<OpList<'a>>
    where
        T: Tokenizer<'a>,
    {
        let pipeline = Self::pipeline(tok)?;

        let mut retval = OpList::Pipeline(pipeline);

        loop {
            // Duplicate the tokenizer. By duplicating, if the parsing
            // fails, the advancements are not committed to the original
            // tokenizer.
            let sub_tok = tok.clone();

            let Some(next_token) = sub_tok.next_token() else {
                break;
            };

            // After this if the token is either and_and or or_or
            if next_token.t_type != TokenType::AndAnd && next_token.t_type != TokenType::OrOr {
                break;
            }

            Self::linebreak(&sub_tok);

            // The next tokens must be a pipeline
            // TODO: do some error handling?
            let Some(rhs_pipeline) = Self::pipeline(&sub_tok) else {
                break;
            };

            retval = match next_token.t_type {
                TokenType::AndAnd => OpList::AndList(AndList {
                    left: Box::new(retval),
                    right: Box::new(OpList::Pipeline(rhs_pipeline)),
                }),
                TokenType::OrOr => OpList::OrList(OrList {
                    left: Box::new(retval),
                    right: Box::new(OpList::Pipeline(rhs_pipeline)),
                }),
                _ => unreachable!("Should not be here!"),
            };

            tok.merge(sub_tok);
        }

        Some(retval)
    }

    /// BNF:
    ///
    /// ```
    /// pipeline ::=      pipe_sequence
    ///            | Bang pipe_sequence
    ///            ;
    /// ```
    pub fn pipeline<'a, T>(tok: &T) -> Option<Pipeline<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        let has_bang = sub_tok.next_if_type(TokenType::Bang).is_some();

        let mut pipe_sequence = Self::pipe_sequence(&sub_tok)?;

        pipe_sequence.negated = has_bang;

        tok.merge(sub_tok);

        Some(pipe_sequence)
    }

    /// BNF:
    ///
    /// ```
    /// pipe_sequence ::=                             command
    ///                 | pipe_sequence '|' linebreak command
    ///                 ;
    /// ```
    pub fn pipe_sequence<'a, T>(tok: &T) -> Option<Pipeline<'a>>
    where
        T: Tokenizer<'a>,
    {
        let first_command = Self::command(tok)?;

        let mut retval = Pipeline {
            cmds: vec![first_command],
            negated: false,
        };

        // Check if there are other commands to concatenate to the return value
        loop {
            // If the next token is not what we expect don't advance the tokenizer
            // sequence
            let sub_tok = tok.clone();

            let Some(_pipe) = sub_tok.next_if_type(TokenType::Pipe) else {
                break;
            };

            Self::linebreak(&sub_tok);

            // TODO: error handling
            let Some(next_command) = Self::command(&sub_tok) else {
                break;
            };

            retval.cmds.push(next_command);

            tok.merge(sub_tok);
        }

        Some(retval)
    }

    /// BNF:
    ///
    /// ```
    /// command ::= simple_command
    ///           | compound_command
    ///           | compound_command redirect_list
    ///           | function_definition
    ///           ;
    /// ```
    pub fn command<'a, T>(tok: &T) -> Option<Command<'a>>
    where
        T: Tokenizer<'a>,
    {
        // TODO: `function_definition`, once `Self::function_definition` is
        // implemented.
        if let Some(simple_command) = Self::simple_command(tok) {
            return Some(simple_command);
        }

        if let Some(command) = Self::compound_command(tok) {
            let redirections = Self::redirect_list(tok).unwrap_or_default();

            return Some(Command::Compound {
                command,
                redirections,
            });
        }

        None
    }

    /// BNF:
    ///
    /// ```
    /// compound_command ::= brace_group
    ///                    | subshell
    ///                    | for_clause
    ///                    | case_clause
    ///                    | if_clause
    ///                    | while_clause
    ///                    | until_clause
    ///                    ;
    /// ```
    pub fn compound_command<'a, T>(tok: &T) -> Option<CompoundCommand<'a>>
    where
        T: Tokenizer<'a>,
    {
        // TODO: try `Self::brace_group` once it is implemented.
        if let Some(subshell) = Self::subshell(tok) {
            return Some(CompoundCommand::Subshell(subshell));
        }

        if let Some(clause) = Self::for_clause(tok) {
            return Some(CompoundCommand::For(clause));
        }

        if let Some(clause) = Self::case_clause(tok) {
            return Some(CompoundCommand::Case(clause));
        }

        if let Some(clause) = Self::if_clause(tok) {
            return Some(CompoundCommand::If(clause));
        }

        if let Some(clause) = Self::while_clause(tok) {
            return Some(CompoundCommand::While(clause));
        }

        Self::until_clause(tok).map(CompoundCommand::Until)
    }

    /// BNF:
    ///
    /// ```
    /// subshell ::= '(' compound_list ')'
    ///            ;
    /// ```
    pub fn subshell<'a, T>(tok: &T) -> Option<Subshell<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        let open_round = sub_tok.next_if_type(TokenType::OpenRound)?;

        let compound_list = Self::compound_list(&sub_tok)?;

        let close_round = sub_tok.next_if_type(TokenType::CloseRound)?;

        tok.merge(sub_tok);

        Some(Subshell {
            open_round,
            close_round,
            seq_list: Box::new(compound_list),
        })
    }

    /// BNF:
    ///
    /// ```
    /// compound_list ::= linebreak term
    ///                 | linebreak term separator
    ///                 ;
    /// ```
    pub fn compound_list<'a, T>(tok: &T) -> Option<List<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        Self::linebreak(&sub_tok);

        let term = Self::term(&sub_tok)?;

        if let Some(sep) = Self::separator(&sub_tok)
            && sep.t_type == TokenType::Andper
        {
            tok.merge(sub_tok);

            return Some(List::AsyncList(term.into()));
        }

        tok.merge(sub_tok);

        Some(List::SequentialList(term))
    }

    /// BNF:
    ///
    /// ```
    /// term ::= term separator and_or
    ///        |                and_or
    ///        ;
    /// ```
    pub fn term<'a, T>(tok: &T) -> Option<SequentialList<'a>>
    where
        T: Tokenizer<'a>,
    {
        let and_or = Self::and_or(tok)?;

        let mut retval = SequentialList {
            left: None,
            right: Box::new(and_or),
        };

        // Check if there are other sequential list to concatenate to the return value
        loop {
            let sub_tok = tok.clone();

            // If the next token is not what we expect don't advance the tokenizer
            // sequence
            let Some(sep) = Self::separator(&sub_tok) else {
                break;
            };

            let Some(next_op_list) = Self::and_or(&sub_tok) else {
                break;
            };

            let rotated_list = if sep.t_type == TokenType::Andper {
                SequentialList {
                    left: Some(Box::new(List::AsyncList(retval.into()))),
                    right: Box::new(next_op_list),
                }
            } else {
                SequentialList {
                    left: Some(Box::new(List::SequentialList(retval))),
                    right: Box::new(next_op_list),
                }
            };

            retval = rotated_list;

            tok.merge(sub_tok);
        }

        Some(retval)
    }

    /// BNF:
    ///
    /// ```
    /// for_clause ::= For name                                      do_group
    ///              | For name                       sequential_sep do_group
    ///              | For name linebreak in          sequential_sep do_group
    ///              | For name linebreak in wordlist sequential_sep do_group
    ///              ;
    /// ```
    pub fn for_clause<'a, T>(tok: &T) -> Option<ForClause<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        Self::reserved_word(&sub_tok, ReservedWord::For)?;
        let name = Self::name(&sub_tok)?;

        // `linebreak in` only when an `in` follows: otherwise the newlines are
        // the `sequential_sep` of `for x <newline> do`.
        let in_tok = sub_tok.clone();
        Self::linebreak(&in_tok);
        let wordlist = if Self::r#in(&in_tok).is_some() {
            sub_tok.merge(in_tok);
            let words = Self::wordlist(&sub_tok);
            Self::sequential_sep(&sub_tok)?;
            Some(words)
        } else {
            // Optional: `for x do`.
            Self::sequential_sep(&sub_tok);
            None
        };

        let body = Self::do_group(&sub_tok)?;

        tok.merge(sub_tok);

        Some(ForClause {
            name,
            wordlist,
            body,
        })
    }

    /// BNF:
    ///
    /// ```
    /// name ::= NAME                                   Apply rule 5
    ///        ;
    /// ```
    ///
    /// Rule 5: the word must be a valid name, unquoted (`for 1x` and `for 'x'`
    /// are syntax errors, like in dash). A reserved word is one too
    /// (`for do in a`).
    pub fn name<'a, T>(tok: &T) -> Option<&'a str>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        let word = Self::word(&sub_tok)?;
        let name = unquoted_literal(&word).filter(|name| is_valid_name(name))?;

        tok.merge(sub_tok);

        Some(name)
    }

    /// BNF:
    ///
    /// ```
    /// in ::= In                                       Apply rule 6
    ///      ;
    /// ```
    pub fn r#in<'a, T>(tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        Self::reserved_word(tok, ReservedWord::In)
    }

    /// BNF:
    ///
    /// ```
    /// wordlist ::= wordlist WORD
    ///            |          WORD
    ///            ;
    /// ```
    ///
    /// Empty when there is no word: the caller tells `in` alone apart. Reserved
    /// words are words here (`for x in in do`).
    pub fn wordlist<'a, T>(tok: &T) -> Vec<Word<'a>>
    where
        T: Tokenizer<'a>,
    {
        let mut words = Vec::new();
        while let Some(word) = Self::word(tok) {
            words.push(word);
        }

        words
    }

    /// BNF:
    ///
    /// ```
    /// case_clause ::= Case WORD linebreak in linebreak case_list    Esac
    ///               | Case WORD linebreak in linebreak case_list_ns Esac
    ///               | Case WORD linebreak in linebreak              Esac
    ///               ;
    /// ```
    pub fn case_clause<'a, T>(tok: &T) -> Option<CaseClause<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        Self::reserved_word(&sub_tok, ReservedWord::Case)?;
        let word = Self::word(&sub_tok)?;
        Self::linebreak(&sub_tok);
        Self::r#in(&sub_tok)?;
        Self::linebreak(&sub_tok);
        let items = Self::case_list_ns(&sub_tok);
        Self::reserved_word(&sub_tok, ReservedWord::Esac)?;

        tok.merge(sub_tok);

        Some(CaseClause { word, items })
    }

    /// BNF:
    ///
    /// ```
    /// case_list_ns ::= case_list case_item_ns
    ///                |           case_item_ns
    ///                ;
    /// ```
    ///
    /// Read as the items with their `;;` (`case_list`), then an optional last
    /// one without it, this covers the three alternatives of `case_clause`
    /// (`case_list`, `case_list_ns` and no item). Every item is parsed once:
    /// trying the alternatives one by one would parse them again, twice at
    /// each level of nesting.
    pub fn case_list_ns<'a, T>(tok: &T) -> Vec<CaseItem<'a>>
    where
        T: Tokenizer<'a>,
    {
        let mut items = Self::case_list(tok);
        items.extend(Self::case_item_ns(tok));

        items
    }

    /// BNF:
    ///
    /// ```
    /// case_list ::= case_list case_item
    ///             |           case_item
    ///             ;
    /// ```
    ///
    /// Empty when there is no item: the caller tells the alternatives apart.
    pub fn case_list<'a, T>(tok: &T) -> Vec<CaseItem<'a>>
    where
        T: Tokenizer<'a>,
    {
        let mut items = Vec::new();
        while let Some(item) = Self::case_item(tok) {
            items.push(item);
        }

        items
    }

    /// BNF:
    ///
    /// ```
    /// case_item_ns ::=     pattern ')' linebreak
    ///                |     pattern ')' compound_list
    ///                | '(' pattern ')' linebreak
    ///                | '(' pattern ')' compound_list
    ///                ;
    /// ```
    ///
    /// Rule 4 is applied here: without the `(`, a word `esac` ends the case
    /// instead of starting an item. After `(` it is a pattern, like in dash
    /// and bash (`case esac in (esac)`).
    pub fn case_item_ns<'a, T>(tok: &T) -> Option<CaseItem<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        let paren = sub_tok.next_if_type(TokenType::OpenRound).is_some();
        if !paren && Self::reserved_word(&sub_tok.clone(), ReservedWord::Esac).is_some() {
            return None;
        }

        let patterns = Self::pattern(&sub_tok)?;
        sub_tok.next_if_type(TokenType::CloseRound)?;

        let body = Self::compound_list(&sub_tok);
        if body.is_none() {
            Self::linebreak(&sub_tok);
        }

        tok.merge(sub_tok);

        Some(CaseItem { patterns, body })
    }

    /// BNF:
    ///
    /// ```
    /// case_item ::=     pattern ')' linebreak     DSEMI linebreak
    ///             |     pattern ')' compound_list DSEMI linebreak
    ///             | '(' pattern ')' linebreak     DSEMI linebreak
    ///             | '(' pattern ')' compound_list DSEMI linebreak
    ///             ;
    /// ```
    pub fn case_item<'a, T>(tok: &T) -> Option<CaseItem<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        let item = Self::case_item_ns(&sub_tok)?;
        sub_tok.next_if_type(TokenType::Dsemi)?;
        Self::linebreak(&sub_tok);

        tok.merge(sub_tok);

        Some(item)
    }

    /// BNF:
    ///
    /// ```
    /// pattern ::=             WORD                Apply rule 4
    ///           | pattern '|' WORD                Do not apply rule 4
    ///           ;
    /// ```
    ///
    /// Rule 4 is applied by `case_item_ns`, which knows about the `(`.
    pub fn pattern<'a, T>(tok: &T) -> Option<Vec<Word<'a>>>
    where
        T: Tokenizer<'a>,
    {
        let mut patterns = vec![Self::word(tok)?];

        loop {
            let sub_tok = tok.clone();

            if sub_tok.next_if_type(TokenType::Pipe).is_none() {
                break;
            }
            let Some(pattern) = Self::word(&sub_tok) else {
                break;
            };

            patterns.push(pattern);
            tok.merge(sub_tok);
        }

        Some(patterns)
    }

    /// BNF:
    ///
    /// ```
    /// if_clause ::= If compound_list Then compound_list else_part Fi
    ///             | If compound_list Then compound_list           Fi
    ///             ;
    /// ```
    pub fn if_clause<'a, T>(tok: &T) -> Option<IfClause<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        Self::reserved_word(&sub_tok, ReservedWord::If)?;
        let condition = Self::compound_list(&sub_tok)?;
        Self::reserved_word(&sub_tok, ReservedWord::Then)?;
        let body = Self::compound_list(&sub_tok)?;
        let else_part = Self::else_part(&sub_tok).map(Box::new);
        Self::reserved_word(&sub_tok, ReservedWord::Fi)?;

        tok.merge(sub_tok);

        Some(IfClause {
            condition,
            body,
            else_part,
        })
    }

    /// BNF:
    ///
    /// ```
    /// else_part ::= Elif compound_list Then compound_list
    ///             | Elif compound_list Then compound_list else_part
    ///             | Else compound_list
    ///             ;
    /// ```
    pub fn else_part<'a, T>(tok: &T) -> Option<ElsePart<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        let else_part = if Self::reserved_word(&sub_tok, ReservedWord::Elif).is_some() {
            let condition = Self::compound_list(&sub_tok)?;
            Self::reserved_word(&sub_tok, ReservedWord::Then)?;
            let body = Self::compound_list(&sub_tok)?;
            let else_part = Self::else_part(&sub_tok).map(Box::new);

            ElsePart::Elif(ElifClause {
                condition,
                body,
                else_part,
            })
        } else {
            Self::reserved_word(&sub_tok, ReservedWord::Else)?;
            let body = Self::compound_list(&sub_tok)?;

            ElsePart::Else(ElseClause { body })
        };

        tok.merge(sub_tok);

        Some(else_part)
    }

    /// BNF:
    ///
    /// ```
    /// while_clause ::= While compound_list do_group
    ///                ;
    /// ```
    pub fn while_clause<'a, T>(tok: &T) -> Option<WhileClause<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        Self::reserved_word(&sub_tok, ReservedWord::While)?;
        let condition = Self::compound_list(&sub_tok)?;
        let body = Self::do_group(&sub_tok)?;

        tok.merge(sub_tok);

        Some(WhileClause { condition, body })
    }

    /// BNF:
    ///
    /// ```
    /// until_clause ::= Until compound_list do_group
    ///                ;
    /// ```
    pub fn until_clause<'a, T>(tok: &T) -> Option<UntilClause<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        Self::reserved_word(&sub_tok, ReservedWord::Until)?;
        let condition = Self::compound_list(&sub_tok)?;
        let body = Self::do_group(&sub_tok)?;

        tok.merge(sub_tok);

        Some(UntilClause { condition, body })
    }

    /// BNF:
    ///
    /// ```
    /// function_definition ::= fname '(' ')' linebreak function_body
    ///                       ;
    /// ```
    pub fn function_definition<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("function_definition: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// function_body ::= compound_command                  Apply rule 9
    ///                 | compound_command redirect_list    Apply rule 9
    ///                 ;
    /// ```
    pub fn function_body<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("function_body: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// fname ::= NAME                                  Apply rule 8
    ///         ;
    /// ```
    pub fn fname<'a, T>(_tok: &T) -> Option<Token<'a>>
    where
        T: Tokenizer<'a>,
    {
        unimplemented!("fname: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// brace_group ::= Lbrace compound_list Rbrace
    ///               ;
    /// ```
    pub fn brace_group<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("brace_group: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// do_group ::= Do compound_list Done              Apply rule 6
    ///            ;
    /// ```
    pub fn do_group<'a, T>(tok: &T) -> Option<List<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        Self::reserved_word(&sub_tok, ReservedWord::Do)?;
        let body = Self::compound_list(&sub_tok)?;
        Self::reserved_word(&sub_tok, ReservedWord::Done)?;

        tok.merge(sub_tok);

        Some(body)
    }

    /// BNF:
    ///
    /// ```
    /// simple_command ::= cmd_prefix cmd_word cmd_suffix
    ///                  | cmd_prefix cmd_word
    ///                  | cmd_prefix
    ///                  | cmd_name cmd_suffix
    ///                  | cmd_name
    ///                  ;
    /// ```
    pub fn simple_command<'a, T>(tok: &T) -> Option<Command<'a>>
    where
        T: Tokenizer<'a>,
    {
        let cmd_prefix = Self::cmd_prefix(tok);
        if !cmd_prefix.is_empty() {
            let cmd_word = Self::cmd_word(tok);
            let cmd_suffix = if cmd_word.is_some() {
                Self::cmd_suffix(tok)
            } else {
                vec![]
            };

            let mut args = Vec::with_capacity(16);
            let mut redirections = Vec::with_capacity(16);
            let mut assignments = Vec::with_capacity(16);

            for prefix in cmd_prefix {
                match prefix {
                    CmdPre::AssignmentWord(a) => assignments.push(a),
                    CmdPre::Redirect(r) => redirections.push(r),
                }
            }

            for suffix in cmd_suffix {
                match suffix {
                    CmdSuf::Word(w) => args.push(w),
                    CmdSuf::Redirect(r) => redirections.push(r),
                }
            }

            return if let Some(program) = cmd_word {
                Some(Command::UnsubCommand(UnsubCommand {
                    program,
                    arguments: args,
                    redirections,
                    envs: assignments,
                }))
            } else {
                Some(Command::SimpleAssignment(SimpleAssignment {
                    redirections,
                    envs: assignments,
                }))
            };
        }

        if let Some(cmd_name) = Self::cmd_name(tok) {
            let cmd_suffix = Self::cmd_suffix(tok);

            let mut args = Vec::with_capacity(16);
            let mut redirections = Vec::with_capacity(16);

            for suffix in cmd_suffix {
                match suffix {
                    CmdSuf::Word(w) => args.push(w),
                    CmdSuf::Redirect(r) => redirections.push(r),
                }
            }

            return Some(Command::UnsubCommand(UnsubCommand {
                program: cmd_name,
                arguments: args,
                redirections,
                envs: vec![],
            }));
        }

        None
    }

    /// BNF:
    ///
    /// ```
    /// cmd_name       ::= WORD                       Apply rule 7a
    ///                  ;
    /// ```
    pub fn cmd_name<'a, T>(tok: &T) -> Option<Word<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        // Rule 7a (and 1): a reserved word where a command starts is not a
        // WORD, so `then` ends the condition of an `if`. After a prefix it is
        // (rule 7b, `cmd_word`): `X=1 if` runs a command named `if`.
        let word = Self::word(&sub_tok).filter(|word| reserved(word).is_none())?;

        tok.merge(sub_tok);

        Some(word)
    }

    /// BNF:
    ///
    /// ```
    /// cmd_word       ::= WORD                       Apply rule 7b
    ///                  ;
    /// ```
    pub fn cmd_word<'a, T>(tok: &T) -> Option<Word<'a>>
    where
        T: Tokenizer<'a>,
    {
        // TODO: apply rule 7b
        Self::word(tok)
    }

    /// ```
    /// cmd_prefix     ::=            io_redirect
    ///                  | cmd_prefix io_redirect
    ///                  |            ASSIGNMENT_WORD
    ///                  | cmd_prefix ASSIGNMENT_WORD
    ///                  ;
    /// ```
    pub fn cmd_prefix<'a, T>(tok: &T) -> Vec<CmdPre<'a>>
    where
        T: Tokenizer<'a>,
    {
        let mut prefixes = Vec::with_capacity(16);

        loop {
            if let Some(word) = Self::assignment_word(tok) {
                prefixes.push(CmdPre::AssignmentWord(word));
                continue;
            }

            if let Some(redirect) = Self::io_redirect(tok) {
                prefixes.push(CmdPre::Redirect(redirect));
                continue;
            }

            break;
        }

        prefixes
    }

    /// BNF:
    ///
    /// ```
    /// cmd_suffix     ::=            io_redirect
    ///                  | cmd_suffix io_redirect
    ///                  |            WORD
    ///                  | cmd_suffix WORD
    ///                  ;
    /// ```
    pub fn cmd_suffix<'a, T>(tok: &T) -> Vec<CmdSuf<'a>>
    where
        T: Tokenizer<'a>,
    {
        let mut suffixes = Vec::with_capacity(16);

        loop {
            if let Some(word) = Self::word(tok) {
                suffixes.push(CmdSuf::Word(word));
                continue;
            }

            if let Some(redirect) = Self::io_redirect(tok) {
                suffixes.push(CmdSuf::Redirect(redirect));
                continue;
            }

            break;
        }

        suffixes
    }

    /// BNF:
    ///
    /// ```
    /// redirect_list ::=               io_redirect
    ///                 | redirect_list io_redirect
    ///                 ;
    /// ```
    pub fn redirect_list<'a, T>(tok: &T) -> Option<Vec<Redirect<'a>>>
    where
        T: Tokenizer<'a>,
    {
        let first_rediret = Self::io_redirect(tok)?;

        let mut redirects = vec![first_rediret];

        while let Some(next_redirect) = Self::io_redirect(tok) {
            redirects.push(next_redirect);
        }

        Some(redirects)
    }

    /// BNF:
    ///
    /// ```
    /// io_redirect ::=           io_file
    ///               | IO_NUMBER io_file
    ///               |           io_here
    ///               | IO_NUMBER io_here
    ///               ;
    /// ```
    pub fn io_redirect<'a, T>(tok: &T) -> Option<Redirect<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        let io_number = sub_tok.next_if_type(TokenType::IoNumber);

        let new_redirect_fd = io_number.map(|io| {
            io.value
                .parse::<i32>()
                .expect("Parsed number shoud be a... number")
        });

        let mut redirect = Self::io_file(&sub_tok)?;

        // Replace fd if io_number is present
        if let Some(new_redirect_fd) = new_redirect_fd {
            match &mut redirect {
                Redirect::FileRedirect(file) => file.redirect_fd = new_redirect_fd,
                Redirect::Dup(dup) => dup.fd = new_redirect_fd,
            }
        }

        tok.merge(sub_tok);

        Some(redirect)
    }

    /// BNF
    ///
    /// ```
    /// io_file ::= '<'       filename
    ///           | LESSAND   filename
    ///           | '>'       filename
    ///           | GREATAND  filename
    ///           | DGREAT    filename
    ///           | LESSGREAT filename
    ///           | CLOBBER   filename
    ///           ;
    /// ```
    pub fn io_file<'a, T>(tok: &T) -> Option<Redirect<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        let redirect_token = sub_tok.next_token()?;

        // TODO: error handling
        let filename = Self::filename(&sub_tok)?;

        let redirect = match redirect_token.t_type {
            TokenType::Less => Redirect::FileRedirect(FileRedirect {
                // TODO: change to handle fds
                redirect_fd: 0,
                file_kind: OpenKind::Read,
                filename,
            }),
            // TODO: `>|` overrides noclobber (`set -C`), which doesn't exist
            // yet: for now it is `>`.
            TokenType::Great | TokenType::Clobber => Redirect::FileRedirect(FileRedirect {
                // TODO: change to handle fds
                redirect_fd: 1,
                file_kind: OpenKind::Replace,
                filename,
            }),
            TokenType::Dgreat => Redirect::FileRedirect(FileRedirect {
                // TODO: change to handle fds
                redirect_fd: 1,
                file_kind: OpenKind::Append,
                filename,
            }),
            TokenType::Lessgreat => Redirect::FileRedirect(FileRedirect {
                // TODO: change to handle fds
                redirect_fd: 0,
                file_kind: OpenKind::Rw,
                filename,
            }),
            TokenType::Lessand => Redirect::Dup(DupRedirect {
                fd: 0,
                output: false,
                target: filename,
            }),
            TokenType::Greatand => Redirect::Dup(DupRedirect {
                fd: 1,
                output: true,
                target: filename,
            }),
            _ => return None,
        };

        tok.merge(sub_tok);

        Some(redirect)
    }

    /// BNF:
    ///
    /// ```
    /// filename ::= WORD                           Apply rule 2
    ///            ;
    /// ```
    pub fn filename<'a, T>(tok: &T) -> Option<Word<'a>>
    where
        T: Tokenizer<'a>,
    {
        Self::word(tok)
    }

    /// BNF:
    ///
    /// ```
    /// io_here ::= DLESS     here_end
    ///           | DLESSDASH here_end
    ///           ;
    /// ```
    pub fn io_here<'a, T>(_tok: &T) -> Option<Redirect<'a>>
    where
        T: Tokenizer<'a>,
    {
        unimplemented!("io_here: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// here_end ::= WORD                               Apply rule 3
    ///            ;
    /// ```
    pub fn here_end<'a, T>(_tok: &T) -> Option<Word<'a>>
    where
        T: Tokenizer<'a>,
    {
        unimplemented!("here_end: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// newline_list ::=              NEWLINE
    ///                | newline_list NEWLINE
    ///                ;
    /// ```
    pub fn newline_list<'a, T>(tok: &T) -> bool
    where
        T: Tokenizer<'a>,
    {
        let newline = tok.next_if_type(TokenType::NewLine);
        if newline.is_none() {
            return false;
        }

        while tok.next_if_type(TokenType::NewLine).is_some() {}

        true
    }

    /// BNF:
    ///
    /// ```
    /// linebreak ::= newline_list
    ///             | EMPTY
    ///             ;
    /// ```
    pub fn linebreak<'a, T>(tok: &T)
    where
        T: Tokenizer<'a>,
    {
        Self::newline_list(tok);
    }

    /// BNF:
    ///
    /// ```
    /// separator_op ::= '&'
    ///                | ';'
    ///                ;
    /// ```
    pub fn separator_op<'a, T>(tok: &T) -> Option<Token<'a>>
    where
        T: Tokenizer<'a>,
    {
        if let Some(andp) = tok.next_if_type(TokenType::Andper) {
            return Some(andp);
        }

        if let Some(semi) = tok.next_if_type(TokenType::Semicolon) {
            return Some(semi);
        }

        None
    }

    /// BNF:
    ///
    /// ```
    /// separator ::= separator_op linebreak
    ///             | newline_list
    ///             ;
    /// ```
    pub fn separator<'a, T>(tok: &T) -> Option<Token<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sep = Self::separator_op(tok);
        if sep.is_some() {
            Self::linebreak(tok);
            return sep;
        }

        let newlines = Self::newline_list(tok);
        if newlines {
            return Some(Token {
                t_type: TokenType::NewLine,
                value: "",
                start: 0,
                end: 0,
                glued: false,
            });
        }

        None
    }

    /// BNF:
    ///
    /// ```
    /// sequential_sep ::= ';' linebreak
    ///                  | newline_list
    ///                  ;
    /// ```
    pub fn sequential_sep<'a, T>(tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        if tok.next_if_type(TokenType::Semicolon).is_some() {
            Self::linebreak(tok);
            return Some(());
        }

        Self::newline_list(tok).then_some(())
    }

    /// The `WORD` terminal: one part, then every part glued to it (§2.3): plain
    /// text, quotes, parameters and command substitutions with no blank between
    /// them.
    pub fn word<'a, T>(tok: &T) -> Option<Word<'a>>
    where
        T: Tokenizer<'a>,
    {
        let first = Self::word_part(tok, true)?;
        let mut parts = vec![first];

        while tok.peek().is_some_and(|token| token.glued) {
            let Some(part) = Self::word_part(tok, false) else {
                break;
            };
            parts.push(part);
        }

        Some(Word { parts })
    }

    /// One part of a word, outside quotes. `glued` digits before `<`/`>` are
    /// part of the word (`'a'2>f`), not an `IO_NUMBER`; a word can't start
    /// with them.
    fn word_part<'a, T>(tok: &T, first: bool) -> Option<WordPart<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();
        let token = sub_tok.peek()?;

        let part = match token.t_type {
            TokenType::Literal => WordPart::Literal(sub_tok.next_token()?),
            TokenType::IoNumber if !first => WordPart::Literal(sub_tok.next_token()?),
            TokenType::Param | TokenType::ParamOpen => {
                WordPart::Param(Self::param_expansion(&sub_tok, Mode::Normal)?)
            }
            TokenType::Backquoted => WordPart::Backquoted(sub_tok.next_token()?),
            TokenType::Andopen => WordPart::CmdSub(Self::cmdsub(&sub_tok)?),
            TokenType::SQuote => {
                sub_tok.next_token()?;
                WordPart::SingleQuoted(Self::single_quoted(&sub_tok)?)
            }
            TokenType::DQuote => {
                sub_tok.next_token()?;
                WordPart::DoubleQuoted(Self::double_quoted(&sub_tok)?)
            }
            _ => return None,
        };

        tok.merge(sub_tok);
        Some(part)
    }

    /// The text between single quotes, after the opening `'`, up to and with
    /// the closing one: none for `''`, a token per line when it spans lines.
    /// `None` when the closing quote is missing.
    fn single_quoted<'a, T>(tok: &T) -> Option<Vec<Token<'a>>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        let texts =
            std::iter::from_fn(|| sub_tok.next_if_type_in(Mode::SingleQuoted, TokenType::SqText))
                .collect();
        sub_tok.next_if_type_in(Mode::SingleQuoted, TokenType::SQuote)?;

        tok.merge(sub_tok);
        Some(texts)
    }

    /// A parameter expansion (§2.6.2), read in `mode` (the context: outside or
    /// inside double quotes, or in the word of another `${...}`):
    ///
    /// ```
    /// param_expansion ::= PARAM                                 $x $1 $?
    ///                   | '${' PARAM_NAME '}'
    ///                   | '${' '#' PARAM_NAME '}'                the length
    ///                   | '${' PARAM_NAME PARAM_OP param_word    up to the '}'
    ///                   ;
    /// ```
    ///
    /// `None` (a syntax error) for a `${` that doesn't name a parameter
    /// (`${}`, `${a b}`), has no `}`, or a bad operator.
    fn param_expansion<'a, T>(tok: &T, mode: Mode) -> Option<ParamExpansion<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        if let Some(token) = sub_tok.next_if_type_in(mode, TokenType::Param) {
            let param = Parameter::from_name(token.value.strip_prefix('$')?)?;
            tok.merge(sub_tok);
            return Some(ParamExpansion {
                param,
                braced: false,
                op: None,
            });
        }

        sub_tok.next_if_type_in(mode, TokenType::ParamOpen)?;
        let length = sub_tok
            .next_if_type_in(Mode::ParamName, TokenType::ParamHash)
            .is_some();
        let name = sub_tok.next_if_type_in(Mode::ParamName, TokenType::ParamName)?;
        let param = Parameter::from_name(name.value)?;

        let next = sub_tok.next_token_in(Mode::ParamOp)?;
        let op = match next.t_type {
            TokenType::CloseBrace if length => Some(ParamOp::Length),
            TokenType::CloseBrace => None,
            TokenType::ParamOp if !length => {
                let quoted = matches!(mode, Mode::DoubleQuoted | Mode::ParamWordDq);
                let word = Self::param_word(&sub_tok, quoted)?;
                Some(ParamOp::new(next.value, word)?)
            }
            _ => return None,
        };

        tok.merge(sub_tok);
        Some(ParamExpansion {
            param,
            braced: true,
            op,
        })
    }

    /// The word after the operator of a `${...}`, up to and with its `}`.
    /// `quoted` inside double quotes, where `'` is an ordinary character. It
    /// can be empty (`${x:-}`).
    fn param_word<'a, T>(tok: &T, quoted: bool) -> Option<Word<'a>>
    where
        T: Tokenizer<'a>,
    {
        let mode = if quoted {
            Mode::ParamWordDq
        } else {
            Mode::ParamWord
        };
        let sub_tok = tok.clone();
        let mut parts = Vec::new();

        loop {
            let token = sub_tok.peek_in(mode)?;
            let part = match token.t_type {
                TokenType::CloseBrace => {
                    sub_tok.next_token_in(mode)?;
                    tok.merge(sub_tok);
                    return Some(Word { parts });
                }
                TokenType::Literal | TokenType::DqText => {
                    WordPart::Literal(sub_tok.next_token_in(mode)?)
                }
                TokenType::Param | TokenType::ParamOpen => {
                    WordPart::Param(Self::param_expansion(&sub_tok, mode)?)
                }
                TokenType::Andopen => WordPart::CmdSub(Self::cmdsub(&sub_tok)?),
                TokenType::Backquoted => WordPart::Backquoted(sub_tok.next_token_in(mode)?),
                TokenType::SQuote => {
                    sub_tok.next_token_in(mode)?;
                    WordPart::SingleQuoted(Self::single_quoted(&sub_tok)?)
                }
                TokenType::DQuote => {
                    sub_tok.next_token_in(mode)?;
                    WordPart::DoubleQuoted(Self::double_quoted(&sub_tok)?)
                }
                // The end of the input: no `}`.
                _ => return None,
            };
            parts.push(part);
        }
    }

    /// The parts between double quotes, after the opening `"`, up to and with
    /// the closing one. `None` when it is missing.
    fn double_quoted<'a, T>(tok: &T) -> Option<Vec<WordPart<'a>>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();
        let mut parts = Vec::new();

        loop {
            let token = sub_tok.peek_in(Mode::DoubleQuoted)?;
            let part = match token.t_type {
                TokenType::DQuote => {
                    sub_tok.next_token_in(Mode::DoubleQuoted)?;
                    tok.merge(sub_tok);
                    return Some(parts);
                }
                TokenType::DqText => WordPart::Literal(sub_tok.next_token_in(Mode::DoubleQuoted)?),
                TokenType::Param | TokenType::ParamOpen => {
                    WordPart::Param(Self::param_expansion(&sub_tok, Mode::DoubleQuoted)?)
                }
                TokenType::Backquoted => {
                    WordPart::Backquoted(sub_tok.next_token_in(Mode::DoubleQuoted)?)
                }
                // `$(` is the same token in both modes, the content is parsed
                // outside quotes.
                TokenType::Andopen => WordPart::CmdSub(Self::cmdsub(&sub_tok)?),
                _ => return None,
            };
            parts.push(part);
        }
    }

    /// The reserved word `expected` (`If`, `Then`, `Fi`... in the grammar): a
    /// word that is exactly its text, unquoted.
    fn reserved_word<'a, T>(tok: &T, expected: ReservedWord) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        let word = Self::word(&sub_tok)?;
        if reserved(&word) != Some(expected) {
            return None;
        }

        tok.merge(sub_tok);

        Some(())
    }

    /// The `ASSIGNMENT_WORD` terminal (rule 7b): a word whose first part is
    /// unquoted text with an `=` after a valid name, `name=value`. The rest of
    /// the word, from after the `=`, is the value.
    pub fn assignment_word<'a, T>(tok: &T) -> Option<AssignmentWord<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();

        let word = Self::word(&sub_tok)?;
        let mut parts = word.parts.into_iter();

        let Some(WordPart::Literal(first)) = parts.next() else {
            return None;
        };
        if first.t_type != TokenType::Literal {
            return None;
        }

        // Otherwise it is an ordinary word (`=x`, `a-b=1`, `./x=1`, `X\=1`).
        let (key, value) = first.value.split_once('=')?;
        if !is_valid_name(key) {
            return None;
        }

        let mut value_parts = Vec::new();
        if !value.is_empty() {
            let offset = key.len() + 1;
            value_parts.push(WordPart::Literal(Token {
                value,
                start: first.start + offset,
                ..first
            }));
        }
        value_parts.extend(parts);

        tok.merge(sub_tok);

        Some(AssignmentWord {
            key,
            value: Word { parts: value_parts },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::UnbufferedTokenizer;

    #[test]
    fn valid_names() {
        for name in ["a", "_a1", "MY_VAR", "_", "é", "città", "ñ_2", "変数"] {
            assert!(is_valid_name(name), "{name}");
        }
        for name in ["", "1a", "a-b", "./x", "a b", "a=", "٣x", "x€"] {
            assert!(!is_valid_name(name), "{name}");
        }
    }

    /// The parameter expansion a word of one part is.
    fn expansion(input: &str) -> ParamExpansion<'_> {
        let tok = UnbufferedTokenizer::new(input);
        let word = SyntaxTree::word(&tok).expect(input);
        assert!(tok.next_is_eof(), "{input}: not all of it");

        let [WordPart::Param(expansion)] = word.parts.as_slice() else {
            panic!("{input}: {word:?}");
        };
        expansion.clone()
    }

    #[test]
    fn parameters() {
        use Parameter::*;

        for (input, param) in [
            ("$x", Name("x")),
            ("${città}", Name("città")),
            ("$1", Positional(1)),
            ("${10}", Positional(10)),
            ("$0", Special(super::Special::Zero)),
            ("${00}", Special(super::Special::Zero)),
            ("$@", Special(super::Special::At)),
            ("${#}", Special(super::Special::Hash)),
            ("$?", Special(super::Special::Question)),
            ("${-}", Special(super::Special::Hyphen)),
            ("$$", Special(super::Special::Dollar)),
            ("$!", Special(super::Special::Bang)),
            ("$*", Special(super::Special::Star)),
        ] {
            let expansion = expansion(input);
            assert_eq!(expansion.param, param, "{input}");
            assert!(expansion.op.is_none(), "{input}");
            assert_eq!(expansion.to_string(), input.replace("00", "0"), "{input}");
        }
    }

    #[test]
    fn operators_and_their_words() {
        let length = expansion("${#x}");
        assert!(matches!(length.op, Some(ParamOp::Length)));
        assert_eq!(length.param, Parameter::Name("x"));
        // `${##}` is the length of `$#`.
        assert_eq!(expansion("${##}").param, Parameter::Special(Special::Hash));

        for (input, op) in [
            ("${x-w}", "-"),
            ("${x:-w}", ":-"),
            ("${x=w}", "="),
            ("${x:=w}", ":="),
            ("${x?w}", "?"),
            ("${x:?}", ":?"),
            ("${x+w}", "+"),
            ("${x:+w}", ":+"),
            ("${x%w}", "%"),
            ("${x%%*.c}", "%%"),
            ("${x#w}", "#"),
            ("${x##*/}", "##"),
            ("${1:-a b}", ":-"),
        ] {
            let expansion = expansion(input);
            let (operator, _) = expansion.op.as_ref().unwrap().operator().unwrap();
            assert_eq!(operator, op, "{input}");
            assert_eq!(expansion.to_string(), input);
        }
    }

    /// The word inside the braces is parsed: what is inside quotes, `$(...)`
    /// or another `${...}` doesn't end it.
    #[test]
    fn the_word_of_an_operator_is_parsed() {
        for (input, parts) in [
            ("${TEST:-$(echo hello)}", 1),
            ("${x:-$(echo })}", 1),
            ("${x:-'}'}", 1),
            ("${x:-\"}\"}", 1),
            ("${x:-a\\}b}", 1),
            ("${a:-${b:-c}}", 1),
            ("${x:-a $y `z`}", 4),
            ("${x:-}", 0),
        ] {
            let expansion = expansion(input);
            let Some(ParamOp::Default { colon: true, word }) = &expansion.op else {
                panic!("{input}: {expansion:?}");
            };
            assert_eq!(word.parts.len(), parts, "{input}: {word:?}");
            assert_eq!(expansion.to_string(), input);
        }

        let Some(ParamOp::Default { word, .. }) = expansion("${TEST:-$(echo hello)}").op else {
            unreachable!();
        };
        assert!(matches!(word.parts.as_slice(), [WordPart::CmdSub(_)]));
    }

    /// Inside double quotes the word is too, but `'` is an ordinary character.
    #[test]
    fn the_word_inside_double_quotes() {
        let tok = UnbufferedTokenizer::new("\"${x:-'a'}${y:-\"b c\"}\"");
        let word = SyntaxTree::word(&tok).unwrap();
        assert!(tok.next_is_eof());

        let [WordPart::DoubleQuoted(inner)] = word.parts.as_slice() else {
            panic!("{word:?}");
        };
        let [WordPart::Param(first), WordPart::Param(second)] = inner.as_slice() else {
            panic!("{inner:?}");
        };
        let Some(ParamOp::Default { word: first, .. }) = &first.op else {
            panic!("{first:?}");
        };
        assert!(matches!(first.parts.as_slice(), [WordPart::Literal(t)] if t.value == "'a'"));
        let Some(ParamOp::Default { word: second, .. }) = &second.op else {
            panic!("{second:?}");
        };
        assert!(matches!(
            second.parts.as_slice(),
            [WordPart::DoubleQuoted(_)]
        ));
    }

    #[test]
    fn a_bad_parameter_is_a_syntax_error() {
        for input in [
            "echo ${}",
            "echo \"a${}\"",
            "echo ${a b}",
            "echo ${x!y}",
            "echo ${#x:-y}",
            "echo ${x:%y}",
            "echo ${x:-a",
            "echo ${x",
            "echo ${99999999999999999999999}",
            "if false; then echo ${x:-$(}; fi",
        ] {
            assert!(!parses(input), "{input}");
        }
        assert!(parses("if false; then echo ${x:-y}; fi"));
    }

    #[test]
    fn assignment_word_needs_a_valid_name() {
        let tok = UnbufferedTokenizer::new("MY_VAR=a\\ b");
        let word = SyntaxTree::assignment_word(&tok).unwrap();
        assert_eq!(word.key, "MY_VAR");
        assert_eq!(word.value.to_string(), "a\\ b");

        for input in ["=x", "'X'=1", "X\\=1", "a-b=1", "$X=1"] {
            let tok = UnbufferedTokenizer::new(input);
            assert!(SyntaxTree::assignment_word(&tok).is_none(), "{input}");
        }
    }

    #[test]
    fn the_value_of_an_assignment_is_a_word() {
        for (input, value, parts) in [
            ("X=a'b c'", "a'b c'", 2),
            ("X=$(c)", "$(c)", 1),
            ("X=\"$y\"z", "\"$y\"z", 2),
            ("X=", "", 0),
        ] {
            let tok = UnbufferedTokenizer::new(input);
            let word = SyntaxTree::assignment_word(&tok).expect(input);
            assert_eq!(word.key, "X");
            assert_eq!(word.value.to_string(), value, "{input}");
            assert_eq!(word.value.parts.len(), parts, "{input}");
        }
    }

    #[test]
    fn glued_parts_make_one_word() {
        let tok = UnbufferedTokenizer::new("a'b'\"c $d\"$(e)f g");
        let word = SyntaxTree::word(&tok).unwrap();

        assert_eq!(word.to_string(), "a'b'\"c $d\"$(e)f");
        assert!(matches!(
            word.parts.as_slice(),
            [
                WordPart::Literal(_),
                WordPart::SingleQuoted(_),
                WordPart::DoubleQuoted(inner),
                WordPart::CmdSub(_),
                WordPart::Literal(_),
            ] if inner.len() == 2
        ));
        assert_eq!(SyntaxTree::word(&tok).unwrap().to_string(), "g");
    }

    #[test]
    fn an_unterminated_quote_is_not_a_word() {
        for input in ["'abc", "\"abc", "a\"b"] {
            let tok = UnbufferedTokenizer::new(input);
            let word = SyntaxTree::word(&tok);
            assert!(word.is_none_or(|w| w.to_string() != input), "{input}");
        }
    }

    #[test]
    fn a_quoted_word_spans_lines() {
        let lines = vec![
            "echo 'a\n".to_string(),
            "b'\"c\n".to_string(),
            "d\"\n".to_string(),
        ];
        let tok = crate::tokenizer::BufTokenizer::from_buffer(&lines);
        let program = SyntaxTree::program(&tok).unwrap();
        assert!(tok.next_is_eof());

        let [command @ List::SequentialList(_)] = program.child.commands.as_slice() else {
            panic!("one command expected");
        };
        assert_eq!(command.to_string(), "echo 'a\nb'\"c\nd\"");
    }

    #[test]
    fn dup_targets_are_words() {
        let tok = UnbufferedTokenizer::new("2>&$(n) <&- 3>&foo");
        let redirects: Vec<_> = std::iter::from_fn(|| SyntaxTree::io_redirect(&tok)).collect();

        assert!(matches!(
            redirects.as_slice(),
            [
                Redirect::Dup(DupRedirect {
                    fd: 2,
                    output: true,
                    ..
                }),
                Redirect::Dup(DupRedirect {
                    fd: 0,
                    output: false,
                    ..
                }),
                Redirect::Dup(DupRedirect {
                    fd: 3,
                    output: true,
                    ..
                }),
            ]
        ));
        let targets: Vec<_> = redirects.iter().map(ToString::to_string).collect();
        assert_eq!(targets, ["2>&$(n)", "<&-", "3>&foo"]);
    }

    #[test]
    fn a_final_semicolon_is_not_async() {
        let lines = vec!["a;\n".to_string(), "b &\n".to_string()];
        let tok = crate::tokenizer::BufTokenizer::from_buffer(&lines);
        let program = SyntaxTree::program(&tok).unwrap();

        assert!(matches!(
            program.child.commands.as_slice(),
            [List::SequentialList(_), List::AsyncList(_)]
        ));
    }

    /// Parse a whole program and print it back.
    fn reprint(input: &str) -> String {
        let lines = vec![format!("{input}\n")];
        let tok = crate::tokenizer::BufTokenizer::from_buffer(&lines);
        let program = SyntaxTree::program(&tok).expect(input);
        assert!(tok.next_is_eof(), "{input}: not fully parsed");

        let commands: Vec<String> = program
            .child
            .commands
            .iter()
            .map(ToString::to_string)
            .collect();
        commands.join("\n")
    }

    #[test]
    fn commands_print_back_as_shell_code() {
        for input in [
            "sleep 10",
            "a | b",
            "! a | b",
            "a && b || c",
            "(a; b) >f",
            "x=1 cmd 2>&1",
            "echo $(ls) $HOME 'q w' a\\ b",
            "echo a'b'\"c $d\"$(e) `f` ${g}h ''",
            "X='a b' Y=$(c) cmd >'f g' 2>&$(n)",
            "echo * ? [x] { } € !x a#b",
            "a; b &",
            "a & b",
            "a <in >>out 3<>rw <&3 4>&- 2>err",
            "x=1 y=2",
        ] {
            assert_eq!(reprint(input), input);
        }
    }

    #[test]
    fn if_clauses_print_back() {
        for input in [
            "if a; then b; fi",
            "if a; then b; elif c; then d; else e; fi >f",
            "if if a; then b; fi; then c; fi",
            "if a & then b & fi",
            "if a; then b; fi | if c; then d; fi && e",
        ] {
            assert_eq!(reprint(input), input);
        }

        let lines = vec![
            "if a\n".to_string(),
            "then\n".to_string(),
            "  b; c\n".to_string(),
            "else d\n".to_string(),
            "fi\n".to_string(),
        ];
        let tok = crate::tokenizer::BufTokenizer::from_buffer(&lines);
        let program = SyntaxTree::program(&tok).unwrap();
        assert!(tok.next_is_eof());
        assert_eq!(
            program.child.commands[0].to_string(),
            "if a; then b; c; else d; fi"
        );
    }

    #[test]
    fn elif_is_an_else_part_with_its_own_else_part() {
        let tok = UnbufferedTokenizer::new("if a; then b; elif c; then d; else e; fi");
        let Some(Command::Compound {
            command: CompoundCommand::If(clause),
            ..
        }) = SyntaxTree::command(&tok)
        else {
            panic!("an if expected");
        };

        let Some(ElsePart::Elif(elif)) = clause.else_part.as_deref() else {
            panic!("an elif expected");
        };
        assert_eq!(elif.condition.to_string(), "c");
        assert_eq!(elif.body.to_string(), "d");
        assert!(matches!(
            elif.else_part.as_deref(),
            Some(ElsePart::Else(ElseClause { body })) if body.to_string() == "e"
        ));
    }

    #[test]
    fn for_clauses_print_back() {
        for input in [
            "for x in a b; do c; done",
            "for x; do c; done",
            "for x in; do c; done",
            "for x in a; do for y in b; do c; done; done",
            "for x in a; do b & done >f",
            "for x in $(ls) \"a b\" ${y}; do echo $x; done | cat && d",
            // Rule 5 asks for a name, a reserved word is one; rule 6 only
            // applies to the third word.
            "for do in a; do b; done",
            "for x in in do; do b; done",
        ] {
            assert_eq!(reprint(input), input);
        }
        assert_eq!(reprint("for x do c; done"), "for x; do c; done");

        for (lines, printed) in [
            (
                &["for x\n", "in a b\n", "do\n", "  c\n", "done\n"][..],
                "for x in a b; do c; done",
            ),
            (&["for x\n", "do c\n", "done\n"][..], "for x; do c; done"),
        ] {
            let lines: Vec<String> = lines.iter().map(ToString::to_string).collect();
            let tok = crate::tokenizer::BufTokenizer::from_buffer(&lines);
            let program = SyntaxTree::program(&tok).unwrap();
            assert!(tok.next_is_eof(), "{printed}");
            assert_eq!(program.child.commands[0].to_string(), printed);
        }
    }

    /// The `for` clause `input` is.
    fn for_clause(input: &str) -> ForClause<'_> {
        let tok = UnbufferedTokenizer::new(input);
        let Some(Command::Compound {
            command: CompoundCommand::For(clause),
            ..
        }) = SyntaxTree::command(&tok)
        else {
            panic!("{input}: a for expected");
        };
        assert!(tok.next_is_eof(), "{input}: not all of it");
        clause
    }

    #[test]
    fn for_clause_word_lists() {
        let clause = for_clause("for x; do a; b; done");
        assert_eq!(clause.name, "x");
        assert!(clause.wordlist.is_none());
        assert_eq!(clause.body.to_string(), "a; b");

        let clause = for_clause("for x in; do a; done");
        assert!(clause.wordlist.is_some_and(|words| words.is_empty()));

        let clause = for_clause("for città in a 'b c'; do a; done");
        assert_eq!(clause.name, "città");
        let words: Vec<String> = clause
            .wordlist
            .unwrap()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(words, ["a", "'b c'"]);
    }

    #[test]
    fn bad_for_clauses() {
        for input in [
            "for; do a; done",
            "for 1x in a; do b; done",
            "for 'x' in a; do b; done",
            "for a\"b\" in c; do d; done",
            "for x in a; do done",
            "for x in a & do b; done",
            // `do b` are words of the list: the `do` is missing.
            "for x in a do b; done",
            "for x in a; b; done",
            "for x in a; do b",
            "for x in a; do b; done done",
        ] {
            assert!(!parses(input), "{input}");
        }
    }

    #[test]
    fn case_clauses_print_back() {
        for input in [
            "case x in esac",
            "case x in a) b;; esac",
            "case $1 in a|b) c; d;; *) e;; esac",
            "case x in a) ;; esac",
            "case x in a) b &;; esac",
            "case x in \"a b\") c;; 'd'|$e) f;; esac",
            // Any word: rule 4 only makes `esac` special.
            "case in in in) a;; esac",
            "case x in if) a;; esac",
            "case x in a) case y in b) c;; esac;; esac",
            "case x in a) b;; esac >f | c",
            // The `(` is needed for `esac` to be a pattern.
            "case esac in (esac) a;; esac",
            "case esac in a|esac) b;; esac",
        ] {
            assert_eq!(reprint(input), input);
        }

        for (input, printed) in [
            ("case x in (a|b) c;; esac", "case x in a|b) c;; esac"),
            ("case x in a) b; esac", "case x in a) b;; esac"),
            ("case x in a) esac", "case x in a) ;; esac"),
        ] {
            assert_eq!(reprint(input), printed);
        }

        let lines: Vec<String> = [
            "case $x\n",
            "in\n",
            "  a)\n",
            "    b\n",
            "    ;;\n",
            "  *) c;;\n",
            "esac\n",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        let tok = crate::tokenizer::BufTokenizer::from_buffer(&lines);
        let program = SyntaxTree::program(&tok).unwrap();
        assert!(tok.next_is_eof());
        assert_eq!(
            program.child.commands[0].to_string(),
            "case $x in a) b;; *) c;; esac"
        );
    }

    #[test]
    fn case_items() {
        let tok = UnbufferedTokenizer::new("case w in a|b) c;; d) ;; esac");
        let Some(Command::Compound {
            command: CompoundCommand::Case(clause),
            ..
        }) = SyntaxTree::command(&tok)
        else {
            panic!("a case expected");
        };
        assert!(tok.next_is_eof());

        assert_eq!(clause.word.to_string(), "w");
        let [first, second] = clause.items.as_slice() else {
            panic!("two items expected: {:?}", clause.items);
        };
        let patterns: Vec<String> = first.patterns.iter().map(ToString::to_string).collect();
        assert_eq!(patterns, ["a", "b"]);
        assert_eq!(
            first.body.as_ref().map(ToString::to_string).as_deref(),
            Some("c")
        );
        assert!(second.body.is_none());
    }

    #[test]
    fn bad_case_clauses() {
        for input in [
            // Rule 4: a bare `esac` ends the case.
            "case x in esac) a;; esac",
            "case x in ) a;; esac",
            "case x in a) b;; ;; esac",
            "case x; in a) ;; esac",
            // `esac` is an argument of `d`.
            "case x in a) b;; c) d esac",
            "case x in a) b; esac esac",
            "case x in a b) c;; esac",
            "case in a) b;; esac",
            "case x in a) b",
            "case x in",
        ] {
            assert!(!parses(input), "{input}");
        }
    }

    #[test]
    fn while_and_until_print_back() {
        for input in [
            "while a; do b; done",
            "until a; do b; done",
            "while a; b; do c; d; done",
            "while a & do b & done",
            "while while a; do b; done; do c; done",
            "until a; do for x in y; do b; done; done >f",
            "while a; do b; done | until c; do d; done && e",
            "while a; do b; done &",
        ] {
            assert_eq!(reprint(input), input);
        }

        let lines: Vec<String> = ["while a\n", "do\n", "  b\n", "done\n"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let tok = crate::tokenizer::BufTokenizer::from_buffer(&lines);
        let program = SyntaxTree::program(&tok).unwrap();
        assert!(tok.next_is_eof());
        assert_eq!(program.child.commands[0].to_string(), "while a; do b; done");
    }

    #[test]
    fn while_and_until_parts() {
        let tok = UnbufferedTokenizer::new("while a; b; do c; done");
        let Some(Command::Compound {
            command: CompoundCommand::While(clause),
            ..
        }) = SyntaxTree::command(&tok)
        else {
            panic!("a while expected");
        };
        assert_eq!(clause.condition.to_string(), "a; b");
        assert_eq!(clause.body.to_string(), "c");

        let tok = UnbufferedTokenizer::new("until x; do y; done");
        let Some(Command::Compound {
            command: CompoundCommand::Until(clause),
            ..
        }) = SyntaxTree::command(&tok)
        else {
            panic!("an until expected");
        };
        assert_eq!(clause.condition.to_string(), "x");
        assert_eq!(clause.body.to_string(), "y");
    }

    #[test]
    fn bad_while_and_until() {
        for input in [
            "while; do a; done",
            "until; do a; done",
            "while a; do done",
            // `do b` are arguments of `a`: the `do` is missing.
            "while a do b; done",
            "while a; b; done",
            "until a; do b",
            "while a; do b; done done",
        ] {
            assert!(!parses(input), "{input}");
        }
    }

    /// The whole input is one program.
    fn parses(input: &str) -> bool {
        let lines = vec![format!("{input}\n")];
        let tok = crate::tokenizer::BufTokenizer::from_buffer(&lines);
        SyntaxTree::program(&tok).is_some() && tok.next_is_eof()
    }

    #[test]
    fn reserved_words_are_not_command_names() {
        for input in [
            "then x",
            "fi",
            "a; fi",
            "if a; then b",
            "if a; b; fi",
            "if; then a; fi",
            "if a; then; fi",
            "if a; then b; else; fi",
            "if a; then b; fi fi",
        ] {
            assert!(!parses(input), "{input}");
        }
    }

    #[test]
    fn reserved_words_only_unquoted_and_whole() {
        for input in [
            "echo if then fi",
            "'if' x",
            "\\fi",
            "X=1 if",
            "iffy",
            "fi'x'",
            "echo for in do done",
            "echo while until",
            "echo case in esac",
        ] {
            let tok = UnbufferedTokenizer::new(input);
            assert!(
                matches!(SyntaxTree::command(&tok), Some(Command::UnsubCommand(_))),
                "{input}"
            );
            assert!(tok.next_is_eof(), "{input}");
        }
    }

    #[test]
    fn blanks_are_normalized() {
        assert_eq!(reprint("a   |b"), "a | b");
        assert_eq!(reprint("a\tb # comment"), "a b");
        assert_eq!(reprint("0<in 1>out"), "<in >out");
    }
}
