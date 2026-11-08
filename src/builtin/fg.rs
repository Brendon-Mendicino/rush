//! `fg` - run jobs in the foreground (regular built-in).
//!
//! ```text
//! fg [job_id]
//! ```
//!
//! Spec: `docs/utilities/fg.html`.

use clap::Parser;

use crate::{
    builtin::{Builtin, BuiltinResult, ENDL, Status, find_job, parse_job_id},
    job::{JobId, JobState},
    waiter::{WaitResult, Waiter},
};

/// Run a job in the foreground.
///
/// Print the command of the job, give it the terminal, continue it if it is
/// stopped and wait for it. Without JOB_ID, the current job is used: the job
/// most recently stopped, or else the one most recently started in the
/// background. JOB_ID is one of the forms listed by jobs --help.
///
/// Only with job control (an interactive shell).
///
/// Exit status:
///   the status of the job: 128 + n when signal n stopped or killed it
///   >0  no job control, or JOB_ID names no job
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "fg [job_id]")]
struct Args {
    #[arg(value_parser = parse_job_id)]
    job_id: Option<JobId>,
}

pub fn fg(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;

    if !bi.shell.is_interactive {
        return Err("no job control".into());
    }

    bi.jobs.update_status();
    let number = find_job(bi.jobs, args.job_id.as_ref())?;

    if let Some(entry) = bi.jobs.get(number)
        && matches!(entry.job.state(), JobState::Done(_))
    {
        // Left in the table: the shell reports it before the next prompt.
        return Err(format!("%{number}: job has terminated").into());
    }

    let Some(entry) = bi.jobs.remove(number) else {
        return Err(format!("%{number}: no such job").into());
    };

    // Written before the job owns the terminal.
    write!(bi.stdout, "{}{ENDL}", entry.command)?;
    bi.stdout.flush()?;

    match Waiter::new(bi.shell).fg(entry.job)? {
        WaitResult::Completed(exit) => Ok(Status::Exited(exit.code())),
        WaitResult::Stopped { job, sig_stop } => {
            bi.jobs.restore(number, job, entry.command);
            if let Some(line) = bi.jobs.report().line(number, false) {
                write!(bi.stderr, "{line}{ENDL}")?;
            }
            Ok(Status::Exited(128 + sig_stop as i32))
        }
    }
}
