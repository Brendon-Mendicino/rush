//! Compound commands (§2.9.4) other than subshells, which have their own file:
//! `if`. They run in the shell itself, or in a forked child inside a pipeline.

mod common;

use std::fs;

use common::{run_shell, run_shell_in, stderr, stdout, test_dir};

#[test]
fn if_runs_the_then_or_the_else_part() {
    let output = run_shell(
        "if true; then echo yes; else echo no; fi\nif false; then echo yes; else echo no; fi\n",
    );

    assert_eq!(stdout(&output), "yes\nno\n");
}

#[test]
fn elif_takes_the_first_condition_that_succeeds() {
    let output = run_shell(
        "if false; then echo 1; elif false; then echo 2; elif true; then echo 3; elif true; then echo 4; else echo 5; fi\n\
         if false; then echo 1; elif false; then echo 2; else echo 3; fi\n",
    );

    assert_eq!(stdout(&output), "3\n3\n");
}

#[test]
fn every_command_of_the_condition_runs() {
    let output = run_shell("if echo a; false; then echo then; else echo else; fi\n");

    assert_eq!(stdout(&output), "a\nelse\n");
}

#[test]
fn the_status_is_the_one_of_the_part_that_ran() {
    let output = run_shell(
        "if true; then false; fi || echo failed\n\
         if false; then :; else false; fi || echo else-failed\n\
         if false; then false; fi && echo none-ran\n",
    );

    assert_eq!(stdout(&output), "failed\nelse-failed\nnone-ran\n");
}

#[test]
fn if_runs_in_the_shell() {
    let dir = test_dir("if_runs_in_the_shell");
    fs::create_dir(dir.join("sub")).unwrap();

    let output = run_shell_in("if true; then X=1; cd sub; fi\necho $X\npwd -P\n", &dir);

    let sub = dir.join("sub").canonicalize().unwrap();
    assert_eq!(stdout(&output), format!("1\n{}\n", sub.display()));
}

#[test]
fn if_in_a_pipeline_runs_in_a_child() {
    let output = run_shell(
        "if true; then echo a; fi | tr a b\n\
         echo c | if true; then tr c d; fi\n\
         if true; then X=2; fi | cat\n\
         echo \"[$X]\"\n",
    );

    assert_eq!(stdout(&output), "b\nd\n[]\n");
}

#[test]
fn redirections_apply_to_the_whole_if() {
    let dir = test_dir("redirections_apply_to_the_whole_if");

    let output = run_shell_in(
        "if echo cond; then echo body; fi >f\necho after\ncat f\n",
        &dir,
    );

    assert_eq!(stdout(&output), "after\ncond\nbody\n");
}

#[test]
fn a_failed_redirection_fails_the_command() {
    // §2.8.1: a non-interactive shell "may exit"; like dash and bash, it doesn't.
    let output = run_shell("if true; then echo body; fi >/nonexistent/f\necho status $?\n");

    assert!(stderr(&output).contains("rush: /nonexistent/f: No such file or directory"));
    assert_eq!(stdout(&output), "status 1\n");
}

#[test]
fn ifs_nest() {
    let output = run_shell(
        "if if false; then :; else true; fi; then if false; then echo no; else echo nested; fi; fi\n",
    );

    assert_eq!(stdout(&output), "nested\n");
}

#[test]
fn if_inside_other_commands() {
    let output = run_shell(
        "echo $(if true; then echo sub; fi)\n\
         ! if true; then :; fi || echo negated\n\
         (if true; then echo subshell; fi)\n",
    );

    assert_eq!(stdout(&output), "sub\nnegated\nsubshell\n");
}

#[test]
fn if_in_the_background() {
    let output = run_shell("if true; then sleep 0.2; echo bg; fi &\necho fg\nsleep 0.4\n");

    assert_eq!(stdout(&output), "fg\nbg\n");
}

/// The REPL runs each line on its own (TODO.md): the lines come from `printf`.
#[test]
fn if_spans_lines_inside_eval() {
    let output =
        run_shell("eval \"$(printf 'if false\\nthen\\n  echo then\\nelse\\n  echo else\\nfi')\"\n");

    assert_eq!(stdout(&output), "else\n");
}

#[test]
fn exit_inside_if_exits_the_shell() {
    let output = run_shell("if true; then exit 3; fi\necho after\n");

    assert_eq!(stdout(&output), "");
    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn reserved_words_only_where_a_command_starts() {
    let output = run_shell("then echo x\necho if then fi\n'if' x\nX=1 fi\n");

    assert!(stderr(&output).contains("rush: syntax error"));
    assert_eq!(stdout(&output), "if then fi\n");
    assert!(stderr(&output).contains("rush: if: command not found"));
    assert!(stderr(&output).contains("rush: fi: command not found"));
}

#[test]
fn an_if_without_fi_is_a_syntax_error() {
    let output = run_shell("if true; then echo a\necho after\n");

    assert!(stderr(&output).contains("rush: syntax error"));
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn a_for_loop_is_not_implemented_yet() {
    let output = run_shell("for x in a; do echo $x; done\necho status $?\n");

    assert!(stderr(&output).contains("rush: for: not implemented yet"));
    assert_eq!(stdout(&output), "status 1\n");
}

#[test]
fn a_bad_for_loop_variable_is_a_syntax_error() {
    let output = run_shell("for 1x in a; do echo x; done\necho after\n");

    assert!(stderr(&output).contains("rush: syntax error"));
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn while_and_until_are_not_implemented_yet() {
    // Neither body runs, so the test can't loop.
    let output = run_shell(
        "while true; do echo x; done\necho status $?\nuntil false; do echo y; done\necho status $?\n",
    );

    assert!(stderr(&output).contains("rush: while: not implemented yet"));
    assert!(stderr(&output).contains("rush: until: not implemented yet"));
    assert_eq!(stdout(&output), "status 1\nstatus 1\n");
}

#[test]
fn a_while_without_do_is_a_syntax_error() {
    let output = run_shell("while true do echo; done\necho after\n");

    assert!(stderr(&output).contains("rush: syntax error"));
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn a_case_is_not_implemented_yet() {
    let output = run_shell("case x in x) echo y;; esac\necho status $?\n");

    assert!(stderr(&output).contains("rush: case: not implemented yet"));
    assert_eq!(stdout(&output), "status 1\n");
}

#[test]
fn a_bare_esac_pattern_is_a_syntax_error() {
    let output = run_shell("case esac in esac) echo x;; esac\necho after\n");

    assert!(stderr(&output).contains("rush: syntax error"));
    assert_eq!(stdout(&output), "after\n");
}
