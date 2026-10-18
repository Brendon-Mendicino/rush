//! Pipelines: `a | b | c`.

mod common;

use common::{run_shell, stdout};

#[test]
fn runs_pipelines() {
    let output = run_shell("echo hello | tr a-z A-Z\nseq 3 | sort -r | head -n 1\n");

    assert_eq!(stdout(&output), "HELLO\n3\n");
}

#[test]
fn pipeline_runs_around_a_missing_command() {
    // The command after a missing one gets an empty input: if it inherited the
    // shell's stdin, `cat` would swallow the `echo after` line.
    let output = run_shell(
        "echo lost | nonexistent-rush-cmd | echo x\nnonexistent-rush-cmd | cat\necho after\n",
    );

    assert_eq!(stdout(&output), "x\nafter\n");
}
