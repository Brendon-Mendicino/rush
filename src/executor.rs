use crate::{
    job::{Job, JobBuilder, JobExit},
    nix::{self, Pid, SigHandler, Signal, close, dup2_raw},
    shell::{JOB_CONTROL_SIGNALS, Shell},
    syntax::*,
    tokenizer::{BufTokenizer, TokenType, Tokenizer, UnbufferedTokenizer},
    waiter::{WaitResult, Waiter},
};
use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::{self, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::{fs::OpenOptionsExt, process::CommandExt},
    },
    process::{self, Child, Stdio},
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

fn not_implemented(what: &str) -> ExecStatus {
    eprintln!("rush: {what} is not implemented yet");
    ExecStatus::FAILURE
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

#[derive(Debug)]
pub struct Executor {
    input_buffer: Vec<String>,
    shell: Shell,
    /// Jobs that got stopped or that are running in background.
    bg_jobs: Vec<Job>,
}

struct PipelineState {
    /// Input of the command: the output of the previous command of the pipeline.
    stdin: Option<Stdio>,
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
    child: Child,
    pgid: Pid,
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

impl Executor {
    pub fn new() -> Self {
        Self {
            input_buffer: Vec::new(),
            shell: Shell::new(),
            bg_jobs: Vec::new(),
        }
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

        // The environment is the exported shell variables plus the command's
        // `VAR=x` prefixes. `env` keeps the last value of a key, so a prefix
        // overrides a shell variable and the last prefix wins. The program is
        // looked up in this `PATH`.
        command.env_clear();
        command.envs(self.shell.vars.exported());
        for assignment in &cmd.envs {
            command.env(assignment.key, assignment.value_text());
        }

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

        let applyer = Box::new(redirect_controller.get_applyer());

        let applyer_ptr = ApplyerPtr(Box::into_raw(applyer));
        let ptr_clone = applyer_ptr.clone();

        unsafe {
            command.pre_exec(move || {
                ptr_clone.apply()?;
                Ok(())
            });
        }

        if let Some(stdin) = state.stdin {
            command.stdin(stdin);
        }

        // We use this to chain the processes in a pipeline
        if state.inside_pipeline {
            command.stdout(Stdio::piped());
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

        let child = match spawn_result {
            Ok(child) => child,
            Err(e) => return CommandRun::Done(spawn_failure(&cmd.program, &e)),
        };

        // A non-interactive shell does no job control: the children stay in the
        // shell's process group. Otherwise the first child of the pipeline leads
        // the group with its pid.
        let pgid = if self.shell.is_interactive {
            state
                .pipeline_pgid
                .unwrap_or_else(|| Pid::from_raw(child.id() as i32))
        } else {
            self.shell.pgid
        };

        CommandRun::Spawned(Spawned { child, pgid })
    }

    fn unsub_command(&self, cmd: &UnsubCommand, state: PipelineState) -> CommandRun {
        let program = match &cmd.program {
            Word::Token(token) => token.text(),
            Word::Substitution(sub) => todo!(),
        };

        let arguments = cmd
            .arguments
            .iter()
            .map(|w| match w {
                Word::Token(token) => token.text(),
                Word::Substitution(sub) => todo!(),
            })
            .collect::<Vec<_>>();

        let expanded = SimpleCommand {
            program,
            arguments,
            redirections: cmd.redirections.clone(),
            envs: cmd.envs.clone(),
        };

        self.simple_command(&expanded, state)
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
            self.shell.vars.upsert(var.key, var.value_text(), None);
        }

        CommandRun::Done(JobExit::Exited(0))
    }

    fn command(&mut self, command: &Command, state: PipelineState) -> CommandRun {
        match command {
            Command::SimpleAssignment(assignment) => self.simple_assignment(assignment, state),
            Command::UnsubCommand(command) => self.unsub_command(command, state),
            Command::Subshell(subshell) => todo!(),
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
                inside_pipeline: true,
                is_foreground,
                pipeline_pgid,
            };

            stdin = match self.command(cmd, state) {
                CommandRun::Spawned(Spawned { mut child, pgid }) => {
                    pipeline_pgid = Some(pgid);
                    let stdout = child.stdout.take().map(Stdio::from);
                    job_builder.add_job(child);
                    stdout
                }
                // The next command reads from a command that wrote nothing. It must
                // not inherit the shell's stdin.
                CommandRun::Done(_) => Some(Stdio::null()),
            };
        }

        let state = PipelineState {
            stdin,
            inside_pipeline: false,
            is_foreground,
            pipeline_pgid,
        };

        match self.command(last_cmd, state) {
            CommandRun::Spawned(Spawned { child, pgid }) => job_builder.build(pgid, child),
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
            List::AsyncList(_) => not_implemented("`&`"),
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

        let mut tokenizer = UnbufferedTokenizer::new(line);

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

    fn process_input(&self) -> Vec<String> {
        let mut lines = Vec::<String>::new();

        // Lines keep their newline: a continuation line ends with "\\\n" (or "\\\r\n").
        // Cut it and paste the next line on it.
        for line in &self.input_buffer {
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

    fn execute(&mut self) -> ExecStatus {
        let input = self.process_input();

        let tokenizer = BufTokenizer::from_buffer(&input);

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

            if let Some(exit) = job.completed_status() {
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
