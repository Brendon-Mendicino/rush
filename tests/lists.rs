//! Lists: `a && b`, `a || b` (and-or lists) and `a; b`.

mod common;

use common::{
    pty::{InteractiveShell, RED},
    run_shell, stdout,
};

/// A command killed by `sig` (the `sh` kills itself).
fn killed_by(sig: &str) -> String {
    format!("sh -c 'kill -{sig} $$'")
}

#[test]
fn and_runs_the_rhs_only_on_success() {
    let output = run_shell("true && echo a\nfalse && echo b\n");

    assert_eq!(stdout(&output), "a\n");
}

#[test]
fn or_runs_the_rhs_only_on_failure() {
    let output = run_shell("false || echo a\ntrue || echo b\n");

    assert_eq!(stdout(&output), "a\n");
}

#[test]
fn and_or_lists_are_left_associative() {
    // `&&` and `||` have the same precedence: `(a && b) || c`.
    let output = run_shell(
        "false && echo no || echo a\ntrue || echo no && echo b\nfalse || false && echo no\n",
    );

    assert_eq!(stdout(&output), "a\nb\n");
}

#[test]
fn a_missing_command_is_a_failure() {
    let output = run_shell("nonexistent-rush-cmd || echo a\nnonexistent-rush-cmd && echo no\n");

    assert_eq!(stdout(&output), "a\n");
}

#[test]
fn a_signaled_command_is_a_failure() {
    let output = run_shell(&format!(
        "{} || echo a\n{} && echo no\n",
        killed_by("TERM"),
        killed_by("TERM")
    ));

    assert_eq!(stdout(&output), "a\n");
}

#[test]
fn sigint_stops_an_and_or_list() {
    // Interrupting the lhs interrupts the whole list, even when its failure
    // would run the rhs.
    let output = run_shell(&format!(
        "{} || echo no\n{} && echo no\necho after\n",
        killed_by("INT"),
        killed_by("INT")
    ));

    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn sigint_stops_an_and_or_list_through_a_bang() {
    let output = run_shell(&format!("! {} && echo no\necho after\n", killed_by("INT")));

    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn and_or_lists_continue_on_the_next_line() {
    let output = run_shell("true &&\necho a\nfalse ||\necho b\n");

    assert_eq!(stdout(&output), "a\nb\n");
}

#[test]
fn and_or_lists_inside_a_sequential_list() {
    let output = run_shell("false && echo no; echo a || echo no; true && echo b\n");

    assert_eq!(stdout(&output), "a\nb\n");
}

#[test]
fn ctrl_c_stops_an_or_list() {
    let mut sh = InteractiveShell::spawn();

    sh.send("sleep 30 || echo rhs\n");
    sh.wait_foreground_job("sleep");
    sh.send("\x03");

    let output = sh.expect_prompt();
    assert!(!output.contains("rhs"), "the rhs ran: {output:?}");
    assert!(output.contains(RED));
}

#[test]
fn ctrl_z_counts_as_a_failure() {
    let mut sh = InteractiveShell::spawn();

    sh.send("sleep 30 && echo and-rhs || echo or-rhs\n");
    let pgid = sh.wait_foreground_job("sleep");
    sh.send("\x1a");

    let output = sh.expect_prompt();
    assert!(!output.contains("and-rhs"), "{output:?}");
    assert!(output.contains("or-rhs"), "{output:?}");

    unsafe { libc::kill(-pgid, libc::SIGKILL) };
}
