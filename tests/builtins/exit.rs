//! The `exit` built-in.

use crate::common::{run_shell, stdout};

#[test]
fn exits_the_shell_with_the_status() {
    let output = run_shell("exit 4\necho after\n");

    assert_eq!(output.status.code(), Some(4));
    assert_eq!(stdout(&output), "");
}

#[test]
fn without_a_status_exits_with_0() {
    let output = run_shell("exit\necho after\n");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "");
}

#[test]
fn exits_only_a_subshell() {
    let output = run_shell("(exit 3) || echo failed\necho after\n");

    assert_eq!(stdout(&output), "failed\nafter\n");
}

#[test]
fn output_written_before_is_kept() {
    let output = run_shell("echo before; exit 2\n");

    assert_eq!(stdout(&output), "before\n");
    assert_eq!(output.status.code(), Some(2));
}
