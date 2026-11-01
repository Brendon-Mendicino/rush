use std::{
    collections::HashMap,
    fmt::{self, Display},
    io::{self, Read, Write},
    sync::LazyLock,
};

use crate::{nix, shell::Shell};

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

pub struct Builtin<'stream, 'args, 'arg, 'shell> {
    pub stdin: &'stream mut dyn Read,
    pub stdout: &'stream mut dyn Write,
    pub stderr: &'stream mut dyn Write,
    pub args: &'args [&'arg str],
    pub shell: &'shell mut Shell,
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
/// by `main`: an error is reported on the built-in's stderr and becomes its exit
/// status. Stdout is flushed, the built-ins don't need to.
pub fn run(f: FnBuiltin, mut bi: Builtin) -> Status {
    let result = f(&mut bi).and_then(|status| {
        bi.stdout.flush()?;
        Ok(status)
    });

    // When even the diagnostic cannot be written there is nothing left to do, the
    // status still tells that it failed.
    match result {
        Ok(status) => status,
        Err(Error::Usage(e)) if e.use_stderr() => {
            let _ = write!(bi.stderr, "{e}");
            Status::Exited(2)
        }
        Err(Error::Usage(e)) => {
            let _ = write!(bi.stdout, "{e}").and_then(|_| bi.stdout.flush());
            Status::SUCCESS
        }
        Err(Error::Failure { status, message }) => {
            let _ = bi.report(message);
            Status::Exited(status)
        }
    }
}

/// Body of the built-ins that are not implemented yet: report it and fail.
fn not_implemented(_bi: &mut Builtin) -> BuiltinResult {
    Err("not implemented yet".into())
}

pub static BUILTINS: LazyLock<HashMap<&str, FnBuiltin>> = LazyLock::new(|| {
    let builtins: [(&str, FnBuiltin); _] = [
        // Special built-ins.
        ("break", self::r#break::r#break),
        (":", self::colon::colon),
        ("continue", self::r#continue::r#continue),
        (".", self::dot::dot),
        ("eval", self::eval::eval),
        ("exec", self::exec::exec),
        ("exit", self::exit::exit),
        ("export", self::export::export),
        ("readonly", self::readonly::readonly),
        ("return", self::r#return::r#return),
        ("set", self::set::set),
        ("shift", self::shift::shift),
        ("times", self::times::times),
        ("trap", self::trap::trap),
        ("unset", self::unset::unset),
        // Regular built-ins.
        ("alias", self::alias::alias),
        ("bg", self::bg::bg),
        ("cd", self::cd::cd),
        ("command", self::command::command),
        ("false", self::r#false::r#false),
        ("fc", self::fc::fc),
        ("fg", self::fg::fg),
        ("getopts", self::getopts::getopts),
        ("hash", self::hash::hash),
        ("jobs", self::jobs::jobs),
        ("pwd", self::pwd::pwd),
        ("read", self::read::read),
        ("true", self::r#true::r#true),
        ("type", self::r#type::r#type),
        ("ulimit", self::ulimit::ulimit),
        ("umask", self::umask::umask),
        ("unalias", self::unalias::unalias),
        ("wait", self::wait::wait),
    ];

    builtins.into_iter().collect()
});

impl Builtin<'_, '_, '_, '_> {
    /// Parse the arguments, `args[0]` is the name of the built-in. The errors, and
    /// `--help`, are printed by [`run`].
    pub(self) fn parse<T>(&self) -> Result<T, Error>
    where
        T: clap::Parser,
    {
        Ok(T::try_parse_from(self.args)?)
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
        status: Status,
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
        let mut stdin = io::empty();
        let mut stdout = Stdout::default();
        let mut stderr = Vec::new();

        let status = run(
            f,
            Builtin {
                stdin: &mut stdin,
                stdout: &mut stdout,
                stderr: &mut stderr,
                args,
                shell: &mut shell,
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

        assert_eq!(run_builtin(exited, &["x"]).status, Status::Exited(3));
        assert_eq!(run_builtin(exit, &["x"]).status, Status::Exit(4));
        assert_eq!(run_builtin(brk, &["x"]).status, Status::Break(2));
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

        assert_eq!(output.status, Status::Exited(1));
        assert_eq!(output.stderr, "rush: name: something went wrong\n");
        assert_eq!(output.stdout, "");
    }

    #[test]
    fn an_error_can_have_its_own_status() {
        fn not_found(_: &mut Builtin) -> BuiltinResult {
            Err(Error::new(127, "nope: not found"))
        }

        let output = run_builtin(not_found, &["name"]);

        assert_eq!(output.status, Status::Exited(127));
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

        assert_eq!(plain.status, Status::Exited(1));
        assert_eq!(plain.stderr, "rush: name: No such file or directory\n");
        assert_eq!(
            context.stderr,
            "rush: name: dir: No such file or directory\n"
        );
    }

    #[test]
    fn a_usage_error_has_status_2() {
        let output = run_builtin(parses, &["name", "-x"]);

        assert_eq!(output.status, Status::Exited(2));
        assert!(output.stderr.contains("unexpected argument '-x'"));
        assert_eq!(output.stdout, "");
    }

    #[test]
    fn help_goes_to_stdout_with_status_0() {
        let output = run_builtin(parses, &["name", "--help"]);

        assert_eq!(output.status, Status::SUCCESS);
        assert!(output.stdout.contains("Usage: name"));
        assert!(output.all_flushed);
        assert_eq!(output.stderr, "");
    }

    #[test]
    fn valid_arguments_are_parsed() {
        assert_eq!(run_builtin(parses, &["name", "-f"]).status, Status::SUCCESS);
    }

    #[test]
    fn report_does_not_stop_the_builtin() {
        fn partial(bi: &mut Builtin) -> BuiltinResult {
            bi.report("a: bad")?;
            writeln!(bi.stdout, "b")?;
            Ok(Status::FAILURE)
        }

        let output = run_builtin(partial, &["name"]);

        assert_eq!(output.status, Status::FAILURE);
        assert_eq!(output.stderr, "rush: name: a: bad\n");
        assert_eq!(output.stdout, "b\n");
    }
}
