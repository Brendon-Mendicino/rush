//! `jobs` - display status of jobs in the current session (regular built-in).
//!
//! ```text
//! jobs [-l|-p] [job_id...]
//! ```
//!
//! Spec: `docs/utilities/jobs.html`.

use clap::Parser;

use crate::{
    builtin::{Builtin, BuiltinResult, ENDL, Status, parse_job_id},
    job::JobId,
};

/// Display the status of jobs.
///
/// Print the jobs of the shell, or the ones named by JOB_ID, one per line:
///   [number] current state command
/// where current is + for the current job, - for the previous one. JOB_ID is
/// %n (job number n), %% or %+ (the current job), %- (the previous job),
/// %string (the job whose command begins with string) or %?string (the job
/// whose command contains string).
///
/// A completed job is reported once, then forgotten.
///
/// Exit status:
///   0   success
///   >0  a JOB_ID names no job
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "jobs [-l|-p] [job_id...]")]
struct Args {
    /// Also print the process ID of each job (its process group leader).
    #[arg(short = 'l', conflicts_with = "pids")]
    long: bool,
    /// Print only the process ID of each job (its process group leader).
    #[arg(short = 'p')]
    pids: bool,
    #[arg(value_parser = parse_job_id)]
    job_ids: Vec<JobId>,
}

pub fn jobs(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;

    bi.jobs.update_status();

    let mut status = Status::SUCCESS;
    let numbers = if args.job_ids.is_empty() {
        bi.jobs.numbers()
    } else {
        let mut numbers = Vec::new();
        for id in &args.job_ids {
            match bi.jobs.find(id) {
                Ok(number) => numbers.push(number),
                Err(e) => {
                    bi.report(format!("{id}: {e}"))?;
                    status = Status::FAILURE;
                }
            }
        }
        numbers
    };

    for number in numbers {
        if args.pids {
            if let Some(entry) = bi.jobs.get(number) {
                write!(bi.stdout, "{}{ENDL}", entry.job.leader().as_raw())?;
            }
            continue;
        }

        if let Some(line) = bi.jobs.format_line(number, args.long) {
            write!(bi.stdout, "{line}{ENDL}")?;
        }
        if let Some(entry) = bi.jobs.get_mut(number) {
            entry.reported = true;
        }
    }

    // Reporting the termination of a job forgets it.
    bi.jobs.forget_reported_done();

    Ok(status)
}
