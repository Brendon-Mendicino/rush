//! Async lists: `list &` runs the list in a forked shell without waiting for it.

mod common;

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};

use common::{run_shell, shell_command, stderr, stdout};

#[test]
fn does_not_wait_for_the_list() {
    let output = run_shell("(sleep 0.3; echo later) &\necho now\n");

    assert_eq!(stdout(&output), "now\nlater\n");
    // Without job control the shell doesn't announce the job.
    assert!(
        !stderr(&output).lines().any(|line| line.starts_with("[1]")),
        "{}",
        stderr(&output)
    );
}

#[test]
fn status_is_success() {
    let output = run_shell("false & echo started\n");

    assert_eq!(stdout(&output), "started\n");
}

#[test]
fn reads_from_dev_null_without_job_control() {
    let output = run_shell("readlink /proc/self/fd/0 &\n");

    assert_eq!(stdout(&output), "/dev/null\n");
}

#[test]
fn ignores_sigint_and_sigquit_without_job_control() {
    let output = run_shell("grep SigIgn /proc/self/status &\n");

    let line = stdout(&output);
    let mask = u64::from_str_radix(line.trim_start_matches("SigIgn:").trim(), 16).unwrap();
    for sig in [libc::SIGINT, libc::SIGQUIT] {
        assert_ne!(
            mask & 1 << (sig - 1),
            0,
            "signal {sig} is not ignored: {line}"
        );
    }
}

#[test]
fn foreground_wait_is_not_confused_by_a_background_job() {
    // Without job control both jobs are in the shell's group: waiting on the
    // group would reap the background job while waiting for `sleep 0.3`.
    let output = run_shell("sleep 0.1 &\nsleep 0.3 && echo waited\n");

    assert_eq!(stdout(&output), "waited\n");
    assert!(
        !stderr(&output).contains("rush: wait"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn finished_jobs_are_reaped() {
    let mut child = shell_command(Path::new(env!("CARGO_TARGET_TMPDIR")))
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    // The background job is done by the time `sleep` ends, and it is reaped
    // before `echo` runs.
    stdin
        .write_all(b"true &\nsleep 0.2\necho reaped\n")
        .unwrap();
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert_eq!(line, "reaped\n");

    // A zombie stays among the children until it is reaped. `echo` itself may
    // still be waited for a moment.
    let children = format!("/proc/{0}/task/{0}/children", child.id());
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let pending = fs::read_to_string(&children).unwrap();
        if pending.trim().is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "children never reaped: {pending}"
        );
        thread::sleep(Duration::from_millis(10));
    }

    drop(stdin);
    child.wait().unwrap();
}
