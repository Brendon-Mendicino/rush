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
    /// `$name`, `${...}`, `$1`, `$?`...
    Param(Token<'a>),
    CmdSub(CmdSub<'a>),
    /// `` `...` ``, the token with its backquotes.
    Backquoted(Token<'a>),
}

/// A word: its parts, at least one.
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
    Subshell(Subshell<'a>),
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
    pub redirections: Vec<Redirect<'a>>,
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

/// A shell variable name: a letter or `_`, then letters, digits and `_`. Letters
/// and digits are Unicode ones, not only ASCII as in POSIX (XBD §3.235): names
/// and values are UTF-8 (`città=Roma`).
pub fn is_valid_name(name: &str) -> bool {
    // TODO: add error from this function
    let mut chars = name.chars();

    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
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
            WordPart::Literal(token) | WordPart::Param(token) | WordPart::Backquoted(token) => {
                f.write_str(token.value)
            }
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
        write!(f, "({})", self.seq_list)?;
        for redirect in &self.redirections {
            write!(f, " {redirect}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Command<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::SimpleAssignment(assignment) => write!(f, "{assignment}"),
            Command::UnsubCommand(command) => write!(f, "{command}"),
            Command::Subshell(subshell) => write!(f, "{subshell}"),
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

        if let Some(mut subshell) = Self::compound_command(tok) {
            let redirect_list = Self::redirect_list(tok);

            if let Some(redirections) = redirect_list {
                subshell.redirections = redirections;
            }

            return Some(Command::Subshell(subshell));
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
    pub fn compound_command<'a, T>(tok: &T) -> Option<Subshell<'a>>
    where
        T: Tokenizer<'a>,
    {
        // TODO: modify return value with appropriate variant, and try the other
        // alternatives once they are implemented: `Self::brace_group`,
        // `Self::for_clause`, `Self::case_clause`, `Self::if_clause`,
        // `Self::while_clause`, `Self::until_clause`.
        Self::subshell(tok)
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
            redirections: vec![],
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
    /// for_clause ::= For name                                     do_group
    ///              | For name                       sequential_sep do_group
    ///              | For name linebreak in          sequential_sep do_group
    ///              | For name linebreak in wordlist sequential_sep do_group
    ///              ;
    /// ```
    pub fn for_clause<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("for_clause: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// name ::= NAME                                   Apply rule 5
    ///        ;
    /// ```
    pub fn name<'a, T>(_tok: &T) -> Option<Token<'a>>
    where
        T: Tokenizer<'a>,
    {
        unimplemented!("name: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// in ::= In                                       Apply rule 6
    ///      ;
    /// ```
    pub fn r#in<'a, T>(_tok: &T) -> Option<Token<'a>>
    where
        T: Tokenizer<'a>,
    {
        unimplemented!("in: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// wordlist ::= wordlist WORD
    ///            |          WORD
    ///            ;
    /// ```
    pub fn wordlist<'a, T>(_tok: &T) -> Vec<Word<'a>>
    where
        T: Tokenizer<'a>,
    {
        unimplemented!("wordlist: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// case_clause ::= Case WORD linebreak in linebreak case_list    Esac
    ///               | Case WORD linebreak in linebreak case_list_ns Esac
    ///               | Case WORD linebreak in linebreak              Esac
    ///               ;
    /// ```
    pub fn case_clause<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("case_clause: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// case_list_ns ::= case_list case_item_ns
    ///                |           case_item_ns
    ///                ;
    /// ```
    pub fn case_list_ns<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("case_list_ns: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// case_list ::= case_list case_item
    ///             |           case_item
    ///             ;
    /// ```
    pub fn case_list<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("case_list: not implemented")
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
    pub fn case_item_ns<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("case_item_ns: not implemented")
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
    pub fn case_item<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("case_item: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// pattern ::=             WORD                Apply rule 4
    ///           | pattern '|' WORD                Do not apply rule 4
    ///           ;
    /// ```
    pub fn pattern<'a, T>(_tok: &T) -> Option<Vec<Word<'a>>>
    where
        T: Tokenizer<'a>,
    {
        unimplemented!("pattern: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// if_clause ::= If compound_list Then compound_list else_part Fi
    ///             | If compound_list Then compound_list           Fi
    ///             ;
    /// ```
    pub fn if_clause<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("if_clause: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// else_part ::= Elif compound_list Then compound_list
    ///             | Elif compound_list Then compound_list else_part
    ///             | Else compound_list
    ///             ;
    /// ```
    pub fn else_part<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("else_part: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// while_clause ::= While compound_list do_group
    ///                ;
    /// ```
    pub fn while_clause<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("while_clause: not implemented")
    }

    /// BNF:
    ///
    /// ```
    /// until_clause ::= Until compound_list do_group
    ///                ;
    /// ```
    pub fn until_clause<'a, T>(_tok: &T) -> Option<()>
    where
        T: Tokenizer<'a>,
    {
        // TODO: the AST node, `()` until it exists.
        unimplemented!("until_clause: not implemented")
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
    pub fn do_group<'a, T>(_tok: &T) -> Option<List<'a>>
    where
        T: Tokenizer<'a>,
    {
        unimplemented!("do_group: not implemented")
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
        Self::word(tok)
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
    pub fn sequential_sep<'a, T>(_tok: &T) -> Option<Token<'a>>
    where
        T: Tokenizer<'a>,
    {
        unimplemented!("sequential_sep: not implemented")
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
            TokenType::Param => WordPart::Param(sub_tok.next_token()?),
            TokenType::Backquoted => WordPart::Backquoted(sub_tok.next_token()?),
            TokenType::Andopen => WordPart::CmdSub(Self::cmdsub(&sub_tok)?),
            TokenType::SQuote => {
                sub_tok.next_token()?;
                let texts = std::iter::from_fn(|| {
                    sub_tok.next_if_type_in(Mode::SingleQuoted, TokenType::SqText)
                })
                .collect();
                // An unterminated quote is a syntax error.
                sub_tok.next_if_type_in(Mode::SingleQuoted, TokenType::SQuote)?;
                WordPart::SingleQuoted(texts)
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
                TokenType::Param => WordPart::Param(sub_tok.next_token_in(Mode::DoubleQuoted)?),
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
    fn blanks_are_normalized() {
        assert_eq!(reprint("a   |b"), "a | b");
        assert_eq!(reprint("a\tb # comment"), "a b");
        assert_eq!(reprint("0<in 1>out"), "<in >out");
    }
}
