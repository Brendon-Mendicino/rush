//! The `eval` built-in. `$?` cannot be expanded yet: statuses are checked with
//! `&&` and `||`.

use std::fs;

use crate::common::{run, run_shell, run_shell_in, shell_command, stderr, stdout, test_dir};

#[test]
fn runs_its_argument() {
    let output = run_shell("eval echo a\neval 'echo b; echo c'\n");

    assert_eq!(stdout(&output), "a\nb\nc\n");
}

#[test]
fn joins_the_arguments_with_spaces() {
    let output = run_shell("eval echo a '&&' echo b\n");

    assert_eq!(stdout(&output), "a\nb\n");
}

#[test]
fn parses_its_arguments_again() {
    // The first parse leaves `'a  b'`, quotes included, which the second one
    // turns into a single argument.
    let output = run_shell("eval echo \\'a\\ \\ b\\'\n");

    assert_eq!(stdout(&output), "a  b\n");
}

#[test]
fn nested() {
    let output = run_shell("eval eval echo a\n");

    assert_eq!(stdout(&output), "a\n");
}

#[test]
fn has_the_status_of_the_command() {
    let output =
        run_shell("eval true && echo a\neval false || echo b\neval 'true && false' || echo c\n");

    assert_eq!(stdout(&output), "a\nb\nc\n");
}

#[test]
fn succeeds_without_arguments() {
    let output = run_shell("false\neval && echo a\nfalse\neval '' '' && echo b\n");

    assert_eq!(stdout(&output), "a\nb\n");
}

#[test]
fn a_missing_command_fails() {
    let output = run_shell("eval nonexistent-rush-cmd || echo failed\n");

    assert!(stderr(&output).contains("rush: nonexistent-rush-cmd: command not found"));
    assert_eq!(stdout(&output), "failed\n");
}

#[test]
fn a_syntax_error_fails() {
    let output = run_shell("eval '&&' || echo failed\necho after\n");

    assert!(stderr(&output).contains("rush: syntax error"));
    assert_eq!(stdout(&output), "failed\nafter\n");
}

#[test]
fn runs_in_the_current_environment() {
    let dir = test_dir("eval_environment");
    fs::create_dir(dir.join("sub")).unwrap();

    let output = run_shell_in("eval cd sub\npwd -P\n", &dir);

    assert_eq!(
        stdout(&output),
        format!("{}/sub\n", fs::canonicalize(&dir).unwrap().display())
    );
}

#[test]
fn assignments_change_the_shell_variables() {
    let mut command = shell_command(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")));
    command.env("RUSH_OUTER", "outer");

    let output = run(command, "eval RUSH_OUTER=changed\nprintenv RUSH_OUTER\n");

    assert_eq!(stdout(&output), "changed\n");
}

#[test]
fn sigint_stops_the_and_or_list_around_it() {
    // The `sh` kills itself: the interruption goes through `eval`.
    let output = run_shell(
        "eval sh -c \\'kill -INT \\$\\$\\' || echo no\n\
         eval sh -c \\'kill -INT \\$\\$\\' && echo no\n\
         echo after\n",
    );

    assert_eq!(stdout(&output), "after\n");
}
