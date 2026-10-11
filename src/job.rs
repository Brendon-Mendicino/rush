use std::{collections::HashMap, process};

use crate::nix::Pid;

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
#[derive(Debug, Clone, Default)]
pub struct Job {
    pub pgid: Pid,
    pub jobs: HashMap<Pid, process::Child>,
    pub job_master_pid: Pid,
    job_status: HashMap<Pid, process::ExitStatus>,
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

    pub fn build(self, pgid: Pid, job_master: process::Child) -> Job {
        let JobBuilder { mut jobs } = self;

        let job_master_pid = Pid::from_raw(job_master.id() as i32);

        jobs.insert(job_master_pid, job_master);

        Job {
            pgid,
            jobs,
            job_master_pid,
            ..Self::default()
        }
    }
}

impl Job {}
