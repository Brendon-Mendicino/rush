use std::{collections::HashMap, process};

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
    jobs: HashMap<Pid, process::Child>,
}

/**
 * This struct represent an instance of a job, composed of many processes.
 *
 * For more info on implementing a job controls system take a look at:
 * https://www.gnu.org/software/libc/manual/html_node/Implementing-a-Shell.html
 *
 */
#[derive(Debug)]
pub struct Job {
    pub pgid: Pid,
    pub jobs: HashMap<Pid, process::Child>,
    pub job_master: JobMaster,
    pub job_status: HashMap<Pid, ProcStatus>,
    /// Terminal modes of the job, saved when it gets stopped and consumed when it is
    /// put back in the foreground.
    pub tmodes: Option<libc::termios>,
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
        Self {
            jobs: HashMap::new(),
        }
    }

    pub fn add_job(&mut self, child: process::Child) {
        self.jobs.insert(Pid::from_raw(child.id() as i32), child);
    }

    pub fn build(mut self, pgid: Pid, job_master: process::Child) -> Job {
        let job_master_pid = Pid::from_raw(job_master.id() as i32);

        self.add_job(job_master);
        self.finish(pgid, JobMaster::Process(job_master_pid))
    }

    /// Build a job whose last command could not be spawned. When nothing was
    /// spawned at all the job is already completed.
    pub fn build_not_spawned(self, pgid: Pid, exit: JobExit) -> Job {
        self.finish(pgid, JobMaster::NotSpawned(exit))
    }

    fn finish(self, pgid: Pid, job_master: JobMaster) -> Job {
        let JobBuilder { jobs } = self;

        let job_status = jobs.keys().map(|&pid| (pid, ProcStatus::Running)).collect();

        Job {
            pgid,
            jobs,
            job_master,
            job_status,
            tmodes: None,
        }
    }
}

impl Job {
    pub fn completed(&self) -> bool {
        self.job_status.values().all(|s| s.is_completed())
    }

    pub fn completed_status(&self) -> Option<JobExit> {
        if !self.completed() {
            return None;
        }

        match self.job_master {
            JobMaster::Process(pid) => self
                .job_status
                .get(&pid)
                .and_then(|&status| JobExit::from_proc_status(status)),
            JobMaster::NotSpawned(exit) => Some(exit),
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
}
