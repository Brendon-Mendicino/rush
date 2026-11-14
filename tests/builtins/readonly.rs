//! The `readonly` built-in, and what a readonly variable does to the commands
//! that try to change it: a variable assignment error (POSIX §2.8.1), which
//! exits a non-interactive shell. `$X` can't be expanded yet: the variables are
//! checked with `printenv` and with the listings of `-p`.

use crate::common::{pty::InteractiveShell, run_shell, stderr, stdout};

/// The lines of `out` that start with `prefix`.
fn lines_with(out: &str, prefix: &str) -> Vec<String> {
    out.lines()
        .filter(|line| line.starts_with(prefix))
        .map(str::to_string)
        .collect()
}

#[test]
fn p_lists_the_readonly_variables() {
    let output = run_shell("readonly X=1 U\nL=2\nreadonly -p\n");

    assert_eq!(stdout(&output), "readonly U\nreadonly X='1'\n");
}

#[test]
fn without_operands_it_lists_like_p() {
    let output = run_shell("readonly X=1\nreadonly\n");

    assert_eq!(stdout(&output), "readonly X='1'\n");
}

#[test]
fn a_readonly_variable_can_still_be_exported() {
    let output = run_shell("readonly X=1\nexport X\nexport -p\nprintenv X\n");

    assert_eq!(lines_with(&stdout(&output), "export X="), ["export X='1'"]);
    assert!(stdout(&output).ends_with("1\n"));
}

#[test]
fn readonly_again_without_a_value_is_fine() {
    let output = run_shell("readonly X=1\nreadonly X\necho ok\n");

    assert_eq!(stdout(&output), "ok\n");
}

/// The script `readonly X=1`, then `command`: it exits with 1 on the error, and
/// `echo after` never runs.
fn assert_exits(command: &str) {
    let output = run_shell(&format!("readonly X=1\n{command}\necho after\n"));

    assert_eq!(output.status.code(), Some(1), "{command}");
    // `rush: X: ...` for an assignment, `rush: <builtin>: X: ...` for a built-in.
    assert!(
        stderr(&output).contains("X: readonly variable"),
        "{command}: {}",
        stderr(&output)
    );
    assert_eq!(stdout(&output), "", "{command}");
}

#[test]
fn an_assignment_exits_the_shell() {
    assert_exits("X=2");
}

#[test]
fn a_prefix_of_a_program_exits_the_shell_without_running_it() {
    assert_exits("X=2 printenv X");
}

#[test]
fn a_prefix_of_a_builtin_exits_the_shell() {
    assert_exits("X=2 pwd");
    assert_exits("X=2 :");
}

#[test]
fn readonly_with_a_new_value_exits_the_shell() {
    assert_exits("readonly X=2");
    assert_exits("export X=2");
}

#[test]
fn in_a_pipeline_only_the_command_fails() {
    let output = run_shell("readonly X=1\nX=2 printenv X | cat\necho after\n");

    assert!(stderr(&output).contains("rush: X: readonly variable"));
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn in_a_subshell_only_the_subshell_exits() {
    let output = run_shell("readonly X=1\n(X=2; echo inside) || echo failed\necho after\n");

    assert_eq!(stdout(&output), "failed\nafter\n");
}

#[test]
fn cd_fails_on_a_readonly_pwd_after_changing_directory() {
    let output = run_shell("readonly PWD\ncd /tmp || echo failed\npwd -P\n");

    assert!(stderr(&output).contains("rush: cd: PWD: readonly variable"));
    assert_eq!(stdout(&output), "failed\n/tmp\n");
}

#[test]
fn an_interactive_shell_does_not_exit() {
    let mut sh = InteractiveShell::spawn();

    sh.send("readonly X=1\nX=2 || echo failed\n");
    sh.stderr.expect("rush: X: readonly variable");
    sh.stdout.expect("failed\r\n");

    sh.send("echo still here\n");
    sh.stdout.expect("still here\r\n");
}
