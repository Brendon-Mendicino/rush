//! The `true` built-in.

use crate::common::{run_shell, stderr, stdout};

#[test]
fn succeeds() {
    let output = run_shell("true && echo a\ntrue || echo b\n");

    assert_eq!(stdout(&output), "a\n");
}

#[test]
fn ignores_its_arguments() {
    let output = run_shell("true x --help -n && echo a\n");

    assert_eq!(stdout(&output), "a\n");
}

#[test]
fn is_not_looked_up_in_path() {
    let output = run_shell("PATH=/rush-nonexistent true && echo a\n");

    assert_eq!(stdout(&output), "a\n");
    assert!(!stderr(&output).contains("command not found"));
}

#[test]
fn negated_fails() {
    let output = run_shell("! true || echo a\n");

    assert_eq!(stdout(&output), "a\n");
}
