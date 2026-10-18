//! Environment of the commands: exported shell variables, `VAR=x` prefixes, and
//! assignments (`a=1` with no command).

mod common;

use std::{fs, path::Path, process::Output};

use common::{run, run_shell, run_shell_in, shell_command, stderr, stdout, test_dir};

/// Run the shell with `RUSH_OUTER=outer` in its environment.
fn run_with_outer(input: &str) -> Output {
    let mut command = shell_command(Path::new(env!("CARGO_TARGET_TMPDIR")));
    command.env("RUSH_OUTER", "outer");
    run(command, input)
}

#[test]
fn prefix_reaches_the_command() {
    let output = run_shell("RUSH_A=x printenv RUSH_A\n");

    assert_eq!(stdout(&output), "x\n");
}

#[test]
fn last_prefix_wins() {
    let output = run_shell("RUSH_A=1 RUSH_A=2 printenv RUSH_A\n");

    assert_eq!(stdout(&output), "2\n");
}

#[test]
fn exported_vars_are_inherited() {
    let output = run_with_outer("printenv RUSH_OUTER\n");

    assert_eq!(stdout(&output), "outer\n");
}

#[test]
fn prefix_overrides_only_that_command() {
    let output = run_with_outer("RUSH_OUTER=inner printenv RUSH_OUTER\nprintenv RUSH_OUTER\n");

    assert_eq!(stdout(&output), "inner\nouter\n");
}

#[test]
fn assignment_updates_an_exported_var() {
    let output = run_with_outer("RUSH_OUTER=changed\nprintenv RUSH_OUTER\n");

    assert_eq!(stdout(&output), "changed\n");
}

#[test]
fn new_assignment_is_not_exported() {
    // printenv prints nothing for a variable that is not in its environment.
    let output = run_shell("RUSH_NEW=1\nprintenv RUSH_NEW\necho after\n");

    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn assignment_in_a_pipeline_does_not_affect_the_shell() {
    // `cat` reads an empty input: if it inherited the shell's stdin it would
    // swallow the next line.
    let output = run_with_outer("RUSH_OUTER=piped | cat\nprintenv RUSH_OUTER\n");

    assert_eq!(stdout(&output), "outer\n");
}

#[test]
fn prefix_path_is_used_for_the_lookup() {
    let output = run_shell("PATH=/nonexistent ls\necho after\n");

    assert!(stderr(&output).contains("rush: ls: command not found"));
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn values_can_hold_a_path_list() {
    let output = run_shell("PATH=/nonexistent:/usr/bin:/bin printenv PATH\n");

    assert_eq!(stdout(&output), "/nonexistent:/usr/bin:/bin\n");
}

#[test]
fn value_escapes_are_removed() {
    let output = run_shell("RUSH_A=a\\ b printenv RUSH_A\n");

    assert_eq!(stdout(&output), "a b\n");
}

#[test]
fn invalid_name_is_a_command() {
    let output = run_shell("=x\n");

    assert!(stderr(&output).contains("rush: =x: command not found"));
}

#[test]
fn assignment_performs_its_redirections() {
    let dir = test_dir("assignment_performs_its_redirections");

    let output = run_shell_in("RUSH_A=1 > created.txt\nRUSH_A=1 > nodir/x\n", &dir);

    assert_eq!(fs::read_to_string(dir.join("created.txt")).unwrap(), "");
    assert!(stderr(&output).contains("rush: nodir/x: No such file or directory"));
}

#[test]
fn failed_redirection_leaves_the_vars_untouched() {
    let dir = test_dir("failed_redirection_leaves_the_vars_untouched");

    let mut command = shell_command(&dir);
    command.env("RUSH_OUTER", "outer");
    let output = run(
        command,
        "RUSH_OUTER=changed > nodir/x\nprintenv RUSH_OUTER\n",
    );

    assert_eq!(stdout(&output), "outer\n");
}
