//! Quoting (POSIX §2.2) and the parts of a word: quotes, backslashes,
//! parameters and command substitutions glued into one word, comments and
//! blanks.

mod common;

use common::{run_shell, stderr, stdout};

#[test]
fn glued_parts_make_one_word() {
    let output = run_shell("echo a'b'\"c\"d $(echo e)$(echo f)\n");

    assert_eq!(stdout(&output), "abcd ef\n");
}

#[test]
fn quotes_keep_the_blanks() {
    let output = run_shell("printf '[%s]' \"a  b\" 'c  d' e\\ \\ f\n");

    assert_eq!(stdout(&output), "[a  b][c  d][e  f]");
}

#[test]
fn single_quotes_keep_everything() {
    let output = run_shell("echo '$x `y` \\ \" #'\n");

    assert_eq!(stdout(&output), "$x `y` \\ \" #\n");
}

#[test]
fn backslash_inside_double_quotes() {
    // Only `$ ` " \` and newline are escaped, the other backslashes stay.
    let output = run_shell("echo \"a\\\"b\\\\c\\$d\\n\"\n");

    assert_eq!(stdout(&output), "a\"b\\c$d\\n\n");
}

#[test]
fn backslash_outside_quotes() {
    let output = run_shell("echo a\\\\b \\$x \\'y\n");

    assert_eq!(stdout(&output), "a\\b $x 'y\n");
}

#[test]
fn substitutions_inside_double_quotes() {
    let output = run_shell("echo \"<$(printf 'x  y')>\" \"`echo z`\"\n");

    assert_eq!(stdout(&output), "<x  y> z\n");
}

#[test]
fn backquotes() {
    let output = run_shell("echo `echo hi` `echo \\`echo nested\\``\n");

    assert_eq!(stdout(&output), "hi nested\n");
}

#[test]
fn other_characters_are_part_of_words() {
    let output = run_shell("echo * ? [x] ~ { } € !x a=b %@\n");

    assert_eq!(stdout(&output), "* ? [x] ~ { } € !x a=b %@\n");
}

#[test]
fn comments_start_only_a_word() {
    let output = run_shell("# a full line\necho a#b # c\necho d;# e\necho 'f'#g\n");

    assert_eq!(stdout(&output), "a#b\nd\nf#g\n");
}

#[test]
fn tabs_are_blanks() {
    let output = run_shell("printf '[%s]'\ta\t\tb\n");

    assert_eq!(stdout(&output), "[a][b]");
}

#[test]
fn bang_alone_negates_glued_is_a_word() {
    let output = run_shell("! false && echo negated\n!false || echo word\n");

    assert_eq!(stdout(&output), "negated\nword\n");
    assert!(stderr(&output).contains("rush: !false: command not found"));
}

#[test]
fn an_unterminated_quote_is_a_syntax_error() {
    let output = run_shell("echo 'a\necho after\n");

    assert!(stderr(&output).contains("rush: syntax error"));
}

#[test]
fn a_double_semicolon_is_a_syntax_error() {
    let output = run_shell("echo a ;; echo b\necho after\n");

    assert!(stderr(&output).contains("rush: syntax error"));
    assert_eq!(stdout(&output), "after\n");
}
