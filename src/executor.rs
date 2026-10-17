use crate::{
    job::{Job, JobBuilder},
    nix::{Pid, close, dup2_raw},
    shell::Shell,
    syntax::*,
    tokenizer::{BufTokenizer, TokenType, Tokenizer, UnbufferedTokenizer},
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

#[derive(Debug)]
pub struct Executor {
    input_buffer: Vec<String>,
    shell: Shell,
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
            .map(|(f, fd)| (f.as_raw_fd(), *fd))
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
        }
    }

    fn simple_command(&self, cmd: &SimpleCommand, state: PipelineState) -> Child {
        let mut redirect_controller = RedirectController::default();

        redirect_controller.add_redirects(&cmd.redirections);

        let mut command = process::Command::new(&cmd.program);
        let applyer = Box::new(redirect_controller.get_applyer());

        let applyer_ptr = ApplyerPtr(Box::into_raw(applyer));
        let ptr_clone = applyer_ptr.clone();

        unsafe {
            command.pre_exec(move || {
                ptr_clone.apply()?;
                Ok(())
            });
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

        spawn_result.unwrap()
    }

    fn unsub_command(&self, cmd: &UnsubCommand, state: PipelineState) -> Child {
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

    fn command(&self, command: &Command, state: PipelineState) -> Child {
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

        let mut pipeline_pgid = None;
        let mut last_child = None;
        let mut job = JobBuilder::new();

        let (last_cmd, inner_cmds) = pipeline.cmds.split_last().unwrap();

        for cmd in inner_cmds {
            let child = self.command(
                cmd,
                PipelineState {
                    prev_child: last_child.as_mut(),
                    inside_pipeline: true,
                    pipeline_pgid,
                },
            );

            if pipeline_pgid.is_none() {
                pipeline_pgid = Some(Pid::from_raw(child.id() as i32));
            }

            match last_child {
                None => {
                    last_child = Some(child);
                }
                Some(to_add) => {
                    last_child = Some(child);
                    job.add_job(to_add);
                }
            }
        }

        let child = self.command(
            last_cmd,
            PipelineState {
                prev_child: last_child.as_mut(),
                inside_pipeline: false,
                pipeline_pgid,
            },
        );

        if let Some(last_child) = last_child {
            job.add_job(last_child);
        }

        let pgid = match pipeline_pgid {
            Some(pgid) => pgid,
            None => Pid::from_raw(child.id() as i32),
        };

        job.build(pgid, child)
    }

    fn wait_pipeline(&self, pipeline: &Pipeline) {
        let job = self.pipeline(pipeline);
    }

    fn op_list(&self, op: &OpList) {
        match op {
            OpList::AndList(and) => {}
            OpList::OrList(or) => {}
            OpList::Pipeline(pipeline) => self.wait_pipeline(pipeline),
        }
    }

    fn sequential_list(&self, seq: &SequentialList) {
        /*
                ListStats stats{};

                if (sequential_list.left.has_value()) {
                    stats = this->list(**sequential_list.left, state);
                }

                stats.last_stats = this->op_list(*sequential_list.right, state);

                return stats;
        */
        if let Some(left) = &seq.left {
            self.list(left);
        }

        self.op_list(&seq.right);
    }

    fn list(&self, list: &List) {
        match list {
            List::SequentialList(s) => self.sequential_list(s),
            List::AsyncList(a) => {}
        }
    }

    fn program(&self, program: &ThisProgram) {
        if program.child.commands.is_empty() {
            return;
        }

        // TODO: riveder
        for complete_command in &program.child.commands {
            self.list(complete_command);
        }
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

        for line in &self.input_buffer {
            match lines.last_mut() {
                Some(previous) if previous.ends_with('\\') => {
                    previous.pop(); // remove trailing '\'
                    previous.push_str(line);
                }
                _ => lines.push(line.clone()),
            }
        }

        lines
    }

    fn execute(&mut self) {
        let input = self.process_input();

        let mut tokenizer = BufTokenizer::from_buffer(&input);

        // if tokenizer.next_is_eof() {
        //     return Ok(ExecStats::default());
        // }

        let program = SyntaxTree::program(&mut tokenizer);

        eprintln!("=== SYNTAX TREE ===");
        eprintln!("{program:#?}");

        eprintln!("=== COMMAND BEGIN ===");

        self.program(todo!());
        //Ok(self.program(program)?)
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

        let exec_stats = self.execute();

        self.input_buffer.clear();

        // Ok(TerminalState {
        //     exit_code: exec_stats.exit_code,
        //     ..Default::default()
        // })
        Ok(TerminalState::default())
    }

    pub fn run(&mut self) {
        let mut state = TerminalState::default();

        while !state.terminate_session {
            // if self.shell.is_interactive {
            //     self.render_prompt(&state)?;
            // }
            self.render_prompt(&state).unwrap();

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
            // self.reap_completed_jobs();

            print!("$ ");
        }

        print!("{RESET}");

        io::stdout().flush()
    }

    // fn reap_completed_jobs(&mut self) {
    //     for job in &mut self.bg_jobs {
    //         Waiter::update_status(job);
    //
    //         if job.completed() {
    //             eprintln!("{}: Completed stats={:?}", job.job_master, job.exec_stats());
    //         }
    //     }
    //
    //     self.bg_jobs.retain(|job| !job.completed());
    // }
}
