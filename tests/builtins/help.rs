//! The `help` built-in, and `--help` as the first argument of any built-in.

use crate::common::{run_shell, stderr, stdout};

#[test]
fn lists_every_builtin_with_its_summary() {
    let output = run_shell("help\n");
    let out = stdout(&output);

    assert!(output.status.success());
    assert!(
        out.contains("  cd         Change the working directory.\n"),
        "{out}"
    );
    for name in ["exit", "jobs", "set", "help", ":", "."] {
        assert!(out.contains(&format!("\n  {name:<10} ")), "{name}: {out}");
    }
}

#[test]
fn prints_the_help_of_a_builtin() {
    let output = run_shell("help cd && echo ok\n");
    let out = stdout(&output);

    assert!(out.starts_with("Change the working directory.\n"), "{out}");
    assert!(out.contains("Usage: cd [-L|-P] [directory]"), "{out}");
    assert!(out.ends_with("ok\n"), "{out}");
}

#[test]
fn is_the_same_as_dash_dash_help() {
    let help = run_shell("help pwd\n");
    let dash_dash = run_shell("pwd --help\n");

    assert_eq!(stdout(&help), stdout(&dash_dash));
    assert!(!stdout(&help).is_empty());
}

#[test]
fn prints_several_helps() {
    let output = run_shell("help : eval\n");
    let out = stdout(&output);

    assert!(out.contains("Usage: : [argument...]"), "{out}");
    assert!(out.contains("Usage: eval [argument...]"), "{out}");
}

#[test]
fn an_unknown_name_fails_and_goes_on() {
    let output = run_shell("help nope true || echo failed\n");

    assert!(stderr(&output).contains("rush: help: nope: no help topics match"));
    let out = stdout(&output);
    assert!(out.starts_with("Return a zero status.\n"), "{out}");
    assert!(out.ends_with("failed\n"), "{out}");
}

#[test]
fn dash_dash_help_works_for_every_builtin() {
    // `:` and `eval` don't parse their arguments, `set` is a stub, `exit`
    // would leave the shell.
    let output = run_shell(": --help\neval --help\nset --help\nexit --help\necho after\n");
    let out = stdout(&output);

    assert!(output.status.success());
    for usage in ["Usage: : ", "Usage: eval ", "Usage: set ", "Usage: exit "] {
        assert!(out.contains(usage), "{usage}: {out}");
    }
    assert!(out.ends_with("after\n"), "{out}");
}

#[test]
fn dash_dash_help_counts_only_as_the_first_argument() {
    // Not `echo --help` alone: GNU echo has a help of its own.
    let output = run_shell("eval echo x --help\n");

    assert_eq!(stdout(&output), "x --help\n");
}
