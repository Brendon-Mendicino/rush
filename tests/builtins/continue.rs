//! The `continue` built-in.

use crate::common::{run_shell, stderr, stdout};

#[test]
fn skips_the_rest_of_the_body() {
    let output = run_shell("for i in 1 2 3; do echo $i; continue; echo no; done\n");

    assert_eq!(stdout(&output), "1\n2\n3\n");
}

#[test]
fn goes_on_with_the_nth_loop() {
    let output =
        run_shell("for i in a b; do for j in 1 2; do echo $i$j; continue 2; done; echo no; done\n");

    assert_eq!(stdout(&output), "a1\nb1\n");
}

#[test]
fn its_status_is_zero() {
    let output = run_shell("for i in 1 2; do false; continue; done; echo $?\n");

    assert_eq!(stdout(&output), "0\n");
}

#[test]
fn outside_a_loop_it_does_nothing() {
    let output = run_shell("continue; echo after st=$?\n");

    assert_eq!(stdout(&output), "after st=0\n");
}

#[test]
fn an_invalid_count_exits_a_script() {
    let output = run_shell("for i in 1 2; do continue 0; done\necho after\n");

    assert!(stderr(&output).contains("0: loop count out of range"));
    assert_eq!(stdout(&output), "");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn runs_the_while_condition_again() {
    let output = run_shell(
        "set -- a b c; while [ $# -gt 0 ]; do x=$1; shift; [ $x = b ] && continue; echo $x; done\n",
    );

    assert_eq!(stdout(&output), "a\nc\n");
}

#[test]
fn in_the_condition_runs_it_again() {
    // Like dash and bash: back to the top of the loop, not into the body.
    let output = run_shell(
        "set -- a b c; while shift; [ $# -gt 1 ] && continue; [ $# -gt 0 ]; do echo $#; done\n",
    );

    assert_eq!(stdout(&output), "1\n");
}

#[test]
fn goes_on_with_a_for_around_a_while() {
    let output =
        run_shell("for i in a b; do while true; do echo $i; continue 2; done; echo no; done\n");

    assert_eq!(stdout(&output), "a\nb\n");
}
