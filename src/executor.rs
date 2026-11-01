use crate::{
    builtin::{self, BUILTINS, Builtin, FnBuiltin, Status},
    job::{Job, JobBuilder, JobExit},
    nix::{self, ForkResult, Pid, SigHandler, Signal, close, dup2_raw},
    shell::{JOB_CONTROL_SIGNALS, Shell},
    syntax::*,
    tokenizer::{BufTokenizer, TokenType, Tokenizer, UnbufferedTokenizer},
    waiter::{WaitResult, Waiter},
};
use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::{fs::OpenOptionsExt, process::CommandExt},
    },
    panic::{self, AssertUnwindSafe},
    process::{self, Stdio},
};

const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

#[derive(Debug, Clone, Copy, Default)]
pub struct TerminalState {
    terminate_session: bool,
    needs_more: bool,
    exit_code: i32,
}

/// Outcome of running a piece of the syntax tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecStatus {
    Exited(i32),
    Signaled(Signal),
    /// The job got stopped and was moved to the background jobs.
    Stopped(Signal),
}

impl ExecStatus {
    pub const SUCCESS: Self = Self::Exited(0);
    pub const FAILURE: Self = Self::Exited(1);
    /// Status of input that could not be parsed, same as other shells.
    pub const SYNTAX_ERROR: Self = Self::Exited(2);

    /// Value for `$?`, following the shell convention of `128 + signum` for signals.
    pub fn code(self) -> i32 {
        match self {
            Self::Exited(code) => code,
            Self::Signaled(sig) | Self::Stopped(sig) => 128 + sig as i32,
        }
    }

    /// Negates the output of the command
    pub fn not(self) -> ExecStatus {
        match self {
            // Keep the interruption visible to `&&` and `||`.
            status if status.interrupted() => status,
            status if status.failure() => Self::SUCCESS,
            _ => Self::FAILURE,
        }
    }

    /// The command was interrupted (Ctrl-C): the rest of the and-or list must
    /// not run.
    pub fn interrupted(self) -> bool {
        matches!(self, Self::Signaled(Signal::SIGINT))
    }

    /// Indicates whether the command exit is considered a failure
    pub fn failure(&self) -> bool {
        match self {
            Self::Stopped(_) | Self::Signaled(_) => true,
            Self::Exited(0) => false,
            Self::Exited(_) => true,
        }
    }
}

impl From<JobExit> for ExecStatus {
    fn from(exit: JobExit) -> Self {
        match exit {
            JobExit::Exited(code) => Self::Exited(code),
            JobExit::Signaled(sig) => Self::Signaled(sig),
        }
    }
}

/// Report a command that could not be spawned. The status follows other shells:
/// 127 when the program does not exist, 126 when it cannot be executed.
fn spawn_failure(program: &str, err: &io::Error) -> JobExit {
    let message = nix::error_message(err);

    match err.kind() {
        io::ErrorKind::NotFound if !program.contains('/') => {
            eprintln!("rush: {program}: command not found");
            JobExit::Exited(127)
        }
        io::ErrorKind::NotFound => {
            eprintln!("rush: {program}: {message}");
            JobExit::Exited(127)
        }
        io::ErrorKind::PermissionDenied => {
            eprintln!("rush: {program}: {message}");
            JobExit::Exited(126)
        }
        _ => {
            eprintln!("rush: {program}: {message}");
            JobExit::Exited(1)
        }
    }
}

/// Lines keep their newline: a continuation line ends with "\\\n" (or "\\\r\n").
/// Cut it and paste the next line on it.
fn join_continuations(input: &[String]) -> Vec<String> {
    let mut lines = Vec::<String>::new();

    for line in input {
        let continued = lines.last_mut().and_then(|previous| {
            let cut = previous
                .strip_suffix("\\\n")
                .or_else(|| previous.strip_suffix("\\\r\n"))?
                .len();
            Some((previous, cut))
        });

        match continued {
            Some((previous, cut)) => {
                previous.truncate(cut);
                previous.push_str(line);
            }
            None => lines.push(line.clone()),
        }
    }

    lines
}

#[derive(Debug)]
pub struct Executor {
    input_buffer: Vec<String>,
    shell: Shell,
    /// Jobs that got stopped or that are running in background.
    bg_jobs: Vec<Job>,
}

struct PipelineState {
    /// Input of the command: the output of the previous command of the pipeline.
    stdin: Option<OwnedFd>,
    /// Output of the command, when it is not the shell's (`$(...)`). Inside a
    /// pipeline the output is a new pipe to the next command instead.
    stdout: Option<OwnedFd>,
    inside_pipeline: bool,
    /// The job gets the terminal (interactive only).
    is_foreground: bool,
    pipeline_pgid: Option<Pid>,
}

/// What a redirection does in the child.
#[derive(Debug, Clone, Copy)]
enum RedirectOp {
    /// `dup2(src, dst)`
    Dup {
        src: RawFd,
        dst: RawFd,
    },
    Close(RawFd),
}

/// A redirection that cannot be set up; the command is not run.
#[derive(Debug)]
enum RedirectError {
    Open { filename: String, err: io::Error },
    BadFd(RawFd),
}

impl fmt::Display for RedirectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { filename, err } => {
                write!(f, "{filename}: {}", nix::error_message(err))
            }
            Self::BadFd(fd) => write!(f, "{fd}: Bad file descriptor"),
        }
    }
}

/// Prepares the redirections of a command in the parent: opens the files and
/// checks the fds, so that errors are reported before spawning.
#[derive(Debug, Default)]
struct RedirectController {
    /// Files opened for the redirections. Dropped, and so closed in the parent,
    /// once the child is spawned.
    files: Vec<OwnedFd>,
    /// The redirections in the order they are written, which is the order they
    /// are applied in: `2>&1 >file` and `>file 2>&1` differ.
    ops: Vec<RedirectOp>,
    /// Fds opened (`true`) or closed (`false`) by the redirections seen so far.
    redirected: HashMap<RawFd, bool>,
}

#[derive(Debug)]
struct RedirectionApplyer {
    ops: Vec<RedirectOp>,
}

/// Outcome of starting a command of a pipeline.
#[derive(Debug)]
enum CommandRun {
    /// A child process, to be waited as part of the job.
    Spawned(Spawned),
    /// The command finished without a process: it ran in the shell (an
    /// assignment), or it could not be started (the error is already printed).
    Done(JobExit),
}

/// A spawned command and the process group it ended up in.
#[derive(Debug)]
struct Spawned {
    pid: Pid,
    pgid: Pid,
    /// Read end of the pipe the command writes to, inside a pipeline.
    stdout: Option<OwnedFd>,
}

/// What a forked shell runs, which decides how it treats the terminal and the
/// signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpawnType {
    /// `( list )` and `$(list)`: part of the job it belongs to, like a command.
    /// It gets the default signals, so Ctrl-C and Ctrl-Z act on it as well.
    Subshell,
    /// `list &`: never takes the terminal. Without job control it ignores
    /// SIGINT and SIGQUIT and reads from /dev/null (POSIX).
    Async,
}

/// `/dev/null`, the input of a command that must not read the shell's.
fn dev_null() -> Option<OwnedFd> {
    File::open("/dev/null").map(OwnedFd::from).ok()
}

#[derive(Clone)]
struct ApplyerPtr(*const RedirectionApplyer);

unsafe impl Send for ApplyerPtr {}
unsafe impl Sync for ApplyerPtr {}

impl RedirectController {
    fn add_redirect(&mut self, redirect: &Redirect) -> Result<(), RedirectError> {
        match redirect {
            Redirect::FileRedirect(file_redirect) => {
                let file = Self::open(file_redirect)?;
                let dst = file_redirect.redirect_fd;

                self.ops.push(RedirectOp::Dup {
                    src: file.as_raw_fd(),
                    dst,
                });
                self.files.push(file);
                self.redirected.insert(dst, true);
            }
            Redirect::FdRedirect(dup_fd) => {
                let src = dup_fd.fd_replacer;

                // Either opened by an earlier redirection of this command, or
                // inherited from the shell.
                let is_open = self
                    .redirected
                    .get(&src)
                    .copied()
                    .unwrap_or_else(|| nix::is_valid_fd(src));
                if !is_open {
                    return Err(RedirectError::BadFd(src));
                }

                self.ops.push(RedirectOp::Dup {
                    src,
                    dst: dup_fd.fd_to_replace,
                });
                self.redirected.insert(dup_fd.fd_to_replace, true);
            }
            Redirect::CloseFd(close_fd) => {
                self.ops.push(RedirectOp::Close(close_fd.fd));
                self.redirected.insert(close_fd.fd, false);
            }
        }

        Ok(())
    }

    fn open(redirect: &FileRedirect) -> Result<OwnedFd, RedirectError> {
        let mut options = File::options();

        match redirect.file_kind {
            OpenKind::Read => options.read(true),
            OpenKind::Replace => options.create(true).write(true).truncate(true),
            OpenKind::Append => options.create(true).append(true),
            OpenKind::Rw => options.create(true).read(true).write(true),
        };
        // rw-rw-r--, before the umask
        options.mode(0o664);

        let filename = redirect.filename.text();
        let file = options
            .open(&filename)
            .map_err(|err| RedirectError::Open { filename, err })?;

        // Move the file out of the low fds that redirections usually target:
        // otherwise a redirection could overwrite a file that a later one still
        // has to dup (`4>a 3>b` with `a` opened on 3). The copy is close-on-exec
        // like the original, only the dup2'd fds reach the program.
        match nix::dup_cloexec(file.as_raw_fd(), 10) {
            Ok(fd) => Ok(unsafe { OwnedFd::from_raw_fd(fd) }),
            Err(_) => Ok(file.into()),
        }
    }

    fn get_applyer(&self) -> RedirectionApplyer {
        RedirectionApplyer {
            ops: self.ops.clone(),
        }
    }

    fn add_redirects(&mut self, redirections: &[Redirect]) -> Result<(), RedirectError> {
        for redirect in redirections {
            self.add_redirect(redirect)?;
        }

        Ok(())
    }
}

impl RedirectionApplyer {
    /// Runs in the child, between fork and exec: async-signal-safe calls only.
    fn apply(&self) -> io::Result<()> {
        for op in &self.ops {
            match *op {
                // dup2 does nothing when the fds are the same, and the fd would
                // keep its close-on-exec flag.
                RedirectOp::Dup { src, dst } if src == dst => nix::set_cloexec(src, false)?,
                RedirectOp::Dup { src, dst } => {
                    dup2_raw(src, dst)?;
                }
                // Closing an fd that is not open is not an error (`3>&-`).
                RedirectOp::Close(fd) => match close(fd) {
                    Err(e) if e.raw_os_error() == Some(libc::EBADF) => {}
                    res => res?,
                },
            }
        }

        Ok(())
    }
}

impl ApplyerPtr {
    unsafe fn apply(&self) -> io::Result<()> {
        unsafe { (*self.0).apply() }
    }
}

// TODO: check this, maybe add a drop trait
/// Copies of the fds that redirections are about to replace in the shell itself,
/// to put them back afterwards.
struct SavedFds(Vec<(RawFd, Option<OwnedFd>)>);

impl SavedFds {
    /// Save every fd the redirections touch, `None` for the ones that are not open.
    fn save(redirections: &RedirectController) -> io::Result<Self> {
        let mut saved = Vec::new();

        for &fd in redirections.redirected.keys() {
            let copy = if nix::is_valid_fd(fd) {
                let copy = nix::dup_cloexec(fd, 10)?;
                Some(unsafe { OwnedFd::from_raw_fd(copy) })
            } else {
                None
            };
            saved.push((fd, copy));
        }

        Ok(Self(saved))
    }

    /// Put the fds back as they were. Nothing more can be done when it fails.
    fn restore(self) {
        for (fd, copy) in self.0 {
            let _ = match copy {
                Some(copy) => dup2_raw(copy.as_raw_fd(), fd).map(drop),
                None => close(fd),
            };
        }
    }
}

impl Executor {
    pub fn new() -> Self {
        Self {
            input_buffer: Vec::new(),
            shell: Shell::new(),
            bg_jobs: Vec::new(),
        }
    }

    fn builtin_from_simple_command(
        &mut self,
        cmd: &SimpleCommand,
        fn_builtin: FnBuiltin,
        in_pipeline: bool,
    ) -> CommandRun {
        let mut redirect_controller = RedirectController::default();

        if let Err(e) = redirect_controller.add_redirects(&cmd.redirections) {
            eprintln!("rush: {e}");
            return CommandRun::Done(JobExit::Exited(1));
        }

        let mut stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        let mut stderr = std::io::stderr();

        let prog = vec![cmd.program.as_str()];
        let prog_args = cmd.arguments.iter().map(|s| s.as_str()).collect();
        let args = vec![prog, prog_args].concat();

        let builtin = Builtin {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
            args: &args,
            shell: &mut self.shell,
        };

        let exit = match builtin::run(fn_builtin, builtin) {
            Status::Exited(code) => JobExit::Exited(code),
            Status::Eval(program) => match self.execute_str(&program) {
                ExecStatus::Exited(code) => JobExit::Exited(code),
                // Keeps an interruption visible to the and-or list around `eval`.
                ExecStatus::Signaled(sig) => JobExit::Signaled(sig),
                // The job is already in the background jobs.
                status @ ExecStatus::Stopped(_) => JobExit::Exited(status.code()),
            },
            Status::Exec(argv) => self.exec(cmd, &argv, &redirect_controller, in_pipeline),
            // TODO: unwind the loops, functions and the shell. No built-in returns
            // these yet.
            Status::Exit(code) | Status::Return(code) => JobExit::Exited(code),
            Status::Break(_) | Status::Continue(_) => JobExit::Exited(0),
        };

        CommandRun::Done(exit)
    }

    /// Spawn an external command. When it cannot be spawned the error is reported
    /// and the command's status is returned instead.
    fn simple_command(&self, cmd: &SimpleCommand, state: PipelineState) -> CommandRun {
        let mut redirect_controller = RedirectController::default();

        if let Err(e) = redirect_controller.add_redirects(&cmd.redirections) {
            eprintln!("rush: {e}");
            return CommandRun::Done(JobExit::Exited(1));
        }

        let mut command = process::Command::new(&cmd.program);
        command.args(&cmd.arguments);

        self.set_command_env(&mut command, &cmd.envs);

        // An interactive shell puts every job in its own process group, the first
        // command of the pipeline becomes the leader (0 = use its own pid).
        // `spawn()` returns only after the child exec'd, so the group is already
        // set when the parent continues: no setpgid race with the child.
        if self.shell.is_interactive {
            command.process_group(state.pipeline_pgid.map_or(0, Pid::as_raw));

            let terminal = self.shell.terminal;
            let is_foreground = state.is_foreground;

            // Registered before the redirections, which could replace the fds.
            // SAFETY: only async-signal-safe syscalls, no allocations.
            unsafe {
                command.pre_exec(move || {
                    // std calls setpgid for `process_group` before running the
                    // closures, so the child is already in the job's group. The
                    // terminal is taken here, before exec: once exec'd the program
                    // could read it before the parent hands it over, and would get
                    // stopped by SIGTTIN. SIGTTOU is still ignored, which lets a
                    // background group call tcsetpgrp.
                    if is_foreground {
                        nix::tcsetpgrp(terminal, nix::getpgrp())?;
                    }

                    // Ignored signals stay ignored across exec: undo the shell's.
                    for sig in JOB_CONTROL_SIGNALS {
                        nix::signal(sig, SigHandler::Default)?;
                    }

                    Ok(())
                });
            }
        }

        if let Some(stdin) = state.stdin {
            command.stdin(Stdio::from(stdin));
        }

        // We use this to chain the processes in a pipeline
        if state.inside_pipeline {
            command.stdout(Stdio::piped());
        } else if let Some(stdout) = state.stdout {
            command.stdout(Stdio::from(stdout));
        }

        let applyer = Box::new(redirect_controller.get_applyer());

        let applyer_ptr = ApplyerPtr(Box::into_raw(applyer));
        let ptr_clone = applyer_ptr.clone();

        unsafe {
            command.pre_exec(move || {
                ptr_clone.apply()?;
                Ok(())
            });
        }

        let spawn_result = command.spawn();

        unsafe {
            // I am sure that leaked_applyer can be freed now because the fork
            // has been called and there is no need to wait for the closure
            // because it will execute in the child context.
            drop(Box::from_raw(applyer_ptr.0 as *mut RedirectionApplyer));
        }

        // The child has its own copies of the files now.
        drop(redirect_controller);

        let mut child = match spawn_result {
            Ok(child) => child,
            Err(e) => return CommandRun::Done(spawn_failure(&cmd.program, &e)),
        };
        let pid = Pid::from_raw(child.id() as i32);

        // A non-interactive shell does no job control: the children stay in the
        // shell's process group. Otherwise the first child of the pipeline leads
        // the group with its pid.
        let pgid = if self.shell.is_interactive {
            state.pipeline_pgid.unwrap_or(pid)
        } else {
            self.shell.pgid
        };

        // Dropping the `Child` neither waits nor kills: the job reaps it by pid.
        CommandRun::Spawned(Spawned {
            pid,
            pgid,
            stdout: child.stdout.take().map(OwnedFd::from),
        })
    }

    /// The environment of a program: the exported shell variables plus the
    /// command's `VAR=x` prefixes. `env` keeps the last value of a key, so a prefix
    /// overrides a shell variable and the last prefix wins. The program is looked up
    /// in this `PATH`.
    fn set_command_env(&self, command: &mut process::Command, assignments: &[AssignmentWord]) {
        command.env_clear();
        command.envs(self.shell.env.exported());
        for assignment in assignments {
            command.env(assignment.key, assignment.value_text());
        }
    }

    // TODO: check this
    /// `exec`: without a command, make the redirections permanent in the shell;
    /// otherwise replace the shell with the command. Returns only when the command
    /// could not be run, after putting the shell back as it was.
    fn exec(
        &self,
        cmd: &SimpleCommand,
        argv: &[String],
        redirections: &RedirectController,
        in_pipeline: bool,
    ) -> JobExit {
        // Built-ins don't get a process of their own in a pipeline yet: the whole
        // shell would be replaced.
        if in_pipeline {
            eprintln!("rush: exec: not supported in a pipeline yet");
            return JobExit::Exited(1);
        }

        // What the shell already wrote goes where it was meant to.
        let _ = io::stdout().flush();

        let Some((program, arguments)) = argv.split_first() else {
            return match redirections.get_applyer().apply() {
                Ok(()) => JobExit::Exited(0),
                Err(e) => {
                    eprintln!("rush: exec: {}", nix::error_message(&e));
                    JobExit::Exited(1)
                }
            };
        };

        let saved = match SavedFds::save(redirections) {
            Ok(saved) => saved,
            Err(e) => {
                eprintln!("rush: exec: {}", nix::error_message(&e));
                return JobExit::Exited(1);
            }
        };
        if let Err(e) = redirections.get_applyer().apply() {
            saved.restore();
            eprintln!("rush: exec: {}", nix::error_message(&e));
            return JobExit::Exited(1);
        }

        let mut command = process::Command::new(program);
        command.args(arguments);
        self.set_command_env(&mut command, &cmd.envs);

        // The program gets the default handling of the signals the shell ignores,
        // it keeps the shell's process group and terminal.
        self.set_job_control_signals(SigHandler::Default);

        let err = command.exec();

        self.set_job_control_signals(SigHandler::Ignore);
        saved.restore();

        spawn_failure(program, &err)
    }

    /// Set the handling of the signals an interactive shell ignores.
    fn set_job_control_signals(&self, handler: SigHandler) {
        if !self.shell.is_interactive {
            return;
        }

        for sig in JOB_CONTROL_SIGNALS {
            if let Err(e) = nix::signal(sig, handler) {
                eprintln!("signal({sig:?}): {e}");
            }
        }
    }

    /// Fork a shell that runs `body` (shell code, not a program) and exits with
    /// its status: a subshell, an async list, a command substitution.
    ///
    /// The child is a copy of the shell, so whatever `body` changes (variables,
    /// later the cwd) stays in the child. It runs without job control: its
    /// commands stay in its process group and it never touches the terminal.
    fn fork_child(
        &mut self,
        spawn_type: SpawnType,
        state: PipelineState,
        redirections: &[Redirect],
        body: impl FnOnce(&mut Self) -> ExecStatus,
    ) -> CommandRun {
        // TODO: check again if this function can be simplified in the future, maybe using more
        // idiomatinc rust and wrapping the libc calls to rust structs
        let PipelineState {
            stdin,
            stdout,
            inside_pipeline,
            is_foreground,
            pipeline_pgid,
        } = state;

        let mut redirect_controller = RedirectController::default();
        if let Err(e) = redirect_controller.add_redirects(redirections) {
            eprintln!("rush: {e}");
            return CommandRun::Done(JobExit::Exited(1));
        }

        // Inside a pipeline the child writes to a new pipe, whose read end is the
        // input of the next command.
        let (next_stdin, stdout) = if inside_pipeline {
            match io::pipe() {
                Ok((reader, writer)) => (Some(OwnedFd::from(reader)), Some(OwnedFd::from(writer))),
                Err(e) => {
                    eprintln!("rush: pipe: {}", nix::error_message(&e));
                    return CommandRun::Done(JobExit::Exited(1));
                }
            }
        } else {
            (None, stdout)
        };

        // Anything still buffered would be written by both processes.
        let _ = io::stdout().flush();

        // SAFETY: the shell is single-threaded, see `nix::fork`.
        let pid = match unsafe { nix::fork() } {
            Ok(ForkResult::Child) => {
                // The read end belongs to the next command.
                drop(next_stdin);
                self.enter_child(
                    spawn_type,
                    is_foreground,
                    pipeline_pgid,
                    stdin,
                    stdout,
                    redirect_controller,
                );

                // The child must never get back to the caller: it would go on
                // running the parent's code (and read its input).
                let status = panic::catch_unwind(AssertUnwindSafe(|| body(self)))
                    .unwrap_or(ExecStatus::FAILURE);

                let _ = io::stdout().flush();
                nix::_exit(status.code());
            }
            Ok(ForkResult::Parent(pid)) => pid,
            Err(e) => {
                eprintln!("rush: fork: {}", nix::error_message(&e));
                return CommandRun::Done(JobExit::Exited(1));
            }
        };

        // The parent's copies of `stdin`, `stdout` and the redirected files are
        // closed when they go out of scope: only the child needs them.

        let pgid = if self.shell.is_interactive {
            let pgid = pipeline_pgid.unwrap_or(pid);

            // Unlike `Command::spawn`, `fork` returns before the child ran: it may
            // not be in its group yet. Both sides set it (GNU libc manual).
            match nix::setpgid(pid, pgid) {
                // The child already exited: it set its group itself.
                Err(e) if e.raw_os_error() == Some(libc::ESRCH) => {}
                Err(e) => eprintln!("rush: setpgid({}, {}): {e}", pid.as_raw(), pgid.as_raw()),
                Ok(()) => {}
            }

            if is_foreground
                && spawn_type != SpawnType::Async
                && let Err(e) = nix::tcsetpgrp(self.shell.terminal, pgid)
            {
                eprintln!("rush: tcsetpgrp({}): {e}", pgid.as_raw());
            }

            pgid
        } else {
            self.shell.pgid
        };

        CommandRun::Spawned(Spawned {
            pid,
            pgid,
            stdout: next_stdin,
        })
    }

    /// Set up a forked child before it runs its shell code.
    fn enter_child(
        &mut self,
        spawn_type: SpawnType,
        is_foreground: bool,
        pipeline_pgid: Option<Pid>,
        stdin: Option<OwnedFd>,
        stdout: Option<OwnedFd>,
        redirect_controller: RedirectController,
    ) {
        if self.shell.is_interactive {
            // Same as a command's `pre_exec`: join the job's group, take the
            // terminal while SIGTTOU is still ignored, restore the signals.
            let _ = nix::setpgid(Pid::from_raw(0), pipeline_pgid.unwrap_or(Pid::from_raw(0)));

            if is_foreground && spawn_type != SpawnType::Async {
                let _ = nix::tcsetpgrp(self.shell.terminal, nix::getpgrp());
            }

            for sig in JOB_CONTROL_SIGNALS {
                let _ = nix::signal(sig, SigHandler::Default);
            }
        } else if spawn_type == SpawnType::Async {
            // Without job control Ctrl-C reaches every process of the shell's
            // group: the background ones must not die with the foreground job.
            for sig in [Signal::SIGINT, Signal::SIGQUIT] {
                let _ = nix::signal(sig, SigHandler::Ignore);
            }
        }

        let stdio = [(stdin, libc::STDIN_FILENO), (stdout, libc::STDOUT_FILENO)];
        for (fd, target) in stdio {
            if let Some(fd) = fd
                && let Err(e) = dup2_raw(fd.as_raw_fd(), target)
            {
                eprintln!("rush: dup2: {}", nix::error_message(&e));
                nix::_exit(1);
            }
        }

        if let Err(e) = redirect_controller.get_applyer().apply() {
            eprintln!("rush: {}", nix::error_message(&e));
            nix::_exit(1);
        }
        // Close the original files: the child does not exec, close-on-exec
        // would never kick in.
        drop(redirect_controller);

        // The child is a shell without job control: its commands stay in its
        // process group (the job's), it never touches the terminal, and stops
        // are not reported. The parent's jobs are not its children.
        self.shell.is_interactive = false;
        self.shell.pgid = nix::getpgrp();
        self.bg_jobs.clear();
    }

    /// `( list )`: run the list in a forked shell.
    fn subshell(&mut self, subshell: &Subshell, state: PipelineState) -> CommandRun {
        self.fork_child(
            SpawnType::Subshell,
            state,
            &subshell.redirections,
            |executor| executor.list(&subshell.seq_list),
        )
    }

    /// `list &`: run the list in a forked shell without waiting for it.
    fn async_list(&mut self, op_list: &OpList) -> ExecStatus {
        let state = PipelineState {
            // POSIX: without job control an async list reads from /dev/null.
            stdin: (!self.shell.is_interactive).then(dev_null).flatten(),
            stdout: None,
            inside_pipeline: false,
            is_foreground: false,
            pipeline_pgid: None,
        };

        match self.fork_child(SpawnType::Async, state, &[], |executor| {
            executor.op_list(op_list)
        }) {
            CommandRun::Spawned(Spawned { pid, pgid, .. }) => {
                eprintln!("{}: Background", pid.as_raw());
                self.bg_jobs.push(JobBuilder::new().build(pgid, pid));
                ExecStatus::SUCCESS
            }
            CommandRun::Done(exit) => exit.into(),
        }
    }

    /// `$(list)`: the output of the list run in a forked shell, without the
    /// trailing newlines.
    fn cmdsub(&mut self, cmdsub: &CmdSub) -> String {
        let (mut reader, writer) = match io::pipe() {
            Ok(pipe) => pipe,
            Err(e) => {
                eprintln!("rush: pipe: {}", nix::error_message(&e));
                return String::new();
            }
        };

        let state = PipelineState {
            stdin: None,
            stdout: Some(writer.into()),
            inside_pipeline: false,
            is_foreground: true,
            pipeline_pgid: None,
        };

        // The write end is moved into the child's state: once `fork_child`
        // returns the parent has no copy left, and the read gets EOF when the
        // child is done.
        let run = self.fork_child(SpawnType::Subshell, state, &[], |executor| {
            executor.list(&cmdsub.seq_list)
        });

        // Read everything before waiting: a child blocked on a full pipe would
        // never exit.
        let mut output = Vec::new();
        if let Err(e) = reader.read_to_end(&mut output) {
            eprintln!("rush: read: {}", nix::error_message(&e));
        }
        drop(reader);

        if let CommandRun::Spawned(Spawned { pid, pgid, .. }) = run {
            let job = JobBuilder::new().build(pgid, pid);

            match Waiter::new(&self.shell).wait(job) {
                Ok(WaitResult::Completed(_)) => {}
                Ok(WaitResult::Stopped { job, .. }) => self.bg_jobs.push(job),
                Err(e) => eprintln!("rush: wait: {e}"),
            }
        }

        let output = String::from_utf8_lossy(&output);
        output.trim_end_matches('\n').to_string()
    }

    /// The text of a word, after substitutions.
    fn expand_word(&mut self, word: &Word) -> String {
        match word {
            Word::Token(token) => token.text(),
            Word::Substitution(Substitution::CmdSub(cmdsub)) => self.cmdsub(cmdsub),
            Word::Substitution(Substitution::VarSub(_)) => todo!(),
        }
    }

    fn unsub_command(&mut self, cmd: &UnsubCommand, state: PipelineState) -> CommandRun {
        let program = self.expand_word(&cmd.program);

        let arguments = cmd
            .arguments
            .iter()
            .map(|word| self.expand_word(word))
            .collect::<Vec<_>>();

        let expanded = SimpleCommand {
            program,
            arguments,
            redirections: cmd.redirections.clone(),
            envs: cmd.envs.clone(),
        };

        // TODO: check this
        match BUILTINS.get(expanded.program.as_str()) {
            Some(builtin) => {
                // A pipeline of more than one command: the first ones write into a
                // pipe, the others read from the previous command.
                let in_pipeline = state.inside_pipeline || state.stdin.is_some();
                self.builtin_from_simple_command(&expanded, *builtin, in_pipeline)
            }
            None => self.simple_command(&expanded, state),
        }
    }

    /// `a=1 b=2` with no command: set the shell variables.
    fn simple_assignment(
        &mut self,
        assignment: &SimpleAssignment,
        state: PipelineState,
    ) -> CommandRun {
        // The redirections are still performed (`a=1 >file` creates the file),
        // and when they fail the variables are left untouched. The files are
        // closed when the controller is dropped.
        let mut redirect_controller = RedirectController::default();
        if let Err(e) = redirect_controller.add_redirects(&assignment.redirections) {
            eprintln!("rush: {e}");
            return CommandRun::Done(JobExit::Exited(1));
        }

        // Each command of a pipeline runs in its own process: the assignment
        // would only change a child's variables, which are thrown away.
        if state.inside_pipeline {
            return CommandRun::Done(JobExit::Exited(0));
        }

        // `None` keeps the attributes of an existing variable: assigning an
        // exported variable changes the environment of the next commands.
        for var in &assignment.envs {
            self.shell.env.upsert(var.key, var.value_text(), None);
        }

        CommandRun::Done(JobExit::Exited(0))
    }

    fn command(&mut self, command: &Command, state: PipelineState) -> CommandRun {
        match command {
            Command::SimpleAssignment(assignment) => self.simple_assignment(assignment, state),
            Command::UnsubCommand(command) => self.unsub_command(command, state),
            Command::Subshell(subshell) => self.subshell(subshell, state),
        }
    }

    fn schedule_pipeline(&mut self, pipeline: &Pipeline, is_foreground: bool) -> Job {
        assert!(
            !pipeline.cmds.is_empty(),
            "A pipeline must always contain something"
        );

        // Decided by the first spawned command, the others join its group.
        let mut pipeline_pgid = None;
        let mut stdin = None;
        let mut job_builder = JobBuilder::new();

        let (last_cmd, inner_cmds) = pipeline.cmds.split_last().unwrap();

        for cmd in inner_cmds {
            let state = PipelineState {
                stdin: stdin.take(),
                stdout: None,
                inside_pipeline: true,
                is_foreground,
                pipeline_pgid,
            };

            stdin = match self.command(cmd, state) {
                CommandRun::Spawned(Spawned { pid, pgid, stdout }) => {
                    pipeline_pgid = Some(pgid);
                    job_builder.add_job(pid);
                    stdout
                }
                // The next command reads from a command that wrote nothing. It must
                // not inherit the shell's stdin.
                CommandRun::Done(_) => dev_null(),
            };
        }

        let state = PipelineState {
            stdin,
            stdout: None,
            inside_pipeline: false,
            is_foreground,
            pipeline_pgid,
        };

        match self.command(last_cmd, state) {
            CommandRun::Spawned(Spawned { pid, pgid, .. }) => job_builder.build(pgid, pid),
            // When nothing was spawned the job is already completed and its pgid
            // is never waited on.
            CommandRun::Done(exit) => {
                job_builder.build_not_spawned(pipeline_pgid.unwrap_or(self.shell.pgid), exit)
            }
        }
    }

    fn wait_pipeline(&mut self, pipeline: &Pipeline) -> ExecStatus {
        let job = self.schedule_pipeline(pipeline, true);

        let exec_status = match Waiter::new(&self.shell).wait(job) {
            Ok(WaitResult::Completed(exit)) => exit.into(),
            Ok(WaitResult::Stopped { job, sig_stop }) => {
                self.bg_jobs.push(job);
                ExecStatus::Stopped(sig_stop)
            }
            Err(e) => {
                eprintln!("rush: wait: {e}");
                ExecStatus::FAILURE
            }
        };

        if pipeline.negated {
            exec_status.not()
        } else {
            exec_status
        }
    }

    fn or_list(&mut self, orl: &OrList) -> ExecStatus {
        let left_status = self.op_list(&orl.left);

        // Short-circuit if the lhs execution did not fail, or was interrupted
        if !left_status.failure() || left_status.interrupted() {
            return left_status;
        }

        self.op_list(&orl.right)
    }

    fn and_list(&mut self, andl: &AndList) -> ExecStatus {
        let left_status = self.op_list(&andl.left);

        // Short-circuit if the lhs execution failed (an interruption is a failure)
        if left_status.failure() {
            return left_status;
        }

        self.op_list(&andl.right)
    }

    fn op_list(&mut self, op: &OpList) -> ExecStatus {
        match op {
            OpList::AndList(andl) => self.and_list(andl),
            OpList::OrList(orl) => self.or_list(orl),
            OpList::Pipeline(pipeline) => self.wait_pipeline(pipeline),
        }
    }

    fn sequential_list(&mut self, seq: &SequentialList) -> ExecStatus {
        if let Some(left) = &seq.left {
            self.list(left);
        }

        self.op_list(&seq.right)
    }

    fn list(&mut self, list: &List) -> ExecStatus {
        match list {
            List::SequentialList(s) => self.sequential_list(s),
            List::AsyncList(async_list) => {
                if let Some(left) = &async_list.left {
                    self.list(left);
                }

                self.async_list(&async_list.right)
            }
        }
    }

    /// Run every complete command, the status is the one of the last command.
    fn program(&mut self, program: &ThisProgram) -> ExecStatus {
        let mut status = ExecStatus::SUCCESS;

        for complete_command in &program.child.commands {
            status = self.list(complete_command);
        }

        status
    }

    fn line_has_continuation(&self) -> bool {
        let Some(line) = self.input_buffer.last() else {
            return false;
        };

        let tokenizer = UnbufferedTokenizer::new(line);

        let mut prev = TokenType::Eof;

        loop {
            let next = tokenizer
                .next_token()
                .map_or(TokenType::Eof, |token| token.t_type);

            match next {
                TokenType::Eof | TokenType::NewLine => break,
                _ => {
                    prev = next;
                }
            }
        }

        matches!(
            prev,
            TokenType::LineContinuation | TokenType::AndAnd | TokenType::OrOr | TokenType::Pipe
        )
    }

    fn read_stdin(&mut self) -> io::Result<bool> {
        let mut line = String::with_capacity(512);

        match io::stdin().read_line(&mut line)? {
            0 => Ok(false), // EOF
            _ => {
                self.input_buffer.push(line);
                Ok(true)
            }
        }
    }

    /// Run the lines read from the input.
    fn execute(&mut self) -> ExecStatus {
        let input = join_continuations(&self.input_buffer);

        self.execute_lines(&input)
    }

    /// Run a program given as a string, like `eval` does.
    fn execute_str(&mut self, program: &str) -> ExecStatus {
        let lines: Vec<String> = program.split_inclusive('\n').map(String::from).collect();
        let input = join_continuations(&lines);

        self.execute_lines(&input)
    }

    fn execute_lines(&mut self, input: &[String]) -> ExecStatus {
        let tokenizer = BufTokenizer::from_buffer(input);

        if tokenizer.next_is_eof() {
            return ExecStatus::SUCCESS;
        }

        // The whole input must be consumed, otherwise there is some garbage the
        // grammar could not match.
        let program = SyntaxTree::program(&tokenizer).filter(|_| tokenizer.next_is_eof());

        let Some(program) = program else {
            eprintln!("rush: syntax error");
            return ExecStatus::SYNTAX_ERROR;
        };

        eprintln!("=== SYNTAX TREE ===");
        eprintln!("{program:#?}");

        eprintln!("=== COMMAND BEGIN ===");

        self.program(&program)
    }

    fn update(&mut self) -> Result<TerminalState, io::Error> {
        if !self.read_stdin()? {
            return Ok(TerminalState {
                terminate_session: true,
                ..Default::default()
            });
        }

        if self.line_has_continuation() {
            return Ok(TerminalState {
                needs_more: true,
                ..Default::default()
            });
        }

        // An interactive shell reaps before printing the prompt.
        if !self.shell.is_interactive {
            self.reap_bg_jobs();
        }

        let status = self.execute();

        self.input_buffer.clear();

        Ok(TerminalState {
            exit_code: status.code(),
            ..Default::default()
        })
    }

    pub fn run(&mut self) {
        let mut state = TerminalState::default();

        while !state.terminate_session {
            if self.shell.is_interactive {
                self.render_prompt(&state).unwrap();
            }

            state = self.update().unwrap();
        }
    }

    fn render_prompt(&mut self, state: &TerminalState) -> io::Result<()> {
        if state.exit_code != 0 {
            print!("{RED}");
        }

        if state.needs_more {
            print!("> ");
        } else {
            self.reap_bg_jobs();

            print!("$ ");
        }

        print!("{RESET}");

        io::stdout().flush()
    }

    /// Update the state of the background jobs and drop the completed ones.
    fn reap_bg_jobs(&mut self) {
        for job in &mut self.bg_jobs {
            if let Err(e) = Waiter::update_status(job) {
                eprintln!("rush: wait: {e}");
            }

            if let Some(exit) = job.completed_status()
                && self.shell.is_interactive
            {
                eprintln!("{}: Completed {exit:?}", job.pgid.as_raw());
            }
        }

        self.bg_jobs.retain(|job| !job.completed());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_status_code() {
        assert_eq!(ExecStatus::SUCCESS.code(), 0);
        assert_eq!(ExecStatus::Exited(42).code(), 42);
        assert_eq!(
            ExecStatus::Signaled(Signal::SIGINT).code(),
            128 + libc::SIGINT
        );
        assert_eq!(
            ExecStatus::Stopped(Signal::SIGTSTP).code(),
            128 + libc::SIGTSTP
        );
        assert_eq!(
            ExecStatus::from(JobExit::Signaled(Signal::SIGKILL)),
            ExecStatus::Signaled(Signal::SIGKILL)
        );
    }

    #[test]
    fn exec_status_failure_and_not() {
        let statuses = [
            (ExecStatus::SUCCESS, false),
            (ExecStatus::Exited(2), true),
            (ExecStatus::Signaled(Signal::SIGTERM), true),
            (ExecStatus::Stopped(Signal::SIGTSTP), true),
        ];

        for (status, failure) in statuses {
            assert_eq!(status.failure(), failure, "{status:?}");
            assert_eq!(status.not().failure(), !failure, "{status:?}");
        }

        let interrupted = ExecStatus::Signaled(Signal::SIGINT);
        assert!(interrupted.interrupted());
        assert_eq!(interrupted.not(), interrupted);
    }

    #[test]
    fn spawn_failure_status() {
        let not_found = io::Error::from(io::ErrorKind::NotFound);
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);

        assert_eq!(spawn_failure("missing", &not_found), JobExit::Exited(127));
        assert_eq!(spawn_failure("./missing", &not_found), JobExit::Exited(127));
        assert_eq!(
            spawn_failure("./not-executable", &denied),
            JobExit::Exited(126)
        );
    }
}
