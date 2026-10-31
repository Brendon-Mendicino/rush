//! Command substitution: `$(list)` is replaced by the output of the list, run in
//! a forked shell.

mod common;

use std::{path::Path, process::Output};

use common::{run, run_shell, shell_command, stderr, stdout};

#[test]
fn replaced_by_the_output() {
    let output = run_shell("echo $(echo hi) there\n");

    assert_eq!(stdout(&output), "hi there\n");
}

#[test]
fn can_be_the_program() {
    let output = run_shell("$(echo echo) prog\n");

    assert_eq!(stdout(&output), "prog\n");
}

#[test]
fn trailing_newlines_are_removed() {
    let output = run_shell("echo $(printf 'a\\n\\n\\n') end\n");

    assert_eq!(stdout(&output), "a end\n");
}

#[test]
fn runs_a_whole_list() {
    let output = run_shell("echo $(echo a; false || echo b | tr b c)\n");

    // The output keeps its inner newline: there is no field splitting yet.
    assert_eq!(stdout(&output), "a\nc\n");
}

#[test]
fn large_output_does_not_block() {
    // More than a pipe buffer (64 KiB), less than the limit of one argument
    // (128 KiB): the shell must read while the child writes.
    let output = run_shell("echo $(seq 15000) | wc -c\n");

    assert_eq!(stdout(&output).trim(), "78894");
}

#[test]
fn changes_stay_in_the_substitution() {
    let mut command = shell_command(Path::new(env!("CARGO_TARGET_TMPDIR")));
    command.env("RUSH_OUTER", "outer");
    let output: Output = run(command, "echo $(RUSH_OUTER=changed)\nprintenv RUSH_OUTER\n");

    assert_eq!(stdout(&output), "\nouter\n");
}

#[test]
fn failing_command_gives_an_empty_word() {
    let output = run_shell("echo $(nonexistent-rush-cmd) after\n");

    assert!(stderr(&output).contains("rush: nonexistent-rush-cmd: command not found"));
    // `echo '' after`
    assert_eq!(stdout(&output), " after\n");
}
