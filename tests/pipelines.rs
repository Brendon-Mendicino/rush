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

#[test]
fn bang_negates_the_status() {
    let output = run_shell("! false && echo a\n! true && echo b\n! true || echo c\n");

    assert_eq!(stdout(&output), "a\nc\n");
}

#[test]
fn bang_negates_the_whole_pipeline() {
    // The status of a pipeline is the one of its last command (`cat`
    // succeeds), whatever happened before it.
    let output = run_shell("! ls /nonexistent-rush 2>&1 | cat > /dev/null || echo negated\n");

    assert_eq!(stdout(&output), "negated\n");
}
