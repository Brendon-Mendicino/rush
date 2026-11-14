//! The `unset` built-in. `$X` can't be expanded yet: the variables are checked
//! with `printenv` and with the listings of `export -p` and `readonly -p`.

use crate::common::{run_shell, stderr, stdout};

#[test]
fn unsets_a_variable() {
    let output = run_shell(
        "export X=1 Y=2\nunset X\nprintenv X || echo unset\nunset -v Y\nprintenv Y || echo unset\n",
    );

    assert_eq!(stdout(&output), "unset\nunset\n");
}

#[test]
fn removes_the_attributes_too() {
    let output = run_shell("export X=1\nunset X\nX=2\nprintenv X || echo local\n");

    assert_eq!(stdout(&output), "local\n");
}

#[test]
fn removes_an_exported_variable_without_a_value() {
    let output = run_shell("export X\nunset X\nexport -p\n");

    assert!(!stdout(&output).lines().any(|line| line == "export X"));
}

#[test]
fn nothing_to_unset_is_not_an_error() {
    let output = run_shell("unset NOPE && unset -f foo && unset && echo ok\n");

    assert_eq!(stdout(&output), "ok\n");
}

#[test]
fn f_and_v_together_are_a_usage_error() {
    let output = run_shell("unset -f -v X\necho after\n");

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
}

#[test]
fn an_invalid_name_exits_the_shell() {
    let output = run_shell("unset 1a\necho after\n");

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("rush: unset: 1a: not a valid identifier"));
    assert_eq!(stdout(&output), "");
}

#[test]
fn a_readonly_variable_exits_the_shell() {
    let output = run_shell("readonly R=1\nunset R\necho after\n");

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("rush: unset: R: readonly variable"));
    assert_eq!(stdout(&output), "");
}
