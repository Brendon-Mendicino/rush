//! `bg` - run jobs in the background (regular built-in).
//!
//! ```text
//! bg [job_id...]
//! ```
//!
//! Spec: `docs/utilities/bg.html`.

use clap::Parser;

use crate::{
    builtin::{Builtin, BuiltinResult, ENDL, Status, find_job, parse_job_id},
    job::{JobId, JobState},
    nix,
    waiter::Waiter,
};

/// Continue stopped jobs in the background.
///
/// Continue each job named by JOB_ID, or the current job, as a background job,
/// and print it as [number] command. A job already running in the background is
/// left as it is. JOB_ID is one of the forms listed by jobs --help.
///
/// Only with job control (an interactive shell).
///
/// Exit status:
///   0   success
///   >0  no job control, or a JOB_ID names no job or a job that terminated
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "bg [job_id...]")]
struct Args {
    #[arg(value_parser = parse_job_id)]
    job_ids: Vec<JobId>,
}

pub fn bg(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;

    if !bi.shell.is_interactive {
        return Err("no job control".into());
    }

    bi.jobs.update_status();

    let ids: Vec<Option<&JobId>> = if args.job_ids.is_empty() {
        vec![None]
    } else {
        args.job_ids.iter().map(Some).collect()
    };

    let mut status = Status::SUCCESS;
    for id in ids {
        if let Err(message) = resume(bi, id) {
            bi.report(message)?;
            status = Status::FAILURE;
        }
    }

    Ok(status)
}

/// Continue one job in the background; the error is the diagnostic.
fn resume(bi: &mut Builtin, id: Option<&JobId>) -> Result<(), String> {
    let number = find_job(bi.jobs, id)?;
    let Some(entry) = bi.jobs.get_mut(number) else {
        return Err(format!("%{number}: no such job"));
    };

    match entry.job.state() {
        // Already a running background job: no effect.
        JobState::Running => return Ok(()),
        JobState::Done(_) => return Err(format!("%{number}: job has terminated")),
        JobState::Stopped(_) => {}
    }

    Waiter::new(bi.shell)
        .bg(&mut entry.job)
        .map_err(|e| format!("%{number}: {}", nix::error_message(&e)))?;

    let line = format!("[{number}] {}{ENDL}", entry.command);
    bi.jobs.touch(number);

    bi.stdout
        .write_all(line.as_bytes())
        .map_err(|e| nix::error_message(&e))
}
