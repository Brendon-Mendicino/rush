use std::io;

use crate::{
    job::{Job, JobExit, JobProgress, ProcStatus},
    nix::{self, Pid, Signal, WaitPid, WaitRes, waitpid},
    shell::Shell,
};

#[derive(Debug)]
pub enum WaitResult {
    /// Every process finished; the job no longer exists
    Completed(JobExit),
    /// The job was stopped (e.g. Ctrl-Z); hand it back so it can go in the job table.
    Stopped { job: Job, sig_stop: Signal },
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

    /// Wait with job control: every job has its own process group, and a stopped
    /// job is handed back.
    fn wait_job(mut job: Job) -> io::Result<WaitResult> {
        debug_assert!(
            job.pgid.as_raw() != 0 || job.completed(),
            "A job with a pgid unitialized must be completed."
        );

        loop {
            if let Some(job_exit) = job.completed_status() {
                return Ok(WaitResult::Completed(job_exit));
            }
            if job.stopped() {
                let sig_stop = job.stop_signal().unwrap_or(Signal::SIGSTOP);
                return Ok(WaitResult::Stopped { job, sig_stop });
            }

            let res = waitpid(WaitPid::Group(job.pgid), libc::WUNTRACED)?;
            Self::process_status(&mut job, res)?;
        }
    }

    /// Wait without job control until every process completed. The jobs share
    /// the shell's process group, so each process is waited by pid: a wait on the
    /// group could reap a background job. Stops are not reported (no
    /// `WUNTRACED`), a stopped process is waited until it is continued and ends.
    fn wait_completed(mut job: Job) -> io::Result<JobExit> {
        loop {
            let pending = match job.completed_or_pending() {
                JobProgress::Completed(job_exit) => return Ok(job_exit),
                JobProgress::Pending(pending) => pending,
            };

            let res = waitpid(WaitPid::Single(pending), 0)?;
            Self::process_status(&mut job, res)?;
        }
    }

    /// Collect, without blocking, the state changes of a background job.
    pub fn update_status(job: &mut Job) -> io::Result<()> {
        // By pid: without job control the background jobs share a process group.
        let pending = job.pending();

        for pid in pending {
            loop {
                match waitpid(WaitPid::Single(pid), libc::WUNTRACED | libc::WNOHANG) {
                    // Nothing to report for this process.
                    Ok(WaitRes::StillAlive) => break,
                    Err(e) if e.raw_os_error() == Some(libc::ECHILD) => break,
                    Err(e) => return Err(e),
                    Ok(res) => {
                        let completed = matches!(res, WaitRes::Exited(..) | WaitRes::Signaled(..));
                        Self::process_status(job, res)?;
                        if completed {
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Wait for a foreground job until it either completes or, with job control,
    /// gets stopped.
    pub fn wait(&self, job: Job) -> io::Result<WaitResult> {
        if !self.shell.is_interactive {
            return Self::wait_completed(job).map(WaitResult::Completed);
        }

        let mut res = Self::wait_job(job);

        // The terminal must go back to the shell on every path, even when
        // waiting failed, otherwise the shell is left without a usable tty.
        let stopped = match &mut res {
            Ok(WaitResult::Stopped { job, .. }) => Some(job),
            _ => None,
        };
        self.reclaim_terminal(stopped);

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
