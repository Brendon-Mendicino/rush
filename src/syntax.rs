use std::vec;

use crate::tokenizer::{Token, TokenType, Tokenizer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenKind {
    Read,
    Replace,
    Append,
    Rw,
}

#[derive(Debug, Clone)]
pub struct AssignmentWord<'a> {
    pub whole: Token<'a>,
    pub key: &'a str,
    pub value: &'a str,
}

#[derive(Debug, Clone)]
pub struct FileRedirect<'a> {
    pub redirect_fd: i32,
    pub file_kind: OpenKind,
    pub filename: Token<'a>,
}

#[derive(Debug, Clone)]
pub struct FdRedirect {
    pub fd_to_replace: i32,
    pub fd_replacer: i32,
}

#[derive(Debug, Clone)]
pub struct CloseFd {
    pub fd: i32,
}

#[derive(Debug, Clone)]
pub enum Redirect<'a> {
    FileRedirect(FileRedirect<'a>),
    FdRedirect(FdRedirect),
    CloseFd(CloseFd),
}

#[derive(Debug, Clone)]
pub enum Word<'a> {
    Substitution(Substitution<'a>),
    Token(Token<'a>),
}

#[derive(Debug, Clone)]
pub struct SimpleAssignment<'a> {
    pub redirections: Vec<Redirect<'a>>,
    pub envs: Vec<AssignmentWord<'a>>,
}

#[derive(Debug, Clone)]
pub struct SimpleCommand<'a> {
    pub program: String,
    pub arguments: Vec<String>,
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
pub enum Substitution<'a> {
    VarSub(VarSub<'a>),
    CmdSub(CmdSub<'a>),
}

#[derive(Debug, Clone)]
pub struct VarSub<'a> {
    pub token: Token<'a>,
}

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

impl<'a> AssignmentWord<'a> {
    /// The value with the backslash escapes removed (`a=x\ y` → `x y`); `value`
    /// is the raw text of the token.
    pub fn value_text(&self) -> String {
        let text = self.whole.text();

        match text.split_once('=') {
            Some((_, value)) => value.to_string(),
            None => String::new(),
        }
    }
}

/// A shell variable name: `[A-Za-z_][A-Za-z0-9_]*`.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();

    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl<'a> SimpleCommand<'a> {
    pub fn text(&self) -> String {
        let cmd = self.program.to_string();

        let total_len = cmd.len() + self.arguments.iter().map(|s| s.len()).sum::<usize>();

        let mut text = String::with_capacity(total_len);

        text.push_str(&cmd);

        for s in &self.arguments {
            text.push_str(s);
        }

        text
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

        // Advance the tokenizer if the next token is a SEMI.
        // If the next token is a SEMI, the tokenizer will be automatically
        // advanced by the function.
        if Self::separator_op(tok).is_some() {
            return Some(List::AsyncList(list.into()));
        }

        Some(List::SequentialList(list))
    }

    /// BNF:
    ///
    /// ```
    /// list ::=                and_or
    ///        | list separator and_or
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
    ///           ;
    /// ```
    pub fn command<'a, T>(tok: &T) -> Option<Command<'a>>
    where
        T: Tokenizer<'a>,
    {
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
        // TODO: modify return value with appropriate variant
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
                Redirect::FdRedirect(fd) => fd.fd_to_replace = new_redirect_fd,
                Redirect::CloseFd(close) => close.fd = new_redirect_fd,
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
            TokenType::Great => Redirect::FileRedirect(FileRedirect {
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
            // TODO: change to handle fds
            TokenType::Lessand => Self::convert_and_redirect(0, filename),
            // TODO: change to handle fds
            TokenType::Greatand => Self::convert_and_redirect(1, filename),
            _ => return None,
        };

        tok.merge(sub_tok);

        Some(redirect)
    }

    pub fn io_here<'a, T>(_tok: &T) -> Option<Redirect<'a>>
    where
        T: Tokenizer<'a>,
    {
        unimplemented!("not implemented");
    }

    /// BNF:
    ///
    /// ```
    /// filename ::= WORD
    ///            ;
    /// ```
    pub fn filename<'a, T>(tok: &T) -> Option<Token<'a>>
    where
        T: Tokenizer<'a>,
    {
        tok.next_if_type(TokenType::Word)
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
            });
        }

        None
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

    pub fn word<'a, T>(tok: &T) -> Option<Word<'a>>
    where
        T: Tokenizer<'a>,
    {
        if let Some(word) = tok.next_if_type(TokenType::Word) {
            return Some(Word::Token(word));
        }

        if let Some(quoted) = tok.next_if_type(TokenType::QuotedWord) {
            return Some(Word::Token(quoted));
        }

        if let Some(var_sub) = tok.next_if_type(TokenType::DollWord) {
            return Some(Word::Substitution(Substitution::VarSub(VarSub {
                token: var_sub,
            })));
        }

        if let Some(cmd_sub) = Self::cmdsub(tok) {
            return Some(Word::Substitution(Substitution::CmdSub(cmd_sub)));
        }

        None
    }

    pub fn assignment_word<'a, T>(tok: &T) -> Option<AssignmentWord<'a>>
    where
        T: Tokenizer<'a>,
    {
        let sub_tok = tok.clone();
        let eq = "=";

        let word = sub_tok.next_if_type(TokenType::Word)?;

        let (key, value) = word.value.split_once(eq)?;

        // TODO: shall i remove this?
        // Otherwise it is an ordinary word (`=x`, `a-b=1`, `./x=1`).
        if !is_valid_name(key) {
            return None;
        }

        tok.merge(sub_tok);

        Some(AssignmentWord {
            whole: word,
            key,
            value,
        })
    }

    pub fn convert_and_redirect<'a>(default_fd: i32, filename: Token<'a>) -> Redirect<'a> {
        assert!(!filename.value.is_empty(), "filename must not be empty!");

        if let Ok(fd) = filename.value.parse::<i32>() {
            Redirect::FdRedirect(FdRedirect {
                fd_to_replace: default_fd,
                fd_replacer: fd,
            })
        } else if filename.value == "-" {
            Redirect::CloseFd(CloseFd { fd: default_fd })
        } else {
            panic!("Redirect is not a number or dash! fd={:#?}", filename);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::UnbufferedTokenizer;

    #[test]
    fn valid_names() {
        for name in ["a", "_a1", "MY_VAR", "_"] {
            assert!(is_valid_name(name), "{name}");
        }
        for name in ["", "1a", "a-b", "./x", "é"] {
            assert!(!is_valid_name(name), "{name}");
        }
    }

    #[test]
    fn assignment_word_needs_a_valid_name() {
        let tok = UnbufferedTokenizer::new("MY_VAR=a\\ b");
        let word = SyntaxTree::assignment_word(&tok).unwrap();
        assert_eq!(word.key, "MY_VAR");
        assert_eq!(word.value_text(), "a b");

        let tok = UnbufferedTokenizer::new("=x");
        assert!(SyntaxTree::assignment_word(&tok).is_none());
    }
}
