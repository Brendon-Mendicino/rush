use crate::{
    job::{Job, JobBuilder, JobExit},
    nix::{Pid, Signal, close, dup2_raw},
    shell::Shell,
    syntax::*,
    tokenizer::{BufTokenizer, TokenType, Tokenizer, UnbufferedTokenizer},
    waiter::{WaitResult, Waiter},
};
use std::{
    fs::File,
    io::{self, Write},
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
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

#[derive(Debug)]
pub struct Executor {
    input_buffer: Vec<String>,
    shell: Shell,
    /// Jobs that got stopped or that are running in background.
    bg_jobs: Vec<Job>,
}

struct PipelineState<'child> {
    prev_child: Option<&'child mut Child>,
    inside_pipeline: bool,
    pipeline_pgid: Option<Pid>,
}

#[derive(Debug, Default)]
struct RedirectController {
    file_redirects: Vec<(File, i32)>,
    duplications: Vec<(i32, i32)>,
    fd_to_close: Vec<i32>,
}

#[derive(Debug)]
struct RedirectionApplyer {
    file_redirects: Vec<(i32, i32)>,
    duplications: Vec<(i32, i32)>,
    fd_to_close: Vec<i32>,
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
    fn add_redirect(&mut self, redirect: &Redirect) {
        match redirect {
            Redirect::FileRedirect(file_redirect) => {
                let mut options = File::options();

                match file_redirect.file_kind {
                    OpenKind::Read => options.read(true),
                    OpenKind::Replace => options.create(true).write(true).truncate(true),
                    OpenKind::Append => options.create(true).append(true),
                    OpenKind::Rw => options.create(true).read(true).write(true),
                };

                let filename = file_redirect.filename.text();
                let file = options.open(&filename).unwrap();

                self.file_redirects.push((file, file_redirect.redirect_fd));
            }
            Redirect::FdRedirect(dup_fd) => {
                self.duplications
                    .push((dup_fd.fd_to_replace, dup_fd.fd_replacer));
            }
            Redirect::CloseFd(close_fd) => {
                self.fd_to_close.push(close_fd.fd);
            }
        }
    }

    fn get_applyer(&self) -> RedirectionApplyer {
        let file_redirects = self
            .file_redirects
            .iter()
            // (to_replace, replacer): the redirected fd gets replaced by the file
            .map(|(f, fd)| (*fd, f.as_raw_fd()))
            .collect::<Vec<_>>();

        RedirectionApplyer {
            file_redirects,
            duplications: self.duplications.clone(),
            fd_to_close: self.fd_to_close.clone(),
        }
    }

    fn add_redirects(&mut self, redirections: &[Redirect]) {
        for redirect in redirections {
            self.add_redirect(redirect);
        }
    }
}

impl RedirectionApplyer {
    fn apply(&self) -> io::Result<()> {
        // TODO: we might want to add a communication pipe with the parent to
        // get back some more info about the failure

        // close fds: leftover from the pipe() calls
        for to_close in &self.fd_to_close {
            close(*to_close)?;
        }

        // Duplicate fds from files
        for (to_replace, replacer) in &self.file_redirects {
            dup2_raw(*replacer, *to_replace)?;
        }

        // Duplicate fds from duplication syntax
        for (to_replace, replacer) in &self.duplications {
            dup2_raw(*replacer, *to_replace)?;
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

    fn simple_command(&self, cmd: &SimpleCommand, state: PipelineState) -> Spawned {
        let mut redirect_controller = RedirectController::default();

        redirect_controller.add_redirects(&cmd.redirections);

        let mut command = process::Command::new(&cmd.program);
        command.args(&cmd.arguments);

        let applyer = Box::new(redirect_controller.get_applyer());

        let applyer_ptr = ApplyerPtr(Box::into_raw(applyer));
        let ptr_clone = applyer_ptr.clone();

        unsafe {
            command.pre_exec(move || {
                ptr_clone.apply()?;
                Ok(())
            });
        }

        // An interactive shell puts every job in its own process group, the first
        // command of the pipeline becomes the leader (0 = use its own pid).
        // `spawn()` returns only after the child exec'd, so the group is already
        // set when the parent continues: no setpgid race with the child.
        if self.shell.is_interactive {
            command.process_group(state.pipeline_pgid.map_or(0, Pid::as_raw));
        }

        if let Some(prev_child) = state.prev_child {
            let stdout = prev_child.stdout.take().unwrap();
            command.stdin(Stdio::from(stdout));
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

        let child = spawn_result.unwrap();

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

        Spawned { child, pgid }
    }

    fn unsub_command(&self, cmd: &UnsubCommand, state: PipelineState) -> Spawned {
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

    fn command(&self, command: &Command, state: PipelineState) -> Spawned {
        match command {
            Command::SimpleAssignment(assignment) => todo!(),
            Command::UnsubCommand(command) => self.unsub_command(command, state),
            Command::Subshell(subshell) => todo!(),
        }
    }

    fn pipeline(&self, pipeline: &Pipeline) -> Job {
        assert!(
            !pipeline.cmds.is_empty(),
            "A pipeline must always contain something"
        );

        // Decided by the first spawned command, the others join its group.
        let mut pipeline_pgid = None;
        let mut last_child = None;
        let mut job_builder = JobBuilder::new();

        let (last_cmd, inner_cmds) = pipeline.cmds.split_last().unwrap();

        for cmd in inner_cmds {
            let Spawned { child, pgid } = self.command(
                cmd,
                PipelineState {
                    prev_child: last_child.as_mut(),
                    inside_pipeline: true,
                    pipeline_pgid,
                },
            );
            pipeline_pgid = Some(pgid);

            match last_child {
                None => {
                    last_child = Some(child);
                }
                Some(to_add) => {
                    last_child = Some(child);
                    job_builder.add_job(to_add);
                }
            }
        }

        let Spawned { child, pgid } = self.command(
            last_cmd,
            PipelineState {
                prev_child: last_child.as_mut(),
                inside_pipeline: false,
                pipeline_pgid,
            },
        );

        if let Some(last_child) = last_child {
            job_builder.add_job(last_child);
        }

        job_builder.build(pgid, child)
    }

    fn wait_pipeline(&mut self, pipeline: &Pipeline) -> ExecStatus {
        let job = self.pipeline(pipeline);

        match Waiter::new(&self.shell).wait(job) {
            Ok(WaitResult::Completed(exit)) => exit.into(),
            Ok(WaitResult::Stopped(job)) => {
                // If a job was stopped while waiting for it, put it in the
                // background jobs.
                let sig = job.stop_signal().unwrap_or(Signal::SIGTSTP);
                self.bg_jobs.push(job);
                ExecStatus::Stopped(sig)
            }
            Err(e) => {
                eprintln!("rush: wait: {e}");
                ExecStatus::FAILURE
            }
        }
    }

    fn op_list(&mut self, op: &OpList) -> ExecStatus {
        match op {
            OpList::AndList(_) => not_implemented("`&&`"),
            OpList::OrList(_) => not_implemented("`||`"),
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
                eprintln!("{}: Completed {exit:?}", job.job_master_pid.as_raw());
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
}
