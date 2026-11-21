//! The `break` built-in.

use crate::common::{run_shell, stderr, stdout};

#[test]
fn leaves_the_loop() {
    let output = run_shell("for i in 1 2 3; do echo $i; break; echo no; done; echo st=$?\n");

    assert_eq!(stdout(&output), "1\nst=0\n");
}

#[test]
fn leaves_n_loops() {
    let output = run_shell(
        "for i in a b; do for j in 1 2; do echo $i$j; break 2; done; echo no; done; echo out\n",
    );

    assert_eq!(stdout(&output), "a1\nout\n");
}

#[test]
fn a_count_past_the_loops_leaves_the_outermost() {
    let output =
        run_shell("for i in a b; do for j in 1 2; do echo $i$j; break 5; done; done; echo out\n");

    assert_eq!(stdout(&output), "a1\nout\n");
}

#[test]
fn outside_a_loop_it_does_nothing() {
    let output = run_shell("break; echo after st=$?\n");

    assert_eq!(stdout(&output), "after st=0\n");
}

#[test]
fn an_invalid_count_exits_a_script() {
    for (count, message) in [
        ("0", "0: loop count out of range"),
        ("x", "x: numeric argument required"),
    ] {
        let output = run_shell(&format!(
            "for i in 1 2; do break {count}; done\necho after\n"
        ));

        assert!(stderr(&output).contains(message), "{count}");
        assert_eq!(stdout(&output), "", "{count}");
        assert_eq!(output.status.code(), Some(2), "{count}");
    }
}

#[test]
fn through_and_or_lists_if_and_eval() {
    let output = run_shell(
        "for i in 1 2; do true && break; echo no; done; echo and\n\
         for i in 1 2; do false || break; echo no; done; echo or\n\
         for i in 1 2; do if true; then break; fi; echo no; done; echo if\n\
         for i in 1 2; do if break; then echo no; fi; echo no; done; echo cond\n\
         for i in 1 2; do eval break; echo no; done; echo eval\n",
    );

    assert_eq!(stdout(&output), "and\nor\nif\ncond\neval\n");
}

#[test]
fn in_a_child_only_the_child_stops() {
    // Like dash: the rest of the subshell doesn't run, the loop goes on.
    let output = run_shell(
        "for i in 1 2; do (break; echo no); echo $i; done\n\
         for i in 1 2; do break | cat; echo $i; done\n\
         for i in 1 2; do x=$(break; echo no); echo \"$i[$x]\"; done\n",
    );

    assert_eq!(stdout(&output), "1\n2\n1\n2\n1[]\n2[]\n");
}

#[test]
fn with_a_redirection() {
    let output = run_shell("for i in 1 2; do break 2>/dev/null; echo no; done; echo out\n");

    assert_eq!(stdout(&output), "out\n");
}

#[test]
fn the_depth_goes_down_after_a_loop() {
    // After a loop that ended normally, then after one left by `break`, a
    // `break` is outside any loop again.
    let output = run_shell(
        "for i in 1; do :; done; break; echo a\nfor i in 1; do break; done; break; echo b\n",
    );

    assert_eq!(stdout(&output), "a\nb\n");
}

#[test]
fn leaves_while_and_until() {
    let output = run_shell(
        "while true; do echo once; break; done\n\
         while true; do until false; do echo in; break 2; done; echo no; done; echo out\n",
    );

    assert_eq!(stdout(&output), "once\nin\nout\n");
}

#[test]
fn in_the_condition_leaves_the_loop() {
    // The condition is in the loop (§2.14): were it not, `break` would do
    // nothing and the body would run once.
    let output = run_shell(
        "set -- a; while break; [ $# -gt 0 ]; do shift; echo no; done; echo while $?\n\
         set -- a; until break; [ $# -eq 0 ]; do shift; echo no; done; echo until $?\n",
    );

    assert_eq!(stdout(&output), "while 0\nuntil 0\n");
}
