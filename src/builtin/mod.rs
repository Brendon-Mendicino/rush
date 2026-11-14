use std::{
    collections::HashMap,
    fmt::{self, Display},
    io::{self, Read, Write},
    sync::LazyLock,
};

use clap::{Arg, ArgAction, CommandFactory};

use crate::{
    job::{JobId, JobTable},
    nix,
    shell::{self, ReadonlyError, Shell, Value, VarAttr},
    syntax,
};

// Special built-ins (POSIX §2.14).
mod r#break;
mod colon;
mod r#continue;
mod dot;
mod eval;
mod exec;
mod exit;
mod export;
mod readonly;
mod r#return;
mod set;
mod shift;
mod times;
mod trap;
mod unset;

// Regular built-ins (POSIX §1.6).
mod alias;
mod bg;
mod cd;
mod command;
mod r#false;
mod fc;
mod fg;
mod getopts;
mod hash;
mod help;
mod jobs;
mod pwd;
mod read;
mod r#true;
mod r#type;
mod ulimit;
mod umask;
mod unalias;
mod wait;

// Not registered yet: until they are implemented the external utility found in
// PATH is used, which already works for the common cases.
#[allow(dead_code)]
mod kill;
#[allow(dead_code)]
mod newgrp;

#[cfg(windows)]
const ENDL: &str = "\r\n";

#[cfg(not(windows))]
const ENDL: &str = "\n";

pub struct Builtin<'stream, 'args, 'arg, 'shell, 'jobs> {
    pub stdin: &'stream mut dyn Read,
    pub stdout: &'stream mut dyn Write,
    pub stderr: &'stream mut dyn Write,
    pub args: &'args [&'arg str],
    pub shell: &'shell mut Shell,
    /// The shell's jobs, for `jobs`, `fg` and `bg`.
    pub jobs: &'jobs mut JobTable,
}

/// How a built-in finished.
#[derive(Debug, Clone, PartialEq, Eq)]
// `Exit`, `Return`, `Break` and `Continue` are not returned by any built-in yet.
#[allow(dead_code)]
pub enum Status {
    /// It completed with this exit status, the value of `$?`.
    Exited(i32),
    /// `exit`: leave the shell, or the subshell, with this status.
    Exit(i32),
    /// `return`: leave the function or the dot script with this status.
    Return(i32),
    /// `break n`: leave the `n`th enclosing loop, `n >= 1`.
    Break(usize),
    /// `continue n`: go to the top of the `n`th enclosing loop, `n >= 1`.
    Continue(usize),
    /// `eval`: parse and run this program in the current shell environment, its
    /// status becomes the status of the built-in.
    Eval(String),
    /// `exec`: replace the shell with this command and its arguments, or, when it
    /// is empty, make the redirections of the built-in permanent.
    Exec(Vec<String>),
}

impl Status {
    pub const SUCCESS: Self = Self::Exited(0);
    pub const FAILURE: Self = Self::Exited(1);
}

/// Why a built-in failed. It is reported by [`run`], the built-in only returns it.
#[derive(Debug)]
pub enum Error {
    /// Invalid options or operands, found by clap: printed as clap formats it, with
    /// status 2. `--help` is an error for clap too, it goes to stdout with status 0.
    Usage(clap::Error),
    /// Printed as `rush: <name>: <message>` on stderr.
    Failure { status: i32, message: String },
}

impl Error {
    /// A failure with a status other than 1.
    pub fn new(status: i32, message: impl Into<String>) -> Self {
        Self::Failure {
            status,
            message: message.into(),
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(e) => write!(f, "{e}"),
            Self::Failure { message, .. } => write!(f, "{message}"),
        }
    }
}

impl From<clap::Error> for Error {
    fn from(e: clap::Error) -> Self {
        Self::Usage(e)
    }
}

/// A system call failed: the message is the one of C's `strerror`.
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::new(1, nix::error_message(&e))
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::new(1, message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::new(1, message)
    }
}

/// A change to a readonly variable: `X: readonly variable`.
impl From<ReadonlyError> for Error {
    fn from(e: ReadonlyError) -> Self {
        Self::new(1, e.to_string())
    }
}

/// Say what an io error was about: `cd: nope: No such file or directory`.
pub trait Context<T> {
    fn context(self, what: impl Display) -> Result<T, Error>;
}

impl<T> Context<T> for io::Result<T> {
    fn context(self, what: impl Display) -> Result<T, Error> {
        self.map_err(|e| Error::new(1, format!("{what}: {}", nix::error_message(&e))))
    }
}

pub type BuiltinResult = Result<Status, Error>;

/// A built-in borrows its [`Builtin`]: [`run`] still needs the streams afterwards,
/// to report the error.
pub type FnBuiltin = fn(&mut Builtin) -> BuiltinResult;

/// Run a built-in and handle its result, like Rust does with the `Result` returned
/// by `main`: an error is reported on the built-in's stderr. Stdout is flushed,
/// the built-ins don't need to.
///
/// `Ok` is how the built-in finished, whatever its status. `Err` is the status of
/// a built-in that failed with an error (an `Err` it returned, or a usage error).
/// For a special built-in that is a "special built-in utility error" (POSIX
/// §2.8.1), which exits a non-interactive shell; a diagnostic after which the
/// built-in goes on (`bi.report` and `Ok(Status::FAILURE)`) is not.
///
/// `--help` as the first argument prints the help of the built-in instead of
/// running it, like bash: also for the built-ins that don't parse their
/// arguments (`: --help`, `eval --help`).
pub fn run(f: FnBuiltin, mut bi: Builtin) -> Result<Status, i32> {
    if bi.args.get(1) == Some(&"--help")
        && let Some(mut cmd) = help_command(bi.args[0])
    {
        return match write!(bi.stdout, "{}", cmd.render_long_help()).and_then(|_| bi.stdout.flush())
        {
            Ok(()) => Ok(Status::SUCCESS),
            Err(e) => {
                let _ = bi.report(nix::error_message(&e));
                Err(1)
            }
        };
    }

    let result = f(&mut bi).and_then(|status| {
        bi.stdout.flush()?;
        Ok(status)
    });

    // When even the diagnostic cannot be written there is nothing left to do, the
    // status still tells that it failed.
    match result {
        Ok(status) => Ok(status),
        Err(Error::Usage(e)) if e.use_stderr() => {
            let _ = write!(bi.stderr, "{e}");
            Err(2)
        }
        // `--help`: not an error.
        Err(Error::Usage(e)) => {
            let _ = write!(bi.stdout, "{e}").and_then(|_| bi.stdout.flush());
            Ok(Status::SUCCESS)
        }
        Err(Error::Failure { status, message }) => {
            let _ = bi.report(message);
            Err(status)
        }
    }
}

/// Parse a job control job ID (XBD §3.204), as the clap `value_parser` of the
/// built-ins that take one: `%%`, `%+` (and a bare `%`), `%-`, `%n`, `%string`,
/// `%?string`.
fn parse_job_id(s: &str) -> Result<JobId, String> {
    let Some(id) = s.strip_prefix('%') else {
        return Err(format!("{s}: no such job"));
    };

    Ok(match id {
        "" | "%" | "+" => JobId::Current,
        "-" => JobId::Previous,
        _ if id.bytes().all(|b| b.is_ascii_digit()) => match id.parse() {
            Ok(number) => JobId::Number(number),
            Err(_) => return Err(format!("{s}: no such job")),
        },
        _ => match id.strip_prefix('?') {
            Some(part) => JobId::Contains(part.to_string()),
            None => JobId::Prefix(id.to_string()),
        },
    })
}

/// The job `id` names, or the current job: for `fg` and `bg`. The error is the
/// diagnostic.
fn find_job(jobs: &JobTable, id: Option<&JobId>) -> Result<usize, String> {
    match id {
        Some(id) => jobs.find(id).map_err(|e| format!("{id}: {e}")),
        None => jobs.current().ok_or_else(|| "no current job".to_string()),
    }
}

/// `name` can be the name of a variable.
fn check_name(name: &str) -> Result<(), Error> {
    if syntax::is_valid_name(name) {
        Ok(())
    } else {
        Err(format!("{name}: not a valid identifier").into())
    }
}

/// An operand `name[=word]` of `export` and `readonly`: the name, and the value
/// when there is one (`X=` gives an empty one).
fn declaration(arg: &str) -> Result<(&str, Option<String>), Error> {
    let (name, value) = match arg.split_once('=') {
        Some((name, value)) => (name, Some(value.to_string())),
        None => (arg, None),
    };
    check_name(name)?;
    Ok((name, value))
}

/// The listing of `export -p` and `readonly -p`: `<keyword> name='value'`, or
/// `<keyword> name` when it is unset, for the variables with the attribute,
/// sorted by name. The values are quoted for reinput.
fn print_declarations(
    bi: &mut Builtin,
    keyword: &str,
    has: impl Fn(&VarAttr) -> bool,
) -> Result<(), Error> {
    for (name, var) in bi.shell.env.iter().filter(|(_, var)| has(&var.attr)) {
        match &var.value {
            Value::Set(value) => writeln!(bi.stdout, "{keyword} {name}={}", shell::quote(value))?,
            Value::Unset => writeln!(bi.stdout, "{keyword} {name}")?,
        }
    }
    Ok(())
}

/// Body of the built-ins that are not implemented yet: report it and fail. Not an
/// error, so that a missing special built-in doesn't exit a script.
fn not_implemented(bi: &mut Builtin) -> BuiltinResult {
    bi.report("not implemented yet")?;
    Ok(Status::FAILURE)
}

/// The two kinds of built-ins (POSIX §2.14, §1.6). They differ in two ways:
/// the `VAR=x` prefixes of a special built-in stay in the shell, the ones of a
/// regular built-in last while it runs; and an error in a special built-in, or a
/// redirection error on one, exits a non-interactive shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Special,
    Regular,
}

/// A built-in of the [`BUILTINS`] table.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinDef {
    pub kind: Kind,
    pub run: FnBuiltin,
    /// The clap command of its `Args`, where its help comes from. Every
    /// built-in has one, also the ones that don't parse their arguments.
    pub command: fn() -> clap::Command,
}

/// The built-in called `name`, if any. The command search (§2.9.1) finds a
/// special built-in first, then a function, then a regular built-in, then a
/// program in PATH. There are no functions yet, so one lookup covers both kinds.
pub fn find(name: &str) -> Option<BuiltinDef> {
    BUILTINS.get(name).copied()
}

/// The clap command of the built-in called `name`, ready to render its help.
pub fn help_command(name: &str) -> Option<clap::Command> {
    find(name).map(|def| command(name, (def.command)()))
}

/// The settings every built-in's clap command shares, the POSIX Utility Syntax
/// Guidelines as far as clap goes. clap already groups flags (`-LP`), takes an
/// option-argument attached or separate (`-ofoo`, `-o foo`), ends the options
/// at `--` and takes a lone `-` as an operand. On top of that:
/// - the usage and errors show the name of the built-in, not `rush`;
/// - only `--help` prints the help: `-h` is an invalid option, as for any
///   POSIX utility that doesn't define it;
/// - an option can be repeated (`pwd -PP`), the last one wins.
///
/// TODO: what clap can't do (a workaround later, BUILTINS.md 0.7): options
/// after an operand are still options (`cd dir -P`), where POSIX takes them as
/// operands; `+x` options (`set +x`); `getopts` needs its own parser.
fn command(name: &str, cmd: clap::Command) -> clap::Command {
    cmd.name(name.to_string())
        .bin_name(name.to_string())
        .disable_help_flag(true)
        .arg(
            Arg::new("help")
                .long("help")
                .action(ArgAction::Help)
                .help("Print help"),
        )
        .args_override_self(true)
}

static BUILTINS: LazyLock<HashMap<&str, BuiltinDef>> = LazyLock::new(|| {
    type Entry = (&'static str, FnBuiltin, fn() -> clap::Command);

    let special: [Entry; _] = [
        (
            "break",
            self::r#break::r#break,
            self::r#break::Args::command,
        ),
        (":", self::colon::colon, self::colon::Args::command),
        (
            "continue",
            self::r#continue::r#continue,
            self::r#continue::Args::command,
        ),
        (".", self::dot::dot, self::dot::Args::command),
        ("eval", self::eval::eval, self::eval::Args::command),
        ("exec", self::exec::exec, self::exec::Args::command),
        ("exit", self::exit::exit, self::exit::Args::command),
        ("export", self::export::export, self::export::Args::command),
        (
            "readonly",
            self::readonly::readonly,
            self::readonly::Args::command,
        ),
        (
            "return",
            self::r#return::r#return,
            self::r#return::Args::command,
        ),
        ("set", self::set::set, self::set::Args::command),
        ("shift", self::shift::shift, self::shift::Args::command),
        ("times", self::times::times, self::times::Args::command),
        ("trap", self::trap::trap, self::trap::Args::command),
        ("unset", self::unset::unset, self::unset::Args::command),
    ];
    let regular: [Entry; _] = [
        ("alias", self::alias::alias, self::alias::Args::command),
        ("bg", self::bg::bg, self::bg::Args::command),
        ("cd", self::cd::cd, self::cd::Args::command),
        (
            "command",
            self::command::command,
            self::command::Args::command,
        ),
        (
            "false",
            self::r#false::r#false,
            self::r#false::Args::command,
        ),
        ("fc", self::fc::fc, self::fc::Args::command),
        ("fg", self::fg::fg, self::fg::Args::command),
        (
            "getopts",
            self::getopts::getopts,
            self::getopts::Args::command,
        ),
        ("hash", self::hash::hash, self::hash::Args::command),
        // Not POSIX: like bash's.
        ("help", self::help::help, self::help::Args::command),
        ("jobs", self::jobs::jobs, self::jobs::Args::command),
        ("pwd", self::pwd::pwd, self::pwd::Args::command),
        ("read", self::read::read, self::read::Args::command),
        ("true", self::r#true::r#true, self::r#true::Args::command),
        ("type", self::r#type::r#type, self::r#type::Args::command),
        ("ulimit", self::ulimit::ulimit, self::ulimit::Args::command),
        ("umask", self::umask::umask, self::umask::Args::command),
        (
            "unalias",
            self::unalias::unalias,
            self::unalias::Args::command,
        ),
        ("wait", self::wait::wait, self::wait::Args::command),
    ];

    let special = special.map(|(name, run, command)| (name, Kind::Special, run, command));
    let regular = regular.map(|(name, run, command)| (name, Kind::Regular, run, command));

    special
        .into_iter()
        .chain(regular)
        .map(|(name, kind, run, command)| (name, BuiltinDef { kind, run, command }))
        .collect()
});

impl Builtin<'_, '_, '_, '_, '_> {
    /// Parse the arguments, `args[0]` is the name of the built-in. The errors, and
    /// `--help`, are printed by [`run`].
    ///
    /// The command gets the settings shared by every built-in, see [`command`].
    pub(self) fn parse<T>(&self) -> Result<T, Error>
    where
        T: clap::Parser,
    {
        let mut matches = command(self.args[0], T::command()).try_get_matches_from(self.args)?;
        Ok(T::from_arg_matches_mut(&mut matches)?)
    }

    /// Write a diagnostic, `rush: <name>: <message>`, without stopping the built-in:
    /// for the errors after which it goes on, and fails at the end.
    pub(self) fn report(&mut self, message: impl Display) -> io::Result<()> {
        writeln!(self.stderr, "rush: {}: {message}", self.args[0])
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::shell::ShellEnv;

    /// A stdout that remembers how much of it was flushed.
    #[derive(Default)]
    struct Stdout {
        data: Vec<u8>,
        flushed: usize,
    }

    impl Write for Stdout {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.data.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushed = self.data.len();
            Ok(())
        }
    }

    struct Output {
        status: Result<Status, i32>,
        stdout: String,
        stderr: String,
        all_flushed: bool,
    }

    /// Build the `Builtin` like the executor does, and run `f` with it.
    fn run_builtin(f: FnBuiltin, args: &[&str]) -> Output {
        let mut shell = Shell {
            pgid: nix::getpgrp(),
            terminal: 0,
            is_interactive: false,
            tmodes: None,
            env: ShellEnv::default(),
        };
        // TODO: for a future plan make the stdout redirect into a pipe so the shell can control
        // how to display it to the user (when in interactive mode)
        let mut stdin = io::empty();
        let mut stdout = Stdout::default();
        let mut stderr = Vec::new();
        let mut jobs = JobTable::default();

        let status = run(
            f,
            Builtin {
                stdin: &mut stdin,
                stdout: &mut stdout,
                stderr: &mut stderr,
                args,
                shell: &mut shell,
                jobs: &mut jobs,
            },
        );

        Output {
            status,
            all_flushed: stdout.flushed == stdout.data.len(),
            stdout: String::from_utf8(stdout.data).unwrap(),
            stderr: String::from_utf8(stderr).unwrap(),
        }
    }

    #[derive(Debug, Parser)]
    struct Args {
        #[arg(short)]
        flag: bool,
    }

    fn parses(bi: &mut Builtin) -> BuiltinResult {
        let _: Args = bi.parse()?;
        Ok(Status::SUCCESS)
    }

    #[test]
    fn the_status_is_passed_through() {
        fn exited(_: &mut Builtin) -> BuiltinResult {
            Ok(Status::Exited(3))
        }
        fn exit(_: &mut Builtin) -> BuiltinResult {
            Ok(Status::Exit(4))
        }
        fn brk(_: &mut Builtin) -> BuiltinResult {
            Ok(Status::Break(2))
        }

        assert_eq!(run_builtin(exited, &["x"]).status, Ok(Status::Exited(3)));
        assert_eq!(run_builtin(exit, &["x"]).status, Ok(Status::Exit(4)));
        assert_eq!(run_builtin(brk, &["x"]).status, Ok(Status::Break(2)));
    }

    #[test]
    fn stdout_is_flushed() {
        fn prints(bi: &mut Builtin) -> BuiltinResult {
            write!(bi.stdout, "out")?;
            Ok(Status::SUCCESS)
        }

        let output = run_builtin(prints, &["x"]);

        assert_eq!(output.stdout, "out");
        assert!(output.all_flushed);
    }

    #[test]
    fn an_error_is_reported_with_status_1() {
        fn fails(_: &mut Builtin) -> BuiltinResult {
            Err("something went wrong".into())
        }

        let output = run_builtin(fails, &["name", "arg"]);

        assert_eq!(output.status, Err(1));
        assert_eq!(output.stderr, "rush: name: something went wrong\n");
        assert_eq!(output.stdout, "");
    }

    #[test]
    fn an_error_can_have_its_own_status() {
        fn not_found(_: &mut Builtin) -> BuiltinResult {
            Err(Error::new(127, "nope: not found"))
        }

        let output = run_builtin(not_found, &["name"]);

        assert_eq!(output.status, Err(127));
        assert_eq!(output.stderr, "rush: name: nope: not found\n");
    }

    #[test]
    fn io_errors_are_reported_like_strerror() {
        fn io_error(_: &mut Builtin) -> BuiltinResult {
            Err(io::Error::from_raw_os_error(libc::ENOENT))?
        }
        fn with_context(_: &mut Builtin) -> BuiltinResult {
            Err(io::Error::from_raw_os_error(libc::ENOENT)).context("dir")?
        }

        let plain = run_builtin(io_error, &["name"]);
        let context = run_builtin(with_context, &["name"]);

        assert_eq!(plain.status, Err(1));
        assert_eq!(plain.stderr, "rush: name: No such file or directory\n");
        assert_eq!(
            context.stderr,
            "rush: name: dir: No such file or directory\n"
        );
    }

    #[test]
    fn a_usage_error_has_status_2() {
        let output = run_builtin(parses, &["name", "-x"]);

        assert_eq!(output.status, Err(2));
        assert!(output.stderr.contains("unexpected argument '-x'"));
        assert_eq!(output.stdout, "");
    }

    #[test]
    fn help_goes_to_stdout_with_status_0() {
        let output = run_builtin(parses, &["name", "--help"]);

        assert_eq!(output.status, Ok(Status::SUCCESS));
        assert!(output.stdout.contains("Usage: name"));
        assert!(output.all_flushed);
        assert_eq!(output.stderr, "");
    }

    #[test]
    fn valid_arguments_are_parsed() {
        assert_eq!(
            run_builtin(parses, &["name", "-f"]).status,
            Ok(Status::SUCCESS)
        );
    }

    #[test]
    fn a_flag_can_be_repeated() {
        for args in [&["name", "-ff"][..], &["name", "-f", "-f"]] {
            assert_eq!(run_builtin(parses, args).status, Ok(Status::SUCCESS));
        }
    }

    #[test]
    fn dash_h_is_not_help() {
        let output = run_builtin(parses, &["name", "-h"]);

        assert_eq!(output.status, Err(2));
        assert!(output.stderr.contains("unexpected argument '-h'"));
        assert_eq!(output.stdout, "");
    }

    #[test]
    fn help_works_without_parsing() {
        // `:` never parses its arguments: `run` prints its help.
        let output = run_builtin(colon::colon, &[":", "--help"]);

        assert_eq!(output.status, Ok(Status::SUCCESS));
        assert!(output.stdout.contains("Usage: : [argument...]"));
        assert!(output.all_flushed);

        // Only as the first argument.
        let output = run_builtin(colon::colon, &[":", "x", "--help"]);
        assert_eq!(output.stdout, "");
    }

    #[test]
    fn every_builtin_has_a_help() {
        for name in BUILTINS.keys() {
            let mut cmd = help_command(name).unwrap();

            let about = cmd.get_about().map(ToString::to_string).unwrap_or_default();
            assert!(about.ends_with('.'), "{name}: {about:?}");

            let usage = cmd.render_usage().to_string();
            assert!(
                usage.starts_with(&format!("Usage: {name}")),
                "{name}: {usage:?}"
            );
        }
    }

    #[test]
    fn job_ids_are_parsed() {
        for (s, id) in [
            ("%", JobId::Current),
            ("%%", JobId::Current),
            ("%+", JobId::Current),
            ("%-", JobId::Previous),
            ("%12", JobId::Number(12)),
            ("%sleep", JobId::Prefix("sleep".into())),
            ("%?ee", JobId::Contains("ee".into())),
            ("%1a", JobId::Prefix("1a".into())),
        ] {
            assert_eq!(parse_job_id(s), Ok(id), "{s}");
        }

        assert_eq!(parse_job_id("1"), Err("1: no such job".into()));
        assert!(parse_job_id("%99999999999999999999999").is_err());
    }

    #[test]
    fn report_does_not_stop_the_builtin() {
        fn partial(bi: &mut Builtin) -> BuiltinResult {
            bi.report("a: bad")?;
            writeln!(bi.stdout, "b")?;
            Ok(Status::FAILURE)
        }

        let output = run_builtin(partial, &["name"]);

        assert_eq!(output.status, Ok(Status::FAILURE));
        assert_eq!(output.stderr, "rush: name: a: bad\n");
        assert_eq!(output.stdout, "b\n");
    }

    #[test]
    fn a_stub_fails_without_an_error() {
        let output = run_builtin(not_implemented, &["set", "-e"]);

        assert_eq!(output.status, Ok(Status::FAILURE));
        assert_eq!(output.stderr, "rush: set: not implemented yet\n");
    }

    #[test]
    fn builtins_have_their_kind() {
        let kind = |name| find(name).map(|def| def.kind);

        for name in [":", "eval", "exec", "exit", "export", "set", "unset", "."] {
            assert_eq!(kind(name), Some(Kind::Special), "{name}");
        }
        for name in [
            "cd", "pwd", "true", "false", "jobs", "fg", "bg", "wait", "help",
        ] {
            assert_eq!(kind(name), Some(Kind::Regular), "{name}");
        }
        assert_eq!(kind("ls"), None);
    }
}
