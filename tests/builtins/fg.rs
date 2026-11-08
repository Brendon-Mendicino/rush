//! The `fg` built-in.

use crate::common::{pty::InteractiveShell, run_shell, stderr, stdout};

#[test]
fn needs_job_control() {
    let output = run_shell("sleep 0.3 &\nfg || echo failed\n");

    assert!(stderr(&output).contains("rush: fg: no job control"));
    assert_eq!(stdout(&output), "failed\n");
}

#[test]
fn without_jobs_is_an_error() {
    let mut sh = InteractiveShell::spawn();

    sh.send("fg || echo failed\n");
    sh.stderr.expect("rush: fg: no current job\n");
    sh.stdout.expect("failed\r\n");
}

#[test]
fn resumes_a_stopped_job_in_the_foreground() {
    let mut sh = InteractiveShell::spawn();

    sh.send("sleep 30\n");
    let pgid = sh.wait_foreground_job("sleep");
    sh.send("\x1a");
    sh.stderr.expect("[1] + Stopped (SIGTSTP) sleep 30\n");
    sh.expect_prompt();

    sh.send("fg\n");
    sh.stdout.expect("sleep 30\r\n");
    assert_eq!(
        sh.wait_foreground_job("sleep"),
        pgid,
        "the job has the terminal"
    );

    sh.send("\x03");
    sh.expect_prompt();
    assert_eq!(sh.foreground_pgid(), sh.pid(), "the shell takes it back");

    // The job is gone.
    sh.send("jobs\necho end\n");
    let output = sh.stdout.expect("end\r\n");
    assert!(!output.contains("[1]"), "{output:?}");
}

#[test]
fn restores_the_terminal_of_the_job() {
    let mut sh = InteractiveShell::spawn();

    sh.send("cat\n");
    sh.wait_foreground_job("cat");
    sh.send("\x1a");
    sh.stderr.expect("[1] + Stopped (SIGTSTP) cat\n");
    sh.expect_prompt();

    sh.send("fg %1\n");
    sh.stdout.expect("cat\r\n");
    sh.wait_foreground_job("cat");

    sh.send("typed\n");
    sh.stdout.expect("typed\r\n");
    sh.send("\x04");
    sh.expect_prompt();
}

#[test]
fn a_job_stopped_again_keeps_its_number() {
    let mut sh = InteractiveShell::spawn();

    sh.send("sleep 30\n");
    let pgid = sh.wait_foreground_job("sleep");
    sh.send("\x1a");
    sh.stderr.expect("[1] + Stopped (SIGTSTP) sleep 30\n");
    sh.expect_prompt();

    sh.send("fg %sleep\n");
    sh.wait_foreground_job("sleep");
    sh.send("\x1a");
    sh.stderr.expect("[1] + Stopped (SIGTSTP) sleep 30\n");
    sh.expect_prompt();

    unsafe { libc::kill(-pgid, libc::SIGKILL) };
}

#[test]
fn brings_a_background_job_to_the_foreground() {
    let mut sh = InteractiveShell::spawn();

    sh.send("sleep 30 &\n");
    sh.stderr.expect("[1] ");
    let pid: i32 = sh.stderr.expect("\n").trim().parse().unwrap();
    sh.expect_prompt();

    sh.send("fg\n");
    sh.stdout.expect("sleep 30\r\n");
    // The job is a forked shell running `sleep`: it leads the group.
    assert_eq!(sh.wait_foreground_process("sleep"), pid);

    sh.send("\x03");
    sh.expect_prompt();
    assert_eq!(sh.foreground_pgid(), sh.pid());
}
