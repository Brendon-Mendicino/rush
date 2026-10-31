//! Subshells: `( list )` runs the list in a forked copy of the shell.

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
fn runs_the_list() {
    let output = run_shell("(echo a; echo b)\n");

    assert_eq!(stdout(&output), "a\nb\n");
}

#[test]
fn changes_stay_in_the_subshell() {
    let output = run_with_outer("(RUSH_OUTER=changed; printenv RUSH_OUTER)\nprintenv RUSH_OUTER\n");

    assert_eq!(stdout(&output), "changed\nouter\n");
}

#[test]
fn status_is_the_one_of_the_last_command() {
    let output = run_shell("(false) || echo a\n(true) && echo b\n(true; false) || echo c\n");

    assert_eq!(stdout(&output), "a\nb\nc\n");
}

#[test]
fn subshells_in_pipelines() {
    let output =
        run_shell("(echo a; echo b) | cat\necho c | (cat; echo d)\necho e | (cat) | cat\n");

    assert_eq!(stdout(&output), "a\nb\nc\nd\ne\n");
}

#[test]
fn nested_subshells() {
    let output = run_shell("((echo deep))\n");

    assert_eq!(stdout(&output), "deep\n");
}

#[test]
fn redirections_apply_to_the_whole_subshell() {
    let dir = test_dir("redirections_apply_to_the_whole_subshell");

    run_shell_in("(echo out; ls /nonexistent-rush) > all.txt 2>&1\n", &dir);

    let all = fs::read_to_string(dir.join("all.txt")).unwrap();
    assert!(all.starts_with("out\n"), "{all}");
    assert!(all.contains("cannot access"), "{all}");
}

#[test]
fn failed_redirection_does_not_run_the_subshell() {
    let output = run_shell("(echo ran) > nodir/f || echo failed\n");

    assert!(stderr(&output).contains("rush: nodir/f: No such file or directory"));
    assert_eq!(stdout(&output), "failed\n");
}

#[test]
fn output_is_written_once() {
    // A buffered write of the parent copied into the child would show twice.
    let output = run_shell("echo before\n(echo inside)\necho after\n");

    assert_eq!(stdout(&output), "before\ninside\nafter\n");
}
