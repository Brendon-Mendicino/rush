use std::collections::HashMap;

use crate::nix::{Pid, Signal};

/// How a job terminated: the status of its last process (the job master).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobExit {
    Exited(i32),
    Signaled(Signal),
}

/// State of a single process inside a [`Job`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcStatus {
    Running,
    Stopped(Signal),
    Exited(i32),
    Signaled(Signal),
}

impl ProcStatus {
    pub fn is_completed(self) -> bool {
        matches!(self, ProcStatus::Exited(_) | ProcStatus::Signaled(_))
    }
}

/// The member of a [`Job`] whose status is the status of the whole job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobMaster {
    Process(Pid),
    /// The last command could not be spawned; this is its status.
    NotSpawned(JobExit),
}

#[derive(Debug)]
pub struct JobBuilder {
    pids: Vec<Pid>,
}

/**
 * This struct represent an instance of a job, composed of many processes.
 *
 * For more info on implementing a job controls system take a look at:
 * https://www.gnu.org/software/libc/manual/html_node/Implementing-a-Shell.html
 *
 * The processes are only known by pid: they are spawned programs
 * (`process::Command`) or forked shells (subshells, async lists), and both are
 * reaped with `waitpid` only.
 */
#[derive(Debug)]
pub struct Job {
    pub pgid: Pid,
    pub job_master: JobMaster,
    pub job_status: HashMap<Pid, ProcStatus>,
    /// Terminal modes of the job, saved when it gets stopped and consumed when it is
    /// put back in the foreground.
    pub tmodes: Option<libc::termios>,
}

/// Where a job stands: done, or still waiting on one of its processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobProgress {
    /// Every process completed; this is the job's status.
    Completed(JobExit),
    /// A process that has not completed yet (running or stopped).
    Pending(Pid),
}

impl JobExit {
    fn from_proc_status(status: ProcStatus) -> Option<Self> {
        match status {
            ProcStatus::Exited(exit) => Some(Self::Exited(exit)),
            ProcStatus::Signaled(sig) => Some(Self::Signaled(sig)),
            _ => None,
        }
    }
}

impl JobBuilder {
    pub fn new() -> Self {
        Self { pids: Vec::new() }
    }

    pub fn add_job(&mut self, pid: Pid) {
        self.pids.push(pid);
    }

    pub fn build(mut self, pgid: Pid, job_master: Pid) -> Job {
        self.add_job(job_master);
        self.finish(pgid, JobMaster::Process(job_master))
    }

    /// Build a job whose last command could not be spawned. When nothing was
    /// spawned at all the job is already completed.
    pub fn build_not_spawned(self, pgid: Pid, exit: JobExit) -> Job {
        self.finish(pgid, JobMaster::NotSpawned(exit))
    }

    fn finish(self, pgid: Pid, job_master: JobMaster) -> Job {
        let job_status = self
            .pids
            .into_iter()
            .map(|pid| (pid, ProcStatus::Running))
            .collect();

        Job {
            pgid,
            job_master,
            job_status,
            tmodes: None,
        }
    }
}

impl Job {
    /// Processes that have not completed yet (running or stopped).
    fn pending_pids(&self) -> impl Iterator<Item = Pid> + '_ {
        self.job_status
            .iter()
            .filter(|(_, status)| !status.is_completed())
            .map(|(&pid, _)| pid)
    }

    /// The job's status, once every process completed.
    fn master_exit(&self) -> JobExit {
        match self.job_master {
            JobMaster::NotSpawned(exit) => exit,
            JobMaster::Process(pid) => JobExit::from_proc_status(self.job_status[&pid])
                .expect("the job master completed with the rest of the job"),
        }
    }

    pub fn completed_or_pending(&self) -> JobProgress {
        match self.pending_pids().next() {
            Some(pid) => JobProgress::Pending(pid),
            None => JobProgress::Completed(self.master_exit()),
        }
    }

    pub fn completed(&self) -> bool {
        self.pending_pids().next().is_none()
    }

    pub fn completed_status(&self) -> Option<JobExit> {
        match self.completed_or_pending() {
            JobProgress::Completed(exit) => Some(exit),
            JobProgress::Pending(_) => None,
        }
    }

    /// A job is stopped when every process is either completed or stopped.
    pub fn stopped(&self) -> bool {
        self.job_status
            .values()
            .all(|s| s.is_completed() || matches!(s, ProcStatus::Stopped(_)))
    }

    /// Signal that stopped the job, taken from the first stopped process.
    pub fn stop_signal(&self) -> Option<Signal> {
        self.job_status.values().find_map(|status| match status {
            ProcStatus::Stopped(sig) => Some(*sig),
            _ => None,
        })
    }

    pub fn mark_running(&mut self) {
        for s in self.job_status.values_mut() {
            if matches!(s, ProcStatus::Stopped(_)) {
                *s = ProcStatus::Running;
            }
        }
    }

    pub fn pending(&self) -> Vec<Pid> {
        // Collected: `update_status` changes the job while going through them.
        self.pending_pids().collect()
    }
}
