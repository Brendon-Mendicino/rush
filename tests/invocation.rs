//! The shell's command line: a script with arguments, `-c`, `-s`, the options,
//! and its exit status.

mod common;

use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Output};

use common::{run, shell_command, stderr, stdout, test_dir};

/// The shell started with `args`, `input` on its stdin.
fn rush(args: &[&str], input: &str, cwd: &Path) -> Output {
    let mut command = shell_command(cwd);
    command.args(args);
    run(command, input)
}

fn tmp() -> &'static Path {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
}

#[test]
fn a_script_with_arguments() {
    let dir = test_dir("a_script_with_arguments");
    fs::write(dir.join("script"), "echo \"$0\" $# \"$1\" \"$2\"\nexit 3\n").unwrap();

    // The script is the input: stdin is left to the commands.
    let output = rush(&["script", "a", "b c"], "not read\n", &dir);

    assert_eq!(stdout(&output), "script 2 a b c\n");
    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn a_script_reads_stdin_for_its_commands() {
    let dir = test_dir("a_script_reads_stdin_for_its_commands");
    fs::write(dir.join("script"), "cat\n").unwrap();

    let output = rush(&["script"], "from stdin\n", &dir);

    assert_eq!(stdout(&output), "from stdin\n");
}

#[test]
fn c_runs_a_string() {
    let output = rush(&["-c", "echo \"$0\" \"$@\"", "name", "a", "b"], "", tmp());
    assert_eq!(stdout(&output), "name a b\n");

    // Without a name, `$0` is the shell's.
    let output = rush(&["-c", "echo $0 $#"], "", tmp());
    assert_eq!(
        stdout(&output),
        format!("{} 0\n", env!("CARGO_BIN_EXE_rush"))
    );

    // The whole string is parsed at once: a quote can span lines.
    let output = rush(&["-c", "echo 'a\nb'"], "", tmp());
    assert_eq!(stdout(&output), "a\nb\n");
}

#[test]
fn s_reads_stdin_with_arguments() {
    let output = rush(&["-s", "a", "b"], "echo $# $1 $2\n", tmp());

    assert_eq!(stdout(&output), "2 a b\n");
}

#[test]
fn the_exit_status_is_the_one_of_the_last_command() {
    let output = rush(&["-c", "true; false"], "", tmp());
    assert_eq!(output.status.code(), Some(1));

    let output = rush(&[], "false\ntrue\n", tmp());
    assert_eq!(output.status.code(), Some(0));

    let output = rush(&[], "true\nfalse\n", tmp());
    assert_eq!(output.status.code(), Some(1));

    let output = rush(&["-c", ")"], "", tmp());
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_script_that_cannot_be_read() {
    let output = rush(&["/nonexistent/script"], "", tmp());
    assert!(stderr(&output).contains("rush: /nonexistent/script: No such file or directory"));
    assert_eq!(output.status.code(), Some(127));

    let dir = test_dir("a_script_that_cannot_be_read");
    let script = dir.join("script");
    fs::write(&script, "echo hi\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads it anyway.
    if fs::read(&script).is_err() {
        let output = rush(&["script"], "", &dir);
        assert!(stderr(&output).contains("rush: script: Permission denied"));
        assert_eq!(output.status.code(), Some(126));
    }

    let output = rush(&["."], "", &dir);
    assert_eq!(output.status.code(), Some(126));
}

#[test]
fn bad_invocations_print_the_usage() {
    for (args, message) in [
        (&["-z"][..], "rush: -z: invalid option"),
        (&["-c"][..], "rush: -c: option requires an argument"),
        (&["-o", "nope"][..], "rush: nope: invalid option name"),
    ] {
        let output = rush(args, "echo not run\n", tmp());

        assert!(
            stderr(&output).contains(message),
            "{args:?}: {}",
            stderr(&output)
        );
        assert!(stderr(&output).contains("usage: rush"), "{args:?}");
        assert_eq!(stdout(&output), "");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn options_can_only_stay_as_they_are() {
    let output = rush(&["-e", "-c", "echo not run"], "", tmp());
    assert!(stderr(&output).contains("rush: -e: not implemented yet"));
    assert_eq!(stdout(&output), "");
    assert_eq!(output.status.code(), Some(2));

    let output = rush(&["+e", "-o", "vi", "-c", "echo not run"], "", tmp());
    assert!(stderr(&output).contains("rush: -o vi: not implemented yet"));

    let output = rush(&["+eu", "+o", "vi", "-c", "echo run"], "", tmp());
    assert_eq!(stdout(&output), "run\n");
}
