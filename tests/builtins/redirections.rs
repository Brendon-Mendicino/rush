//! Redirections of a built-in that runs in the shell: they are applied to the
//! shell while the built-in runs, then undone.

use std::fs;

use crate::common::{run_shell_in, stderr, stdout, test_dir};

fn cwd_of(dir: &std::path::Path) -> String {
    fs::canonicalize(dir).unwrap().to_str().unwrap().to_string()
}

#[test]
fn output_goes_to_the_file_and_then_back() {
    let dir = test_dir("builtin_output_goes_to_the_file_and_then_back");

    let output = run_shell_in("pwd > out.txt\necho after\n", &dir);

    assert_eq!(
        fs::read_to_string(dir.join("out.txt")).unwrap(),
        format!("{}\n", cwd_of(&dir))
    );
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn errors_go_to_the_redirected_stderr_and_then_back() {
    let dir = test_dir("builtin_errors_go_to_the_redirected_stderr");

    let output = run_shell_in(
        "cd /nonexistent-rush-1 2> err.txt\ncd /nonexistent-rush-2\n",
        &dir,
    );

    let err = fs::read_to_string(dir.join("err.txt")).unwrap();
    assert!(err.contains("nonexistent-rush-1"), "{err}");
    assert!(!err.contains("nonexistent-rush-2"), "{err}");
    assert!(stderr(&output).contains("rush: cd: /nonexistent-rush-2"));
}

#[test]
fn eval_runs_with_the_redirections() {
    let dir = test_dir("eval_runs_with_the_redirections");

    let output = run_shell_in("eval echo hi > f.txt\necho after\n", &dir);

    assert_eq!(fs::read_to_string(dir.join("f.txt")).unwrap(), "hi\n");
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn a_failed_redirection_does_not_run_the_builtin() {
    let dir = test_dir("builtin_failed_redirection");

    let output = run_shell_in("pwd > nodir/x || echo failed\n", &dir);

    assert!(stderr(&output).contains("rush: nodir/x: No such file or directory"));
    assert_eq!(stdout(&output), "failed\n");
}

#[test]
fn the_saved_stdout_survives_a_redirection_to_a_high_fd() {
    // The shell saves stdout before redirecting it: the copy must not land on
    // fd 10, which the command redirects too.
    let dir = test_dir("the_saved_stdout_survives_a_redirection_to_a_high_fd");

    let output = run_shell_in("pwd > out.txt 10>&1\necho after\n", &dir);

    assert_eq!(
        fs::read_to_string(dir.join("out.txt")).unwrap(),
        format!("{}\n", cwd_of(&dir))
    );
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn colon_creates_the_file() {
    let dir = test_dir("colon_creates_the_file");

    run_shell_in(": > created.txt\n", &dir);

    assert_eq!(fs::read_to_string(dir.join("created.txt")).unwrap(), "");
}
