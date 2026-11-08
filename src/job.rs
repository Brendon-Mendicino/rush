use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    ops::Deref,
};

use crate::{
    nix::{Pid, Signal},
    waiter::Waiter,
};

/// How a job terminated: the status of its last process (the job master).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobExit {
    Exited(i32),
    Signaled(Signal),
}

impl JobExit {
    /// Value for `$?`: `128 + signum` when a signal ended the job.
    pub fn code(self) -> i32 {
        match self {
            Self::Exited(code) => code,
            Self::Signaled(sig) => 128 + sig as i32,
        }
    }
}

/// What a job is doing, as `jobs` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Stopped(Signal),
    Done(JobExit),
}

/// The `<state>` field of `jobs` (`docs/utilities/jobs.html`). A job killed by a
/// signal has no fixed format, it only has to name the signal.
impl fmt::Display for JobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Running => write!(f, "Running"),
            Self::Stopped(sig) => write!(f, "Stopped ({sig:?})"),
            Self::Done(JobExit::Exited(0)) => write!(f, "Done"),
            Self::Done(JobExit::Exited(code)) => write!(f, "Done({code})"),
            Self::Done(JobExit::Signaled(sig)) => write!(f, "Terminated ({sig:?})"),
        }
    }
}

/// A job control job ID (XBD §3.204), already parsed: the built-ins turn the
/// `%` forms into this, the table only looks it up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobId {
    /// `%%`, `%+`
    Current,
    /// `%-`
    Previous,
    /// `%n`
    Number(usize),
    /// `%string`: the command begins with it.
    Prefix(String),
    /// `%?string`: the command contains it.
    Contains(String),
}

/// Back to the `%` form, for the error messages.
impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Current => write!(f, "%+"),
            Self::Previous => write!(f, "%-"),
            Self::Number(n) => write!(f, "%{n}"),
            Self::Prefix(s) => write!(f, "%{s}"),
            Self::Contains(s) => write!(f, "%?{s}"),
        }
    }
}

/// A job ID that doesn't name exactly one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobIdError {
    NoSuchJob,
    /// `%str` or `%?str` matches more than one job.
    Ambiguous,
}

impl fmt::Display for JobIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchJob => write!(f, "no such job"),
            Self::Ambiguous => write!(f, "ambiguous job spec"),
        }
    }
}

/// A job in the [`JobTable`]; its job number is the key it is stored under.
#[derive(Debug)]
pub struct JobEntry {
    /// The command that formed the job, as `jobs`, `fg` and `bg` print it.
    pub command: String,
    pub job: Job,
    /// Whether the last change of state was shown to the user, through a
    /// [`JobReport`].
    // TODO: check if i can remove this from here, having a bool does not seems like a good idea
    // for retaining the completed jobs that already reported, this could be made into an enum
    // somewere
    reported: bool,
    /// When the job was last started, stopped or resumed: the most recent one is
    /// the current job.
    last_used: u64,
}

/// The jobs of the shell: the background jobs and the stopped ones, by job
/// number.
#[derive(Debug, Default)]
pub struct JobTable {
    jobs: BTreeMap<usize, JobEntry>,
    /// Source of [`JobEntry::last_used`].
    clock: u64,
}

impl JobTable {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// Add a job and return its job number: one more than the highest in use, so
    /// numbering starts again from 1 once the table is empty (like bash and dash).
    ///
    /// The entry counts as reported: a running job has nothing to say yet, and
    /// the caller reports a job that stopped.
    pub fn insert(&mut self, job: Job, command: String) -> usize {
        let number = self.jobs.last_key_value().map_or(1, |(&n, _)| n + 1);
        let last_used = self.tick();

        self.jobs.insert(
            number,
            JobEntry {
                command,
                job,
                reported: true,
                last_used,
            },
        );

        number
    }

    pub fn remove(&mut self, number: usize) -> Option<JobEntry> {
        self.jobs.remove(&number)
    }

    /// Put back a job taken out with [`JobTable::remove`], under the same number
    /// (`fg` on a job that stops again). It becomes the most recent job, and the
    /// caller reports it.
    pub fn restore(&mut self, number: usize, job: Job, command: String) {
        let last_used = self.tick();

        self.jobs.insert(
            number,
            JobEntry {
                command,
                job,
                reported: true,
                last_used,
            },
        );
    }

    /// Make it the most recent job (`bg` resumed it).
    pub fn touch(&mut self, number: usize) {
        let stamp = self.tick();
        if let Some(entry) = self.jobs.get_mut(&number) {
            entry.last_used = stamp;
        }
    }

    pub fn get(&self, number: usize) -> Option<&JobEntry> {
        self.jobs.get(&number)
    }

    pub fn get_mut(&mut self, number: usize) -> Option<&mut JobEntry> {
        self.jobs.get_mut(&number)
    }

    pub fn numbers(&self) -> Vec<usize> {
        self.jobs.keys().copied().collect()
    }

    /// The job numbers by preference for the default of `fg` and `bg`: the
    /// stopped jobs first, then the most recent.
    fn ranked(&self) -> impl Iterator<Item = usize> {
        let mut ranked: Vec<(bool, u64, usize)> = self
            .jobs
            .iter()
            .map(|(&number, entry)| {
                let stopped = matches!(entry.job.state(), JobState::Stopped(_));
                (stopped, entry.last_used, number)
            })
            .collect();
        ranked.sort_unstable_by(|a, b| b.cmp(a));

        ranked.into_iter().map(|(_, _, number)| number)
    }

    /// The current job (`%+`), the default of `fg` and `bg`. If any job is
    /// stopped, it is a stopped one.
    pub fn current(&self) -> Option<usize> {
        self.ranked().next()
    }

    /// The previous job (`%-`). If at least two jobs are stopped, it is a stopped
    /// one too.
    pub fn previous(&self) -> Option<usize> {
        self.ranked().nth(1)
    }

    /// The `<current>` field of `jobs`: `+`, `-` or a space.
    pub fn marker(&self, number: usize) -> char {
        let mut ranked = self.ranked();

        if ranked.next() == Some(number) {
            '+'
        } else if ranked.next() == Some(number) {
            '-'
        } else {
            ' '
        }
    }

    /// The number of the job `id` names.
    pub fn find(&self, id: &JobId) -> Result<usize, JobIdError> {
        match id {
            JobId::Current => self.current().ok_or(JobIdError::NoSuchJob),
            JobId::Previous => self.previous().ok_or(JobIdError::NoSuchJob),
            JobId::Number(number) => self
                .jobs
                .contains_key(number)
                .then_some(*number)
                .ok_or(JobIdError::NoSuchJob),
            JobId::Prefix(prefix) => self.unique(|command| command.starts_with(prefix.as_str())),
            JobId::Contains(part) => self.unique(|command| command.contains(part.as_str())),
        }
    }

    /// The only job whose command matches.
    fn unique(&self, matches: impl Fn(&str) -> bool) -> Result<usize, JobIdError> {
        let mut found = self
            .jobs
            .iter()
            .filter(|(_, entry)| matches(&entry.command))
            .map(|(&number, _)| number);

        match (found.next(), found.next()) {
            (Some(number), None) => Ok(number),
            (None, _) => Err(JobIdError::NoSuchJob),
            (Some(_), Some(_)) => Err(JobIdError::Ambiguous),
        }
    }

    /// Collect, without blocking, what happened to the jobs. A job whose state
    /// changed has something to report.
    pub fn update_status(&mut self) {
        for entry in self.jobs.values_mut() {
            let before = entry.job.state();

            if let Err(e) = Waiter::update_status(&mut entry.job) {
                eprintln!("rush: wait: {e}");
            }

            if entry.job.state() != before {
                entry.reported = false;
            }
        }
    }

    /// Start reporting jobs to the user. The jobs shown through the report are
    /// marked reported, and the completed ones leave the table when the report
    /// is dropped.
    pub fn report(&mut self) -> JobReport<'_> {
        JobReport { table: self }
    }

    /// A line of `jobs`: `"[%d] %c %s %s"` (job number, current, state, command),
    /// with the process group ID before the state when `long` (`jobs -l`).
    fn format_line(&self, number: usize, long: bool) -> Option<String> {
        let entry = self.jobs.get(&number)?;
        let marker = self.marker(number);
        let state = entry.job.state();
        let command = &entry.command;

        Some(if long {
            let pgid = entry.job.leader().as_raw();
            format!("[{number}] {marker} {pgid} {state} {command}")
        } else {
            format!("[{number}] {marker} {state} {command}")
        })
    }
}

/// Jobs being reported to the user (`jobs`, the shell's own notifications).
///
/// Each line taken from the report marks its job reported. When the report is
/// dropped the reported jobs that completed leave the table: their status was
/// shown, and their pids are gone (POSIX: reporting the termination of a job
/// forgets its process ID). The table can still be read through the report.
pub struct JobReport<'t> {
    table: &'t mut JobTable,
}

impl JobReport<'_> {
    /// The `jobs` line of a job (see [`JobTable::format_line`]); the job is now
    /// reported.
    pub fn line(&mut self, number: usize, long: bool) -> Option<String> {
        let line = self.table.format_line(number, long)?;

        if let Some(entry) = self.table.jobs.get_mut(&number) {
            entry.reported = true;
        }
        Some(line)
    }

    /// The jobs whose last change was not reported yet, by job number. The
    /// iterator borrows the report: collect it before calling
    /// [`JobReport::line`].
    pub fn unreported(&self) -> impl Iterator<Item = usize> {
        self.table
            .jobs
            .iter()
            .filter(|(_, entry)| !entry.reported)
            .map(|(&number, _)| number)
    }
}

impl Deref for JobReport<'_> {
    type Target = JobTable;

    fn deref(&self) -> &JobTable {
        self.table
    }
}

impl Drop for JobReport<'_> {
    fn drop(&mut self) {
        self.table
            .jobs
            .retain(|_, entry| !entry.reported || !matches!(entry.job.state(), JobState::Done(_)));
    }
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

    /// The process that stands for the job (`jobs -p`): the leader of its process
    /// group when it has its own, otherwise its last process. Without job
    /// control the jobs share the shell's group, whose leader is the shell.
    pub fn leader(&self) -> Pid {
        match self.job_master {
            JobMaster::Process(pid) if !self.job_status.contains_key(&self.pgid) => pid,
            _ => self.pgid,
        }
    }

    /// What the job is doing, from the state of its processes.
    pub fn state(&self) -> JobState {
        // TODO: rewrite the other methods based on current state
        if let Some(exit) = self.completed_status() {
            JobState::Done(exit)
        } else if self.stopped() {
            JobState::Stopped(self.stop_signal().unwrap_or(Signal::SIGSTOP))
        } else {
            JobState::Running
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A job of one fake process, in this state. Nothing is spawned.
    fn job(pid: i32, status: ProcStatus) -> Job {
        let pid = Pid::from_raw(pid);
        let mut job = JobBuilder::new().build(pid, pid);
        job.job_status.insert(pid, status);
        job
    }

    fn running(pid: i32) -> Job {
        job(pid, ProcStatus::Running)
    }

    fn stopped(pid: i32) -> Job {
        job(pid, ProcStatus::Stopped(Signal::SIGTSTP))
    }

    #[test]
    fn numbers_follow_the_highest_and_restart_when_empty() {
        let mut table = JobTable::default();

        assert_eq!(table.insert(running(10), "a".into()), 1);
        assert_eq!(table.insert(running(11), "b".into()), 2);
        table.remove(1);
        assert_eq!(table.insert(running(12), "c".into()), 3);

        table.remove(2);
        table.remove(3);
        assert!(table.numbers().is_empty());
        assert_eq!(table.insert(running(13), "d".into()), 1);
    }

    #[test]
    fn current_and_previous_are_the_most_recent() {
        let mut table = JobTable::default();
        table.insert(running(10), "a".into());
        table.insert(running(11), "b".into());
        table.insert(running(12), "c".into());

        assert_eq!(table.current(), Some(3));
        assert_eq!(table.previous(), Some(2));

        table.touch(1);
        assert_eq!(table.current(), Some(1));
        assert_eq!(table.previous(), Some(3));
        assert_eq!(
            [table.marker(1), table.marker(2), table.marker(3)],
            ['+', ' ', '-']
        );
    }

    #[test]
    fn stopped_jobs_come_first() {
        let mut table = JobTable::default();
        table.insert(stopped(10), "a".into());
        table.insert(running(11), "b".into());
        table.insert(stopped(12), "c".into());
        table.insert(running(13), "d".into());

        // Two stopped jobs: both the current and the previous one are stopped.
        assert_eq!(table.current(), Some(3));
        assert_eq!(table.previous(), Some(1));
    }

    #[test]
    fn restore_keeps_the_number_and_makes_it_current() {
        let mut table = JobTable::default();
        table.insert(running(10), "a".into());
        table.insert(running(11), "b".into());

        let entry = table.remove(1).unwrap();
        table.restore(1, entry.job, entry.command);

        assert_eq!(table.numbers(), [1, 2]);
        assert_eq!(table.current(), Some(1));
    }

    #[test]
    fn find_every_job_id() {
        let mut table = JobTable::default();
        table.insert(running(10), "sleep 10".into());
        table.insert(running(11), "sleep 20".into());
        table.insert(running(12), "cat file".into());

        assert_eq!(table.find(&JobId::Current), Ok(3));
        assert_eq!(table.find(&JobId::Previous), Ok(2));
        assert_eq!(table.find(&JobId::Number(1)), Ok(1));
        assert_eq!(table.find(&JobId::Prefix("cat".into())), Ok(3));
        assert_eq!(table.find(&JobId::Contains("20".into())), Ok(2));

        assert_eq!(table.find(&JobId::Number(4)), Err(JobIdError::NoSuchJob));
        assert_eq!(
            table.find(&JobId::Prefix("ls".into())),
            Err(JobIdError::NoSuchJob)
        );
        assert_eq!(
            table.find(&JobId::Prefix("sleep".into())),
            Err(JobIdError::Ambiguous)
        );
        assert_eq!(
            table.find(&JobId::Contains("0".into())),
            Err(JobIdError::Ambiguous)
        );
    }

    #[test]
    fn an_empty_table_has_no_current_job() {
        let table = JobTable::default();

        assert_eq!(table.find(&JobId::Current), Err(JobIdError::NoSuchJob));
        assert_eq!(table.find(&JobId::Previous), Err(JobIdError::NoSuchJob));
    }

    #[test]
    fn lines_have_the_jobs_format() {
        let mut table = JobTable::default();
        table.insert(job(10, ProcStatus::Exited(0)), "true".into());
        table.insert(job(11, ProcStatus::Exited(3)), "exit 3".into());
        table.insert(job(12, ProcStatus::Signaled(Signal::SIGKILL)), "x".into());
        table.insert(running(13), "sleep 1".into());
        table.insert(stopped(14), "cat".into());

        let lines: Vec<String> = table
            .numbers()
            .into_iter()
            .map(|n| table.format_line(n, false).unwrap())
            .collect();

        assert_eq!(
            lines,
            [
                "[1]   Done true",
                "[2]   Done(3) exit 3",
                "[3]   Terminated (SIGKILL) x",
                "[4] - Running sleep 1",
                "[5] + Stopped (SIGTSTP) cat",
            ]
        );
        assert_eq!(
            table.format_line(5, true).unwrap(),
            "[5] + 14 Stopped (SIGTSTP) cat"
        );
    }

    #[test]
    fn the_leader_is_the_group_leader_or_the_last_process() {
        // Its own group, led by its first process.
        let mut builder = JobBuilder::new();
        builder.add_job(Pid::from_raw(10));
        let own = builder.build(Pid::from_raw(10), Pid::from_raw(11));
        assert_eq!(own.leader(), Pid::from_raw(10));

        // The shell's group (no job control).
        let shared = JobBuilder::new().build(Pid::from_raw(1), Pid::from_raw(11));
        assert_eq!(shared.leader(), Pid::from_raw(11));
    }

    #[test]
    fn a_report_drops_the_completed_jobs_it_showed() {
        let mut table = JobTable::default();
        table.insert(job(10, ProcStatus::Exited(0)), "a".into());
        table.insert(job(11, ProcStatus::Exited(0)), "b".into());
        table.insert(running(12), "c".into());
        table.jobs.get_mut(&1).unwrap().reported = false;
        table.jobs.get_mut(&2).unwrap().reported = false;

        {
            let mut report = table.report();
            assert_eq!(report.unreported().collect::<Vec<_>>(), [1, 2]);
            assert_eq!(report.line(1, false).unwrap(), "[1]   Done a");
            assert_eq!(report.line(3, false).unwrap(), "[3] + Running c");
            // Still in the table while the report lives.
            assert_eq!(report.numbers(), [1, 2, 3]);
            assert_eq!(report.unreported().collect::<Vec<_>>(), [2]);
        }

        // `1` was shown, `2` was not, `3` is still running.
        assert_eq!(table.numbers(), [2, 3]);
    }
}
