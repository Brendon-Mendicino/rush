//! End-to-end sanity tests: run the shell binary non-interactively (stdin is a
//! pipe) and check what the commands print.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

fn run_shell_in(input: &str, cwd: &Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_iron-shell"))
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start the shell");

    // Dropping stdin closes it: the shell sees EOF and terminates.
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();

    child.wait_with_output().unwrap()
}

fn run_shell(input: &str) -> Output {
    run_shell_in(input, Path::new(env!("CARGO_TARGET_TMPDIR")))
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A fresh, empty directory for a test.
fn test_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn exits_cleanly_at_eof() {
    let output = run_shell("");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "");
}

#[test]
fn runs_a_command_with_arguments() {
    let output = run_shell("echo hello world\n");

    assert_eq!(stdout(&output), "hello world\n");
}

#[test]
fn runs_pipelines() {
    let output = run_shell("echo hello | tr a-z A-Z\nseq 3 | sort -r | head -n 1\n");

    assert_eq!(stdout(&output), "HELLO\n3\n");
}

#[test]
fn runs_sequential_commands_in_order() {
    let output = run_shell("echo one; echo two\necho three\n");

    assert_eq!(stdout(&output), "one\ntwo\nthree\n");
}

#[test]
fn skips_empty_lines() {
    let output = run_shell("\n\necho x\n\n");

    assert_eq!(stdout(&output), "x\n");
}

#[test]
fn joins_line_continuations() {
    let output = run_shell("echo a \\\nb\n");

    assert_eq!(stdout(&output), "a b\n");
}

#[test]
fn recovers_from_a_syntax_error() {
    let output = run_shell(")\necho after\n");

    assert!(output.status.success());
    assert!(stderr(&output).contains("syntax error"));
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn redirects_output_and_input() {
    let dir = test_dir("redirects_output_and_input");

    let output = run_shell_in("echo first > out.txt\necho second >> out.txt\ncat < out.txt\n", &dir);

    assert_eq!(fs::read_to_string(dir.join("out.txt")).unwrap(), "first\nsecond\n");
    assert_eq!(stdout(&output), "first\nsecond\n");
}

#[test]
fn duplicates_file_descriptors() {
    let output = run_shell("echo to-stderr 1>&2\nls /nonexistent 2>&1 | head -c 2\n");

    assert!(stderr(&output).contains("to-stderr"));
    assert_eq!(stdout(&output), "ls");
}

#[test]
fn reports_a_missing_command() {
    let output = run_shell("nonexistent-rush-cmd\necho after\n");

    assert!(stderr(&output).contains("rush: nonexistent-rush-cmd: command not found"));
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn pipeline_runs_around_a_missing_command() {
    // The command after a missing one gets an empty input: if it inherited the
    // shell's stdin, `cat` would swallow the `echo after` line.
    let output = run_shell("echo lost | nonexistent-rush-cmd | echo x\nnonexistent-rush-cmd | cat\necho after\n");

    assert_eq!(stdout(&output), "x\nafter\n");
}

#[test]
fn non_interactive_children_stay_in_the_shell_group() {
    let output = run_shell("cat /proc/self/stat\n");

    // pid (comm) state ppid pgrp ...
    let stat = stdout(&output);
    let (_, rest) = stat.split_once(") ").unwrap();
    let pgrp: i32 = rest.split_whitespace().nth(2).unwrap().parse().unwrap();

    // The shell was spawned in the test's group and does no job control.
    assert_eq!(pgrp, unsafe { libc::getpgrp() });
}
