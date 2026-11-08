//! The `bg` built-in.

use std::{
    fs, thread,
    time::{Duration, Instant},
};

use crate::common::{pty::InteractiveShell, run_shell, stderr, stdout};

/// The state letter of a process (`R`, `S`, `T`, ...), from `/proc/<pid>/stat`.
fn state(pid: i32) -> char {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let (_, rest) = stat.rsplit_once(") ").unwrap();
    rest.chars().next().unwrap()
}

/// Wait until the process is no longer stopped.
fn wait_continued(pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while state(pid) == 'T' {
        assert!(Instant::now() < deadline, "{pid} is still stopped");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn needs_job_control() {
    let output = run_shell("bg || echo failed\n");

    assert!(stderr(&output).contains("rush: bg: no job control"));
    assert_eq!(stdout(&output), "failed\n");
}

#[test]
fn continues_a_stopped_job_in_the_background() {
    let mut sh = InteractiveShell::spawn();

    sh.send("sleep 30\n");
    let pgid = sh.wait_foreground_job("sleep");
    sh.send("\x1a");
    sh.stderr.expect("[1] + Stopped (SIGTSTP) sleep 30\n");
    sh.expect_prompt();

    sh.send("bg\n");
    sh.stdout.expect("[1] sleep 30\r\n");
    sh.expect_prompt();
    assert_eq!(
        sh.foreground_pgid(),
        sh.pid(),
        "the shell keeps the terminal"
    );
    wait_continued(pgid);

    sh.send("jobs\n");
    sh.stdout.expect("[1] + Running sleep 30\r\n");

    // Already running: nothing to do, and it succeeds.
    sh.send("bg && echo ok\n");
    let output = sh.stdout.expect("ok\r\n");
    assert!(!output.contains("[1] sleep"), "{output:?}");

    unsafe { libc::kill(-pgid, libc::SIGKILL) };
}

#[test]
fn an_unknown_job_is_an_error() {
    let mut sh = InteractiveShell::spawn();

    sh.send("bg %3 || echo failed\n");
    sh.stderr.expect("rush: bg: %3: no such job\n");
    sh.stdout.expect("failed\r\n");
}
