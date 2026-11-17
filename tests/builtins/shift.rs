//! The `shift` built-in.

use crate::common::{run_shell, stderr, stdout};

#[test]
fn drops_the_first_parameters() {
    let output =
        run_shell("set -- a b c d\nshift\necho $# $1\nshift 2\necho $# $1\nshift 0\necho $#\n");

    assert_eq!(stdout(&output), "3 b\n1 d\n1\n");
}

#[test]
fn shifting_too_many_exits_a_script() {
    let output = run_shell("set -- a\nshift 2\necho after\n");

    assert!(stderr(&output).contains("rush: shift: 2: shift count out of range"));
    assert_eq!(stdout(&output), "");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn in_a_pipeline_only_the_child_shifts() {
    let output = run_shell("set -- a b\nshift | cat\necho $1\nshift >/dev/null\necho $1\n");

    assert_eq!(stdout(&output), "a\nb\n");
}
