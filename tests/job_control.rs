//! Job control: process groups, the terminal and signals in interactive mode (on
//! a pty), and their absence in non-interactive mode.

mod common;

use std::fs;

use common::{
    pty::{InteractiveShell, RED},
    run_shell, stdout,
};

/// `pid`, `pgrp` and `tpgid` (foreground group of the terminal) from a line of
/// `/proc/<pid>/stat`, as printed by `cat /proc/self/stat`.
///
/// The line is `pid (comm) state ppid pgrp session tty_nr tpgid ...`: `pgrp`
/// tells which process group `cat` runs in, `tpgid` which group owns its
/// terminal. The kernel fills them in while `cat` runs, so they show the state
/// of the job, not of the shell.
fn stat_ids(output: &str) -> (i32, i32, i32) {
    let line = output.lines().find(|l| l.contains(" (cat) ")).unwrap();
    let (pid, rest) = line.split_once(" (cat) ").unwrap();
    // state ppid pgrp session tty_nr tpgid
    let fields: Vec<_> = rest.split_whitespace().collect();

    (
        pid.parse().unwrap(),
        fields[2].parse().unwrap(),
        fields[5].parse().unwrap(),
    )
}

/// The `SigIgn` mask from a `/proc/<pid>/status` file: a hex bitmask of the
/// signals the process ignores (`SIG_IGN`), bit `n - 1` for signal `n`. Ignored
/// signals survive exec, so a child that is not reset shows the shell's ones.
fn ignored_signals(status: &str) -> u64 {
    let line = status.lines().find(|l| l.starts_with("SigIgn:")).unwrap();
    u64::from_str_radix(line["SigIgn:".len()..].trim(), 16).unwrap()
}

/// The `SigIgn` bits of the signals an interactive shell ignores and its
/// children must reset to default.
fn job_control_mask() -> u64 {
    [
        libc::SIGINT,
        libc::SIGQUIT,
        libc::SIGTSTP,
        libc::SIGTTIN,
        libc::SIGTTOU,
    ]
    .iter()
    .fold(0, |mask, sig| mask | 1 << (sig - 1))
}

#[test]
fn non_interactive_children_stay_in_the_shell_group() {
    let output = run_shell("cat /proc/self/stat\n");

    // pid (comm) state ppid pgrp ...
    let stat = stdout(&output);
    let (_, rest) = stat.split_once(") ").unwrap();
    let pgrp: i32 = rest.split_whitespace().nth(2).unwrap().parse().unwrap();

    // The shell was spawned in the test's group and does no job control.
    // `getpgrp()` is the process group of the calling process (the test).
    assert_eq!(pgrp, unsafe { libc::getpgrp() });
}

#[test]
fn foreground_job_gets_its_own_group_and_the_terminal() {
    let mut sh = InteractiveShell::spawn();

    sh.send("cat /proc/self/stat\n");
    let (pid, pgrp, tpgid) = stat_ids(&sh.expect_prompt());
    assert_eq!(pgrp, pid, "a single command leads its group");
    assert_ne!(pgrp, sh.pid(), "the job is not in the shell's group");
    assert_eq!(tpgid, pgrp, "the job owns the terminal");

    sh.send("sleep 0 | cat /proc/self/stat\n");
    let (pid, pgrp, tpgid) = stat_ids(&sh.expect_prompt());
    assert_ne!(
        pgrp, pid,
        "the first command of the pipeline leads the group"
    );
    assert_ne!(pgrp, sh.pid());
    assert_eq!(tpgid, pgrp);

    assert_eq!(
        sh.foreground_pgid(),
        sh.pid(),
        "the shell takes the terminal back"
    );
}

#[test]
fn foreground_job_can_read_the_terminal() {
    let mut sh = InteractiveShell::spawn();

    sh.send("cat\n");
    sh.wait_foreground_job("cat");
    sh.send("typed\n");
    sh.stdout.expect("typed\r\n");
    // End of input for cat.
    sh.send("\x04");

    assert!(!sh.expect_prompt().contains(RED), "cat succeeded");
}

#[test]
fn children_restore_the_default_signal_handling() {
    let mut sh = InteractiveShell::spawn();
    let mask = job_control_mask();

    let shell_status = fs::read_to_string(format!("/proc/{}/status", sh.pid())).unwrap();
    assert_eq!(
        ignored_signals(&shell_status) & mask,
        mask,
        "the shell ignores them"
    );

    sh.send("grep SigIgn /proc/self/status\n");
    assert_eq!(ignored_signals(&sh.expect_prompt()) & mask, 0);

    sh.send("grep SigIgn /proc/self/status | cat\n");
    assert_eq!(ignored_signals(&sh.expect_prompt()) & mask, 0);
}

#[test]
fn ctrl_c_interrupts_the_foreground_job() {
    let mut sh = InteractiveShell::spawn();

    sh.send("sleep 30\n");
    sh.wait_foreground_job("sleep");
    sh.send("\x03");

    sh.stderr.expect("Terminated by signal SIGINT");
    assert!(sh.expect_prompt().contains(RED), "the status is 130");

    sh.send("echo alive\n");
    sh.stdout.expect("alive\r\n");
}

#[test]
fn ctrl_z_stops_the_foreground_job() {
    let mut sh = InteractiveShell::spawn();

    sh.send("sleep 30\n");
    let pgid = sh.wait_foreground_job("sleep");
    sh.send("\x1a");

    // Reported in the format of `jobs`.
    sh.stderr.expect("[1] + Stopped (SIGTSTP) sleep 30\n");
    sh.expect_prompt();
    assert_eq!(
        sh.foreground_pgid(),
        sh.pid(),
        "the shell takes the terminal back"
    );

    let stat = fs::read_to_string(format!("/proc/{pgid}/stat")).unwrap();
    assert!(stat.contains(") T "), "sleep is stopped: {stat}");

    sh.send("echo alive\n");
    sh.stdout.expect("alive\r\n");

    // Don't leave the stopped `sleep` around: a negative pid sends the signal
    // to the whole process group. SIGKILL works on stopped processes too.
    unsafe { libc::kill(-pgid, libc::SIGKILL) };
}

#[test]
fn subshell_commands_join_the_job_group() {
    let mut sh = InteractiveShell::spawn();

    // `cat` is a child of the forked subshell, which leads the job's group.
    sh.send("(cat /proc/self/stat)\n");
    let (pid, pgrp, tpgid) = stat_ids(&sh.expect_prompt());
    assert_ne!(pgrp, pid, "the subshell leads the group, not cat");
    assert_ne!(pgrp, sh.pid());
    assert_eq!(tpgid, pgrp, "the job owns the terminal");

    assert_eq!(sh.foreground_pgid(), sh.pid());
}

#[test]
fn subshell_can_read_the_terminal() {
    let mut sh = InteractiveShell::spawn();

    sh.send("(cat)\n");
    sh.wait_foreground_process("cat");
    sh.send("typed\n");
    sh.stdout.expect("typed\r\n");
    sh.send("\x04");

    assert!(!sh.expect_prompt().contains(RED));
}

#[test]
fn ctrl_c_interrupts_a_subshell() {
    let mut sh = InteractiveShell::spawn();

    sh.send("(sleep 30; echo after)\n");
    sh.wait_foreground_process("sleep");
    sh.send("\x03");

    let output = sh.expect_prompt();
    assert!(
        !output.contains("after"),
        "the subshell went on: {output:?}"
    );
    assert!(output.contains(RED));
}

#[test]
fn ctrl_z_stops_a_subshell() {
    let mut sh = InteractiveShell::spawn();

    sh.send("(sleep 30; echo after)\n");
    let pgid = sh.wait_foreground_process("sleep");
    sh.send("\x1a");

    sh.stderr
        .expect("[1] + Stopped (SIGTSTP) (sleep 30; echo after)\n");
    let output = sh.expect_prompt();
    assert!(!output.contains("after"), "{output:?}");
    assert_eq!(sh.foreground_pgid(), sh.pid());

    // The subshell itself is stopped, not only its child.
    let stat = fs::read_to_string(format!("/proc/{pgid}/stat")).unwrap();
    assert!(stat.contains(") T "), "the subshell is not stopped: {stat}");

    unsafe { libc::kill(-pgid, libc::SIGKILL) };
}

#[test]
fn async_job_does_not_take_the_terminal() {
    let mut sh = InteractiveShell::spawn();

    sh.send("sleep 30 &\n");
    // `[job number] pid`
    sh.stderr.expect("[1] ");
    let pid: i32 = sh.stderr.expect("\n").trim().parse().unwrap();

    sh.expect_prompt();
    assert_eq!(sh.foreground_pgid(), sh.pid());

    // Its own process group, like any job.
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let (_, rest) = stat.rsplit_once(") ").unwrap();
    let pgrp: i32 = rest.split_whitespace().nth(2).unwrap().parse().unwrap();
    assert_eq!(pgrp, pid);

    unsafe { libc::kill(-pid, libc::SIGKILL) };
}

#[test]
fn completed_background_jobs_are_reported_before_the_prompt() {
    let mut sh = InteractiveShell::spawn();

    sh.send("sleep 0.1 &\n");
    sh.stderr.expect("[1] ");
    sh.expect_prompt();

    sh.send("sleep 0.5\n");
    sh.stderr.expect("[1] + Done sleep 0.1\n");
    sh.expect_prompt();

    // Reported once, then forgotten.
    sh.send("jobs\necho end\n");
    let output = sh.stdout.expect("end\r\n");
    assert!(!output.contains("[1]"), "{output:?}");
}

#[test]
fn ctrl_z_does_not_stop_a_command_substitution() {
    let mut sh = InteractiveShell::spawn();

    sh.send("echo $(sleep 0.5 && echo test)\n");
    // Not a job: it runs in the shell's group, which keeps the terminal.
    sh.wait_shell_group_process("sleep");
    assert_eq!(sh.foreground_pgid(), sh.pid());
    sh.send("\x1a");

    // Ignored: the substitution completes and the shell goes on.
    sh.stdout.expect("test\r\n");
    sh.expect_prompt();
    assert_eq!(sh.foreground_pgid(), sh.pid());

    sh.send("jobs\necho end\n");
    let output = sh.stdout.expect("end\r\n");
    assert!(!output.contains("[1]"), "not in the job table: {output:?}");
}

#[test]
fn ctrl_c_interrupts_a_command_substitution() {
    let mut sh = InteractiveShell::spawn();

    sh.send("echo $(sleep 30; echo test) after\n");
    sh.wait_shell_group_process("sleep");
    sh.send("\x03");

    // The shell survives and gets the terminal back.
    sh.expect_prompt();
    assert_eq!(sh.foreground_pgid(), sh.pid());
    sh.send("echo alive\n");
    sh.stdout.expect("alive\r\n");
}
