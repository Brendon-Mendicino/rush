//! Special built-ins against regular ones (POSIX §2.14, §2.9.1, §2.8.1): what
//! happens to their `VAR=x` prefixes, and to the shell when they fail. `$VAR`
//! can't be expanded yet: the variables are checked through exported ones.

use std::path::Path;

use crate::common::{
    pty::InteractiveShell, run, run_shell, run_shell_in, shell_command, stderr, stdout, test_dir,
};

/// The shell without job control, with `HOME=/rush-home`.
fn run_with_home(input: &str, cwd: &Path) -> std::process::Output {
    let mut command = shell_command(cwd);
    command.env("HOME", "/rush-home");
    run(command, input)
}

#[test]
fn the_prefixes_of_a_special_builtin_stay() {
    let output = run_with_home(
        "HOME=/x :\nprintenv HOME\n",
        Path::new(env!("CARGO_TARGET_TMPDIR")),
    );

    assert_eq!(stdout(&output), "/x\n");
}

#[test]
fn the_prefixes_of_a_special_builtin_stay_when_it_fails() {
    let output = run_with_home(
        "HOME=/x exec nonexistent-rush-cmd\n",
        Path::new(env!("CARGO_TARGET_TMPDIR")),
    );

    // `exec` failed and the shell exited: nothing to check but the status. The
    // interactive test below checks the variable.
    assert_eq!(output.status.code(), Some(127));
}

#[test]
fn the_prefixes_of_a_regular_builtin_last_while_it_runs() {
    let dir = test_dir("special_regular_prefix");
    let expected = dir.canonicalize().unwrap();

    // `cd` without operand goes to the HOME of its prefix.
    let output = run_with_home(
        &format!("HOME={} cd\npwd\nprintenv HOME\n", expected.display()),
        Path::new(env!("CARGO_TARGET_TMPDIR")),
    );

    assert_eq!(
        stdout(&output),
        format!("{}\n/rush-home\n", expected.display())
    );
}

#[test]
fn a_regular_prefix_does_not_change_an_unrelated_run() {
    let output = run_with_home(
        "HOME=/x pwd >/dev/null\nprintenv HOME\n",
        Path::new(env!("CARGO_TARGET_TMPDIR")),
    );

    assert_eq!(stdout(&output), "/rush-home\n");
}

#[test]
fn a_special_redirection_error_exits_a_non_interactive_shell() {
    let dir = test_dir("special_redirection_error");

    let output = run_shell_in(": <missing\necho after\n", &dir);

    assert_eq!(stdout(&output), "");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn a_regular_builtin_error_does_not_exit() {
    let output = run_shell("cd /rush-nonexistent\necho after\n");

    assert!(stderr(&output).contains("rush: cd: /rush-nonexistent"));
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn a_regular_redirection_error_does_not_exit() {
    let dir = test_dir("regular_redirection_error");

    let output = run_shell_in("pwd >nodir/x\necho after\n", &dir);

    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn only_the_subshell_exits() {
    let output = run_shell("(eval '&&'; echo inside) || echo failed\necho after\n");

    assert_eq!(stdout(&output), "failed\nafter\n");
}

#[test]
fn only_the_pipeline_command_exits() {
    let dir = test_dir("special_pipeline_error");

    let output = run_shell_in(": <missing | cat\necho after\n", &dir);

    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn a_stub_does_not_exit() {
    let output = run_shell("trap\necho after\n");

    assert!(stderr(&output).contains("rush: trap: not implemented yet"));
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn an_interactive_shell_does_not_exit() {
    let dir = test_dir("special_interactive");
    let missing = dir.join("missing");
    let mut sh = InteractiveShell::spawn();

    sh.send(&format!(": <{} || echo failed\n", missing.display()));
    sh.stdout.expect("failed\r\n");

    // `exec` failed, the shell went on, and its prefix stayed.
    sh.send("HOME=/rush-x exec nonexistent-rush-cmd || printenv HOME\n");
    sh.stdout.expect("/rush-x\r\n");
}
