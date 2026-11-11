//! The `:` built-in.

use std::fs;

use crate::common::{run_shell, run_shell_in, stderr, stdout, test_dir};

#[test]
fn succeeds() {
    let output = run_shell(": && echo a\n: || echo b\n");

    assert_eq!(stdout(&output), "a\n");
}

#[test]
fn ignores_its_arguments() {
    let output = run_shell(": a b --help -x -- && echo a\n");

    assert_eq!(stdout(&output), "a\n");
    assert!(!stderr(&output).contains("rush: :"));
}

#[test]
fn is_not_looked_up_in_path() {
    // The prefix stays in the shell (special built-in): nothing after it could
    // be found in this PATH, the status of `:` is the status of the shell.
    let output = run_shell("PATH=/rush-nonexistent :\n");

    assert!(output.status.success());
    assert!(!stderr(&output).contains("command not found"));
}

#[test]
fn creates_a_file_with_a_redirection() {
    let dir = test_dir("colon_create");

    let output = run_shell_in(": >new && echo a\n", &dir);

    assert_eq!(stdout(&output), "a\n");
    assert_eq!(fs::read_to_string(dir.join("new")).unwrap(), "");
}

#[test]
fn truncates_a_file_with_a_redirection() {
    let dir = test_dir("colon_truncate");
    fs::write(dir.join("file"), "content\n").unwrap();

    run_shell_in(": >file\n", &dir);

    assert_eq!(fs::read_to_string(dir.join("file")).unwrap(), "");
}

#[test]
fn a_failed_redirection_exits_a_non_interactive_shell() {
    let dir = test_dir("colon_bad_redirection");

    let output = run_shell_in(": <missing || echo failed\necho after\n", &dir);

    assert!(stderr(&output).contains("missing"));
    assert_eq!(stdout(&output), "");
    assert_eq!(output.status.code(), Some(1));
}
