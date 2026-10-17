use std::io;

use crate::{
    job::{Job, JobExit, ProcStatus},
    nix::{self, Pid, Signal, WaitPid, WaitRes, waitpid},
    shell::Shell,
};

#[derive(Debug)]
pub enum WaitResult {
    /// Everty process finished; the job no longer exists
    Completed(JobExit),
    /// The job was stopped (e.g. Ctrl-Z); hand it back so it can go in the job table.
    Stopped(Job),
}

pub struct Waiter<'s> {
    shell: &'s Shell,
}

impl<'s> Waiter<'s> {
    pub fn new(shell: &'s Shell) -> Self {
        Self { shell }
    }

    fn process_status(job: &mut Job, res: WaitRes) -> io::Result<()> {
        let (pid, status) = match res {
            WaitRes::Exited(pid, code) => (pid, ProcStatus::Exited(code)),
            WaitRes::Signaled(pid, sig, _core_dumped) => {
                eprintln!("{}: Terminated by signal {sig:?}", pid.as_raw());
                (pid, ProcStatus::Signaled(sig))
            }
            WaitRes::Stopped(pid, sig) => {
                eprintln!("{}: stopped by {sig:?}", pid.as_raw());
                (pid, ProcStatus::Stopped(sig))
            }
            WaitRes::Continued(pid) => (pid, ProcStatus::Running),
            WaitRes::StillAlive => {
                unreachable!("StillAlive must be handled by the WNOHANG caller")
            }
        };

        let slot = job.job_status.get_mut(&pid).ok_or_else(|| {
            io::Error::other(format!(
                "pid={} is not part of pgid={}",
                pid.as_raw(),
                job.pgid.as_raw()
            ))
        })?;
        *slot = status;

        Ok(())
    }

    fn wait_job(mut job: Job) -> io::Result<WaitResult> {
        // TODO: this pgid, when executing in non-interctive shell, should be
        // equal for all the jobs
        debug_assert!(
            job.pgid.as_raw() != 0 || job.completed(),
            "A job with a pgid unitialized must be completed."
        );

        loop {
            if let Some(job_exit) = job.completed_status() {
                return Ok(WaitResult::Completed(job_exit));
            }
            if job.stopped() {
                return Ok(WaitResult::Stopped(job));
            }

            let res = waitpid(WaitPid::Group(job.pgid), libc::WUNTRACED)?;
            Self::process_status(&mut job, res)?;
        }
    }

    /// Wait for a foreground job until it either completes or gets stopped.
    pub fn wait(&self, job: Job) -> io::Result<WaitResult> {
        let mut res = Self::wait_job(job);

        if self.shell.is_interactive {
            // The terminal must go back to the shell on every path, even when
            // waiting failed, otherwise the shell is left without a usable tty.
            let stopped = match &mut res {
                Ok(WaitResult::Stopped(job)) => Some(job),
                _ => None,
            };
            self.reclaim_terminal(stopped);
        }

        res
    }

    /// Put the shell back in the foreground and restore its terminal modes.
    ///
    /// When the job got stopped its current modes are saved, so that [`Waiter::fg`]
    /// can restore them. A completed job is dropped, there is nothing to save.
    fn reclaim_terminal(&self, stopped: Option<&mut Job>) {
        if let Err(e) = nix::tcsetpgrp(self.shell.terminal, self.shell.pgid) {
            eprintln!(
                "tcsetpgrp({}, {}): {e}",
                self.shell.terminal,
                self.shell.pgid.as_raw()
            );
        }

        if let Some(job) = stopped {
            job.tmodes = match nix::tcgetattr(self.shell.terminal) {
                Ok(tmodes) => Some(tmodes),
                Err(e) => {
                    eprintln!("tcgetattr({}): {e}", self.shell.terminal);
                    None
                }
            };
        }

        // TODO: this is not fine when calling `stty`. Decide how do handle this
        if let Some(tmodes) = &self.shell.tmodes {
            if let Err(e) = nix::tcsetattr(self.shell.terminal, libc::TCSADRAIN, tmodes) {
                eprintln!("tcsetattr({}): {e}", self.shell.terminal);
            }
        }
    }

    pub fn wait_inside_async(mut job: Job) -> io::Result<JobExit> {
        // Don't check for stopped jobs. An async list will terminate only when all
        // the children are completed. Some of them might get stopped by the tty. We
        // don't want to lose them before exiting the async list.
        loop {
            if let Some(job_exit) = job.completed_status() {
                return Ok(job_exit);
            }

            let res = waitpid(WaitPid::Group(job.pgid), libc::WUNTRACED)?;
            Self::process_status(&mut job, res)?;
        }
    }

    pub fn bg(&self, job: &mut Job) {
        let pgrp = Pid::from_raw(-job.pgid.as_raw());
        if let Err(e) = nix::kill(pgrp, Signal::SIGCONT) {
            eprintln!("kill({}, SIGCONT): {e}", pgrp.as_raw());
        }

        // TODO: error handling?
        job.mark_running();
    }

    pub fn fg(&self, mut job: Job) -> io::Result<WaitResult> {
        // Put the job in foreground
        if let Err(e) = nix::tcsetpgrp(self.shell.terminal, job.pgid) {
            eprintln!(
                "tcsetpgrp({}, {}): {e}",
                self.shell.terminal,
                job.pgid.as_raw()
            );
        }

        // Restore the tmodes on the terminal before sending SIGCONT. They are taken
        // out of the job: once it runs again they are stale, and a fresh copy is
        // saved if it gets stopped again.
        if let Some(tmodes) = job.tmodes.take() {
            if let Err(e) = nix::tcsetattr(self.shell.terminal, libc::TCSADRAIN, &tmodes) {
                eprintln!("tcsetattr({}): {e}", self.shell.terminal);
            }
        }

        self.bg(&mut job);
        self.wait(job)
    }
}
