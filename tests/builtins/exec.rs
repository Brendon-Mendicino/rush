//! The `exec` built-in.

use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use crate::common::{
    pty::InteractiveShell, run, run_shell, run_shell_in, shell_command, stderr, stdout, test_dir,
};

#[test]
fn replaces_the_shell() {
    let output = run_shell("exec echo hi\necho after\n");

    assert_eq!(stdout(&output), "hi\n");
}

#[test]
fn the_shell_exits_with_the_status_of_the_command() {
    let output = run_shell("exec sh -c 'exit 3'\n");

    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn skips_a_leading_double_dash() {
    let output = run_shell("exec -- echo hi\n");

    assert_eq!(stdout(&output), "hi\n");
}

#[test]
fn passes_the_exported_variables_and_the_prefixes() {
    let mut command = shell_command(Path::new(env!("CARGO_TARGET_TMPDIR")));
    command.env("RUSH_OUTER", "outer");

    let output = run(command, "RUSH_A=a exec printenv RUSH_OUTER RUSH_A\n");

    assert_eq!(stdout(&output), "outer\na\n");
}

#[test]
fn the_command_gets_the_redirections() {
    let dir = test_dir("exec_command_redirection");

    let output = run_shell_in("exec echo hi >out\n", &dir);

    assert_eq!(stdout(&output), "");
    assert_eq!(fs::read_to_string(dir.join("out")).unwrap(), "hi\n");
}

#[test]
fn a_missing_command_fails_and_the_shell_goes_on() {
    let output = run_shell("exec nonexistent-rush-cmd || echo failed\necho after\n");

    assert!(stderr(&output).contains("rush: nonexistent-rush-cmd: command not found"));
    assert_eq!(stdout(&output), "failed\nafter\n");
}

#[test]
fn a_failed_exec_puts_the_fds_back() {
    let dir = test_dir("exec_restore_fds");

    let output = run_shell_in("exec nonexistent-rush-cmd >out || echo failed\n", &dir);

    assert_eq!(stdout(&output), "failed\n");
    assert_eq!(fs::read_to_string(dir.join("out")).unwrap(), "");
}

#[test]
fn a_file_that_is_not_executable_fails() {
    let dir = test_dir("exec_not_executable");
    fs::write(dir.join("script"), "echo no\n").unwrap();
    fs::set_permissions(dir.join("script"), fs::Permissions::from_mode(0o644)).unwrap();

    let output = run_shell_in("exec ./script || echo failed\n", &dir);

    assert!(stderr(&output).contains("rush: ./script: Permission denied"));
    assert_eq!(stdout(&output), "failed\n");
}

#[test]
fn without_command_succeeds() {
    let output = run_shell("false\nexec && echo a\n");

    assert_eq!(stdout(&output), "a\n");
}

#[test]
fn without_command_the_redirections_stay() {
    let dir = test_dir("exec_permanent_stdout");

    let output = run_shell_in("exec >out\necho a\necho b\n", &dir);

    assert_eq!(stdout(&output), "");
    assert_eq!(fs::read_to_string(dir.join("out")).unwrap(), "a\nb\n");
}

#[test]
fn opens_and_closes_fds_for_the_next_commands() {
    let dir = test_dir("exec_fd_3");

    let output = run_shell_in(
        "exec 3>out\necho a >&3\necho b >&3\nexec 3>&-\necho c >&3 || echo failed\n",
        &dir,
    );

    assert!(stderr(&output).contains("3: Bad file descriptor"));
    assert_eq!(stdout(&output), "failed\n");
    assert_eq!(fs::read_to_string(dir.join("out")).unwrap(), "a\nb\n");
}

#[test]
fn redirects_the_errors_of_the_shell() {
    let dir = test_dir("exec_stderr");

    run_shell_in("exec 2>err\nnonexistent-rush-cmd\n", &dir);

    let err = fs::read_to_string(dir.join("err")).unwrap();
    assert!(err.contains("rush: nonexistent-rush-cmd: command not found"));
}

#[test]
fn a_failed_redirection_fails() {
    let dir = test_dir("exec_bad_redirection");

    let output = run_shell_in("exec <missing || echo failed\necho after\n", &dir);

    assert!(stderr(&output).contains("missing"));
    assert_eq!(stdout(&output), "failed\nafter\n");
}

#[test]
fn replaces_only_the_child_in_a_pipeline() {
    let output = run_shell("exec echo a | cat\nsleep 0 | exec echo b\necho after\n");

    assert_eq!(stdout(&output), "a\nb\nafter\n");
}

#[test]
fn the_command_gets_the_default_signals() {
    let mut sh = InteractiveShell::spawn();

    sh.send("exec grep SigIgn /proc/self/status\n");
    let line = sh.stdout.expect("\n");

    // The signals an interactive shell ignores are not ignored any more: bit
    // `n - 1` of the mask is signal `n`.
    let mask = u64::from_str_radix(line.trim().trim_start_matches("SigIgn:").trim(), 16).unwrap();
    let job_control = [
        libc::SIGINT,
        libc::SIGQUIT,
        libc::SIGTSTP,
        libc::SIGTTIN,
        libc::SIGTTOU,
    ]
    .iter()
    .fold(0, |mask, sig| mask | 1 << (sig - 1));
    assert_eq!(mask & job_control, 0, "{line}");
}

#[test]
fn after_a_failed_exec_the_shell_still_ignores_ctrl_c() {
    let mut sh = InteractiveShell::spawn();

    sh.send("exec nonexistent-rush-cmd\n");
    sh.expect_prompt();
    sh.send("\x03");
    sh.send("echo alive\n");

    sh.stdout.expect("alive");
}

#[test]
fn a_redirection_to_the_terminal_fd_keeps_job_control_working() {
    // The interactive shell keeps its copy of the terminal on a high fd, 10 when
    // it starts: `exec 10>…` must move it, not overwrite it.
    let mut sh = InteractiveShell::spawn();

    sh.send("exec 10>/dev/null\n");
    sh.expect_prompt();

    sh.send("cat\n");
    sh.wait_foreground_job("cat");
    sh.send("typed\n");
    sh.stdout.expect("typed\r\n");
    sh.send("\x04");
    sh.expect_prompt();

    assert_eq!(sh.foreground_pgid(), sh.pid());
}
