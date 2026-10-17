/*
void Waiter::process_wstatus(Job &job, pid_t pid, int wstatus) {
    assertm(pid != 0,
            "A pid=0 likely means a return from waitpid(...,WNOHANG) no "
            "signal was received from any child");

    pid_t pgid = job.pgid;

    if (pid == -1) {
        throw std::runtime_error(std::format("wait_job: waitpid({}): {}", -pgid,
                                             std::strerror(errno)));
    }

    if (!job.jobs.contains(pid)) {
        throw std::runtime_error(
            std::format("pid={} is not part of pgid={}", pid, pgid));
    }

    auto &stats = job.jobs.at(pid);

    if (WIFSTOPPED(wstatus)) {
        stats.stopped = true;
        std::println(stderr, "{}: stopped by {}({})", pid,
                     strsignal(WSTOPSIG(wstatus)), WSTOPSIG(wstatus));
        return;
    }

    stats.completed = true;

    if (WIFEXITED(wstatus)) {
        stats.exit_code = WEXITSTATUS(wstatus);
    } else if (WIFSIGNALED(wstatus)) {
        stats.exit_code = 1;
        stats.signaled = WTERMSIG(wstatus);

        std::println(stderr, "{}: Terminated by signal {}({})", pid,
                     strsignal(WTERMSIG(wstatus)), WTERMSIG(wstatus));
    }
}

Job Waiter::wait_job(Job &&job) {
    // TODO: this pgid, when executing in non-interctive shell, should be
    // equal for all the jobs
    pid_t pgid = job.pgid;

    assertm((pgid != 0) || (pgid == 0 && job.completed()),
            "A job with a pgid unitialized must be completed.");

    while (!job.completed() && !job.stopped()) {
        int wstatus;
        const pid_t pid = waitpid(-pgid, &wstatus, WUNTRACED);

        process_wstatus(job, pid, wstatus);
    }

    return job;
}

void Waiter::update_status(Job &job) {
    // TODO: this pgid, when executing in non-interctive shell, should
    // be equal for all the jobs
    pid_t pgid = job.pgid;

    assertm((pgid != 0) || (pgid == 0 && job.completed()),
            "A job with a pgid unitialized must be completed.");

    for (;;) {
        int wstatus;
        const pid_t pid = waitpid(-pgid, &wstatus, WUNTRACED | WNOHANG);

        if (pid == 0) {
            /* No processes ready to report.  */
            break;
        }

        if (pid == -1 && errno == ECHILD) {
            /* No processes ready to report.  */
            break;
        }

        process_wstatus(job, pid, wstatus);
    }
}

void Waiter::wait(Job &job) const {
    Job retval;

    if (shell.is_interactive) {
        /* Wait for it to report.  */
        retval = Waiter::wait_job(std::move(job));

        /* Put the shell back in the foreground.  */
        if (tcsetpgrp(shell.terminal, shell.pgid) == -1) {
            std::println(stderr, "tcsetpgrp({}, {}): {}", shell.terminal,
                         shell.pgid, std::strerror(errno));
        }

        /* Restore the shell’s terminal modes.  */
        job.set_modes(shell);
        // TODO: this is not fine when calling `stty`. Decide how do handle this
        tcsetattr(shell.terminal, TCSADRAIN, &shell.tmodes);
    } else {
        retval = Waiter::wait_job(std::move(job));
    }

    job = retval;
}

void Waiter::wait_inside_async(Job &job) const {
    pid_t pgid = job.pgid;

    /* Don't check for stopped jobs. An asyn list will terminate only when all
     * the childred are completed. Some of them might get stopped by the tty. We
     * don't want to loose them before exiting the async list.
     */
    while (!job.completed()) {
        int wstatus;
        const pid_t pid = waitpid(-pgid, &wstatus, WUNTRACED);

        process_wstatus(job, pid, wstatus);
    }
}

void Waiter::bg(Job &job) const {
    if (kill(-job.pgid, SIGCONT) < 0) {
        std::println(stderr, "kill({}, SIGCONG): {}", -job.pgid,
                     std::strerror(errno));
    }

    // TODO: error handling?
    job.mark_running();
}

void Waiter::fg(Job &job) const {
    /* Put the job in foreground */
    tcsetpgrp(shell.terminal, job.pgid);

    /* Restore the tmodes on the terminal before sening SIGCONT */
    job.restore_modes(shell);

    if (kill(-job.pgid, SIGCONT) < 0) {
        std::println(stderr, "kill({}, SIGCONT): {}", -job.pgid,
                     std::strerror(errno));
    }

    // TODO: error handling?
    job.mark_running();

    this->wait(job);
}
*/

use std::{io, os::fd::RawFd};

use crate::{
    job::{Job, JobExit, ProcStatus},
    nix::{self, Pid, Signal, WaitPid, WaitRes, waitpid},
};

#[derive(Debug)]
pub enum WaitResult {
    /// Everty process finished; the job no longer exists
    Completed(JobExit),
    /// The job was stopped (e.g. Ctrl-Z); hand it back so it can go in the job table.
    Stopped(Job),
}

pub struct Waiter {
    is_interactive: bool,
    terminal: RawFd,
    pgid: Pid,
    tmodes: libc::termios,
}

impl Waiter {
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

        if self.is_interactive {
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
        if let Err(e) = nix::tcsetpgrp(self.terminal, self.pgid) {
            eprintln!("tcsetpgrp({}, {}): {e}", self.terminal, self.pgid.as_raw());
        }

        if let Some(job) = stopped {
            job.tmodes = match nix::tcgetattr(self.terminal) {
                Ok(tmodes) => Some(tmodes),
                Err(e) => {
                    eprintln!("tcgetattr({}): {e}", self.terminal);
                    None
                }
            };
        }

        // TODO: this is not fine when calling `stty`. Decide how do handle this
        if let Err(e) = nix::tcsetattr(self.terminal, libc::TCSADRAIN, &self.tmodes) {
            eprintln!("tcsetattr({}): {e}", self.terminal);
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
        if let Err(e) = nix::tcsetpgrp(self.terminal, job.pgid) {
            eprintln!("tcsetpgrp({}, {}): {e}", self.terminal, job.pgid.as_raw());
        }

        // Restore the tmodes on the terminal before sending SIGCONT. They are taken
        // out of the job: once it runs again they are stale, and a fresh copy is
        // saved if it gets stopped again.
        if let Some(tmodes) = job.tmodes.take() {
            if let Err(e) = nix::tcsetattr(self.terminal, libc::TCSADRAIN, &tmodes) {
                eprintln!("tcsetattr({}): {e}", self.terminal);
            }
        }

        self.bg(&mut job);
        self.wait(job)
    }
}
