//! Built-ins inside a pipeline of more than one command: each runs in a forked
//! child, like any command of the pipeline, so it can't change or exit the shell.

use std::fs;

use crate::common::{pty::InteractiveShell, run_shell, run_shell_in, stdout, test_dir};

#[test]
fn exit_in_a_pipeline_does_not_exit_the_shell() {
    let output = run_shell("exit 1 | sleep 0\nsleep 0 | exit 1\necho after\n");

    assert_eq!(stdout(&output), "after\n");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn the_status_is_the_one_of_the_last_command() {
    let output = run_shell(
        "sleep 0 | exit 3 || echo failed\nexit 3 | true && echo ok\ntrue | false || echo last\n",
    );

    assert_eq!(stdout(&output), "failed\nok\nlast\n");
}

#[test]
fn cd_in_a_pipeline_does_not_change_the_shell() {
    let dir = test_dir("cd_in_a_pipeline_does_not_change_the_shell");

    let output = run_shell_in("cd / | cat\npwd\nsleep 0 | cd /\npwd\n", &dir);

    let cwd = fs::canonicalize(&dir).unwrap();
    let cwd = cwd.to_str().unwrap();
    assert_eq!(stdout(&output), format!("{cwd}\n{cwd}\n"));
}

#[test]
fn output_goes_into_the_pipe() {
    let dir = test_dir("builtin_output_goes_into_the_pipe");

    let output = run_shell_in("pwd | tr / _\n", &dir);

    let cwd = fs::canonicalize(&dir).unwrap();
    assert_eq!(
        stdout(&output),
        format!("{}\n", cwd.to_str().unwrap().replace('/', "_"))
    );
}

#[test]
fn redirections_are_applied_in_the_child() {
    let dir = test_dir("builtin_redirections_are_applied_in_the_child");

    let output = run_shell_in("pwd > out.txt | cat\n", &dir);

    let cwd = fs::canonicalize(&dir).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("out.txt")).unwrap(),
        format!("{}\n", cwd.to_str().unwrap())
    );
    // The output went to the file, nothing into the pipe.
    assert_eq!(stdout(&output), "");
}

#[test]
fn interactive_shell_survives_exit_in_a_pipeline() {
    let mut sh = InteractiveShell::spawn();

    sh.send("exit 1 | sleep 0\n");
    sh.expect_prompt();
    sh.send("sleep 0 | exit 1\n");
    sh.expect_prompt();

    sh.send("echo alive\n");
    sh.stdout.expect("alive\r\n");
}
