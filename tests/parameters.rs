//! Parameters (§2.5): the special ones (`$?`, `$$`, `$!`, `$#`, `$@`, `$*`,
//! `$-`, `$0`), the positional ones, the fields `$@` and `$*` give, and the
//! variables the shell sets when it starts.

mod common;

use std::{fs, os::unix::fs::symlink, path::Path};

use common::{run, run_shell, shell_command, stderr, stdout, test_dir};

#[test]
fn question_mark_is_the_last_status() {
    let output = run_shell(
        "echo $?\nfalse; echo $?\ntrue; ! true; echo $?\n\
         (exit 3) || echo $?\n\
         if false; then :; else echo $?; fi\n\
         X=$(exit 4); echo $?\n\
         X=$(exit 4) true; echo $?\n\
         sleep 0 & echo $?\n",
    );

    assert_eq!(stdout(&output), "0\n1\n1\n3\n1\n4\n0\n0\n");
}

#[test]
fn a_syntax_error_sets_the_status_to_2() {
    let output = run_shell(")\necho $?\n");

    assert_eq!(stdout(&output), "2\n");
}

#[test]
fn dollar_is_the_pid_of_the_shell_also_in_subshells() {
    let output = run_shell("echo $$\nsh -c 'echo $PPID'\n(echo $$)\necho $(echo $$)\n");
    let out = stdout(&output);
    let lines: Vec<&str> = out.lines().collect();

    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(lines.iter().all(|line| *line == lines[0]), "{lines:?}");
}

#[test]
fn bang_is_the_pid_of_the_last_background_command() {
    let output = run_shell("echo \"[$!]\"\nsleep 0.2 &\necho $!\njobs -p\n");
    let out = stdout(&output);
    let lines: Vec<&str> = out.lines().collect();

    assert_eq!(lines[0], "[]");
    assert_eq!(lines[1], lines[2], "{out}");
}

#[test]
fn positional_parameters() {
    let output = run_shell(
        "echo $# \"[$1]\"\n\
         set -- a b c d e f g h i j k\n\
         echo $# $1 $9 ${10} ${11} $10 \"[${12}]\"\n",
    );

    // `$10` is `$1` then `0`.
    assert_eq!(stdout(&output), "0 []\n11 a i j k a0 []\n");
}

#[test]
fn quoted_at_gives_a_field_per_parameter() {
    let output = run_shell(
        "set -- a 'b c' ''\n\
         printf '[%s]' \"$@\"; echo\n\
         printf '[%s]' \"x$@y\"; echo\n\
         printf '[%s]' \"${@}\"; echo\n\
         set --\n\
         printf '[%s]' \"$@\" end; echo\n\
         printf '[%s]' \"x$@\" end; echo\n",
    );

    assert_eq!(
        stdout(&output),
        "[a][b c][]\n[xa][b c][y]\n[a][b c][]\n[end]\n[x][end]\n"
    );
}

#[test]
fn star_joins_with_the_first_character_of_ifs() {
    let output = run_shell(
        "set -- a 'b c' d\n\
         printf '[%s]' \"$*\"; echo\n\
         IFS=:\nprintf '[%s]' \"$*\"; echo\n\
         IFS=\nprintf '[%s]' \"$*\"; echo\n\
         unset IFS\nprintf '[%s]' \"$*\"; echo\n",
    );

    assert_eq!(
        stdout(&output),
        "[a b c d]\n[a:b c:d]\n[ab cd]\n[a b c d]\n"
    );
}

#[test]
fn unquoted_at_and_star_give_a_field_per_parameter() {
    let output = run_shell(
        "set -- a '' 'b c'\nprintf '[%s]' $@; echo\nprintf '[%s]' $*; echo\n\
         X=$@; printf '[%s]' \"$X\"; echo\n",
    );

    // Empty parameters are dropped; no IFS splitting inside a parameter yet.
    assert_eq!(stdout(&output), "[a][b c]\n[a][b c]\n[a  b c]\n");
}

#[test]
fn an_empty_unquoted_expansion_removes_its_field() {
    let output = run_shell(
        "printf '[%s]' $NOPE x \"$NOPE\" '' \"\"; echo\n\
         $NOPE\necho status $?\n\
         X=1 $NOPE; echo $X\n",
    );

    assert_eq!(stdout(&output), "[x][][][]\nstatus 0\n1\n");
}

#[test]
fn zero_and_hyphen() {
    let output = run_shell("echo \"[$-]\"\n");
    assert_eq!(stdout(&output), "[]\n");

    let output = run_shell("echo $0\n");
    assert_eq!(stdout(&output), format!("{}\n", env!("CARGO_BIN_EXE_rush")));
}

/// Like dash: a parameter expansion that isn't one is found by the parser, so
/// the whole line is a syntax error, also where it would never run.
#[test]
fn a_bad_substitution_is_a_syntax_error() {
    for line in [
        "echo ${}",
        "echo \"${a b}\"",
        "if false; then echo ${}; fi",
        "false && echo ${}",
        "echo before; echo ${}",
    ] {
        let output = run_shell(&format!("{line}\necho $?\n"));

        assert!(stderr(&output).contains("rush: syntax error"), "{line}");
        assert_eq!(stdout(&output), "2\n", "{line}: nothing ran");
    }
}

/// The word inside the braces is parsed: a command substitution in it runs
/// only when the word is used.
#[test]
fn the_default_word_can_hold_any_expansion() {
    let output = run_shell(
        "echo ${TEST:-$(echo hello)}\n\
         TEST=x; echo ${TEST:-$(echo hello)}\n\
         echo ${NOPE:-$(echo })} ${NOPE:-'}'} ${NOPE:-\"a  b\"} ${NOPE:-${ALSO:-nested}}\n\
         echo \"${NOPE:-'q'}\" \"${NOPE:-\"in quotes\"}\"\n",
    );

    assert_eq!(
        stdout(&output),
        "hello\nx\n} } a  b nested\n'q' in quotes\n"
    );
}

/// Unset, null and set, without and with the colon (§2.6.2's table).
#[test]
fn default_assign_and_alternative() {
    let output = run_shell(
        "n=\n\
         echo \"[${u-d}] [${n-d}] [${u:-d}] [${n:-d}]\"\n\
         echo \"[${u+a}] [${n+a}] [${u:+a}] [${n:+a}]\"\n\
         echo \"[${u=v1}] [$u]\"\n\
         echo \"[${n=v2}] [$n]\"\n\
         echo \"[${n:=v3}] [$n]\"\n\
         s=set; echo \"[${s:-d}] [${s:+a}] [${s:=x}]\"\n",
    );

    assert_eq!(
        stdout(&output),
        "[d] [] [d] [d]\n[] [a] [] []\n[v1] [v1]\n[] []\n[v3] [v3]\n[set] [a] [set]\n"
    );
}

#[test]
fn assigning_a_parameter_that_is_not_a_variable_is_an_error() {
    let output = run_shell("echo ${1=x}\necho after\n");

    assert!(stderr(&output).contains("rush: $1: cannot assign in this way"));
    assert_eq!(stdout(&output), "");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn question_mark_exits_a_script_when_unset() {
    let output = run_shell("x=set; echo ${x?boom}\necho ${nope:?boom $x}\necho after\n");
    assert_eq!(stdout(&output), "set\n");
    assert!(stderr(&output).contains("rush: nope: boom set"));
    assert_eq!(output.status.code(), Some(1));

    let output = run_shell("n=; echo \"[${n?}]\"; echo ${n:?}\n");
    assert_eq!(stdout(&output), "[]\n");
    assert!(stderr(&output).contains("rush: n: parameter null or not set"));

    let output = run_shell("echo ${u?}\n");
    assert!(stderr(&output).contains("rush: u: parameter not set"));
}

#[test]
fn question_mark_in_a_subshell_exits_only_the_subshell() {
    let output = run_shell("x=$(echo ${u:?}); echo \"[$x]\" $?\n(echo ${u:?}) || echo failed\n");

    assert_eq!(stdout(&output), "[] 1\nfailed\n");
}

#[test]
fn an_expansion_error_in_a_pipeline_fails_only_its_command() {
    // Each command of a pipeline has a subshell environment of its own: the
    // error ends that one, not the script (dash, bash).
    let output = run_shell(
        "echo ${x?boom} | cat; echo after\n\
         if :; then :; fi >${x?boom} | cat; echo after\n\
         if :; then :; fi >&foo | cat; echo after\n",
    );

    assert!(stderr(&output).contains("rush: x: boom"));
    assert_eq!(stdout(&output), "after\nafter\nafter\n");
}

#[test]
fn an_expansion_error_in_a_redirection_exits_a_script() {
    // Like dash (bash goes on): an expansion error "shall exit" (§2.8.1).
    let output = run_shell("( : ) >${x?boom}\necho after\n");

    assert!(stderr(&output).contains("rush: x: boom"));
    assert_eq!(stdout(&output), "");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn length() {
    let output = run_shell(
        "x=città; e=\necho ${#x} ${#e} ${#nope}\nset -- a b c\necho ${#} ${##} ${#1} ${#@}\n",
    );

    assert_eq!(stdout(&output), "5 0 0\n3 1 1 3\n");
}

#[test]
fn quoted_expansions_keep_their_field() {
    let output = run_shell(
        "printf '[%s]' \"${n:-}\" ${n:-} \"${u+x}\" \"${#n}\"; echo\n\
         printf '[%s]' \"${@:-none}\"; echo\n\
         set -- a 'b c'\nprintf '[%s]' \"${@:-none}\"; echo\n",
    );

    assert_eq!(stdout(&output), "[][][0]\n[none]\n[a][b c]\n");
}

#[test]
fn remove_a_prefix_or_a_suffix() {
    let output = run_shell(
        "f=dir/sub/file.tar.gz\n\
         echo ${f%.*} ${f%%.*} ${f#*/} ${f##*/}\n\
         echo ${f%.nope} ${f#[a-z]} ${f##*[!a-z.]}\n",
    );

    assert_eq!(
        stdout(&output),
        "dir/sub/file.tar dir/sub/file sub/file.tar.gz file.tar.gz\n\
         dir/sub/file.tar.gz ir/sub/file.tar.gz file.tar.gz\n"
    );
}

/// What is quoted in the pattern matches itself; what an unquoted expansion
/// gives is a pattern.
#[test]
fn quoting_in_patterns() {
    let output = run_shell(
        "s='*a*b'\n\
         echo ${s#*} ${s#\"*\"} ${s#'*a'} ${s#\\*} \"${s%\"*b\"}\"\n\
         p='*a'; echo ${s#$p} ${s#\"$p\"}\n",
    );

    assert_eq!(stdout(&output), "*a*b a*b *b a*b *a\n*b *b\n");
}

#[test]
fn an_empty_redirection_target_is_ambiguous() {
    let output = run_shell("echo a >$NOPE\necho status $?\n");

    assert!(stderr(&output).contains("rush: $NOPE: ambiguous redirect"));
    assert_eq!(stdout(&output), "status 1\n");
}

#[test]
fn startup_variables() {
    let output = run_shell("echo $PPID $OPTIND\nprintf '[%s]' \"$IFS\"\n");

    assert_eq!(
        stdout(&output),
        format!("{} 1\n[ \t\n]", std::process::id())
    );
}

#[test]
fn the_environment_ifs_is_ignored() {
    let mut command = shell_command(Path::new(env!("CARGO_TARGET_TMPDIR")));
    command.env("IFS", ":");
    let output = run(command, "printf '[%s]' \"$IFS\"\n");

    assert_eq!(stdout(&output), "[ \t\n]");
}

#[test]
fn pwd_from_the_environment_is_kept_only_when_it_is_right() {
    let dir = test_dir("pwd_from_the_environment");
    fs::create_dir(dir.join("real")).unwrap();
    symlink(dir.join("real"), dir.join("link")).unwrap();
    let real = dir.join("real").canonicalize().unwrap();
    let link = dir.canonicalize().unwrap().join("link");

    // A symbolic link to the working directory: kept, like `cd link` would.
    let mut command = shell_command(&real);
    command.env("PWD", &link);
    let output = run(command, "echo $PWD\n");
    assert_eq!(stdout(&output), format!("{}\n", link.display()));

    // Another directory, or one with `..`: replaced by the real path.
    for pwd in ["/".to_string(), format!("{}/../real", real.display())] {
        let mut command = shell_command(&real);
        command.env("PWD", &pwd);
        let output = run(command, "echo $PWD\n");
        assert_eq!(stdout(&output), format!("{}\n", real.display()), "{pwd}");
    }
}
