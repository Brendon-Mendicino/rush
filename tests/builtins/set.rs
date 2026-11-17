//! The `set` built-in: the positional parameters and the listings. Changing an
//! option is refused until the options have a behavior.

use crate::common::{run_shell, run_shell_in, stderr, stdout, test_dir};

#[test]
fn operands_replace_the_positional_parameters() {
    let output = run_shell(
        "set a 'b c' ''\nprintf '[%s]' \"$@\"; echo $#\n\
         set -- -x y\nprintf '[%s]' \"$@\"; echo $#\n\
         set --\necho $#\n",
    );

    assert_eq!(stdout(&output), "[a][b c][]3\n[-x][y]2\n0\n");
}

#[test]
fn without_arguments_lists_the_variables_for_reinput() {
    let output = run_shell(
        "X='a b'\nY=\"it's\"\n\
         L=$(set | grep -e '^X=' -e '^Y=')\n\
         echo \"$L\"\n\
         unset X Y\n\
         eval \"$L\"\n\
         printf '[%s]' \"$X\" \"$Y\"\n",
    );

    assert_eq!(stdout(&output), "X='a b'\nY='it'\\''s'\n[a b][it's]");
}

#[test]
fn o_lists_the_options() {
    let output = run_shell("set -o\nset +o\n");
    let out = stdout(&output);

    assert!(out.contains("errexit     off\n"), "{out}");
    assert!(out.contains("set +o errexit\n"), "{out}");
    assert!(out.contains("set +o monitor\n"), "not interactive: {out}");
}

#[test]
fn changing_an_option_is_not_implemented() {
    let output = run_shell("set -- a\nset -e -- b || echo failed\necho $1\nset +e && echo off\n");

    assert!(stderr(&output).contains("rush: set: -e: not implemented yet"));
    // Nothing changed, the positional parameters neither; asking for what is
    // already so works.
    assert_eq!(stdout(&output), "failed\na\noff\n");
}

#[test]
fn an_invalid_option_exits_a_script() {
    let output = run_shell("set -z\necho after\n");

    assert!(stderr(&output).contains("rush: set: -z: invalid option"));
    assert_eq!(stdout(&output), "");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn redirected_and_in_a_pipeline() {
    let dir = test_dir("set_redirected_and_in_a_pipeline");
    let output = run_shell_in(
        "set -o >out\ngrep -c errexit out\nset -- a | cat\necho $#\n",
        &dir,
    );

    // In a pipeline it runs in a child: the shell's parameters don't change.
    assert_eq!(stdout(&output), "1\n0\n");
}
