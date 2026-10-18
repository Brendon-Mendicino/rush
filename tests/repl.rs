//! The read-eval loop: input lines, continuations, syntax errors.

mod common;

use common::{run_shell, stderr, stdout};

#[test]
fn exits_cleanly_at_eof() {
    let output = run_shell("");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "");
}

#[test]
fn runs_sequential_commands_in_order() {
    let output = run_shell("echo one; echo two\necho three\n");

    assert_eq!(stdout(&output), "one\ntwo\nthree\n");
}

#[test]
fn skips_empty_lines() {
    let output = run_shell("\n\necho x\n\n");

    assert_eq!(stdout(&output), "x\n");
}

#[test]
fn joins_line_continuations() {
    let output = run_shell("echo a \\\nb\n");

    assert_eq!(stdout(&output), "a b\n");
}

#[test]
fn recovers_from_a_syntax_error() {
    let output = run_shell(")\necho after\n");

    assert!(output.status.success());
    assert!(stderr(&output).contains("syntax error"));
    assert_eq!(stdout(&output), "after\n");
}
