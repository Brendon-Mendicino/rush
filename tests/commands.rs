//! Running external commands, and commands that cannot be run.

mod common;

use common::{
    pty::{InteractiveShell, RED},
    run_shell, stderr, stdout,
};

#[test]
fn runs_a_command_with_arguments() {
    let output = run_shell("echo hello world\n");

    assert_eq!(stdout(&output), "hello world\n");
}

#[test]
fn reports_a_missing_command() {
    let output = run_shell("nonexistent-rush-cmd\necho after\n");

    assert!(stderr(&output).contains("rush: nonexistent-rush-cmd: command not found"));
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn missing_command_fails_without_killing_the_shell() {
    let mut sh = InteractiveShell::spawn();

    sh.send("nonexistent-rush-cmd\n");
    sh.stderr.expect("nonexistent-rush-cmd: command not found");
    assert!(sh.expect_prompt().contains(RED));
    assert_eq!(sh.foreground_pgid(), sh.pid());

    sh.send("echo alive\n");
    sh.stdout.expect("alive\r\n");
}
// giaveno coazze selvaggie
