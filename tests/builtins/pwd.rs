//! The `pwd` built-in. `$?` cannot be expanded yet: failures are checked with
//! `|| echo failed`.

use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

use crate::common::{run, run_shell_in, shell_command, stderr, stdout, test_dir};

/// The path without symbolic links.
fn real(path: &Path) -> String {
    fs::canonicalize(path).unwrap().display().to_string()
}

/// `dir/real/child` and `dir/link` pointing to it.
fn dir_with_symlink(name: &str) -> PathBuf {
    let dir = test_dir(name);
    fs::create_dir_all(dir.join("real/child")).unwrap();
    symlink(dir.join("real/child"), dir.join("link")).unwrap();
    dir
}

#[test]
fn prints_the_directory_set_by_cd() {
    let dir = test_dir("pwd_after_cd");

    let output = run_shell_in(&format!("cd {}\npwd\n", dir.display()), &dir);

    assert_eq!(stdout(&output), format!("{}\n", dir.display()));
}

#[test]
fn is_logical_by_default() {
    let dir = dir_with_symlink("pwd_logical");
    let link = dir.join("link");

    let output = run_shell_in(
        &format!("cd {}\npwd\npwd -L\npwd -P\n", link.display()),
        &dir,
    );

    assert_eq!(
        stdout(&output),
        format!("{0}\n{0}\n{1}/real/child\n", link.display(), real(&dir))
    );
}

#[test]
fn the_last_of_l_and_p_wins() {
    let dir = dir_with_symlink("pwd_last_option");
    let link = dir.join("link");

    let output = run_shell_in(
        &format!("cd {}\npwd -P -L\npwd -L -P\n", link.display()),
        &dir,
    );

    assert_eq!(
        stdout(&output),
        format!("{}\n{}/real/child\n", link.display(), real(&dir))
    );
}

#[test]
fn options_can_be_grouped_and_repeated() {
    let dir = dir_with_symlink("pwd_grouped_options");
    let link = dir.join("link");

    let output = run_shell_in(
        &format!("cd {}\npwd -LP\npwd -PL\npwd -PP\n", link.display()),
        &dir,
    );

    assert_eq!(
        stdout(&output),
        format!(
            "{0}/real/child\n{1}\n{0}/real/child\n",
            real(&dir),
            link.display()
        )
    );
}

#[test]
fn dash_h_is_an_invalid_option() {
    let dir = test_dir("pwd_dash_h");

    let output = run_shell_in("pwd -h || echo failed\n", &dir);

    assert_eq!(stdout(&output), "failed\n");
    assert!(stderr(&output).contains("unexpected argument '-h'"));
}

#[test]
fn a_stale_pwd_from_the_environment_is_ignored() {
    let dir = test_dir("pwd_stale");

    let mut command = shell_command(&dir);
    command.env("PWD", "/");
    let output = run(command, "pwd\n");

    assert_eq!(stdout(&output), format!("{}\n", real(&dir)));
}

#[test]
fn a_valid_pwd_from_the_environment_is_kept() {
    let dir = dir_with_symlink("pwd_valid");
    let link = dir.join("link");

    let mut command = shell_command(&link);
    command.env("PWD", &link);
    let output = run(command, "pwd\npwd -P\n");

    assert_eq!(
        stdout(&output),
        format!("{}\n{}/real/child\n", link.display(), real(&dir))
    );
}

#[test]
fn a_pwd_with_dot_dot_is_not_used() {
    let dir = dir_with_symlink("pwd_dot_dot");
    let link = dir.join("link");

    // `real/../link` names the directory, but it is not canonical.
    let output = run_shell_in(
        &format!(
            "cd {}\nPWD={}/real/../link\npwd\n",
            link.display(),
            dir.display()
        ),
        &dir,
    );

    assert_eq!(stdout(&output), format!("{}/real/child\n", real(&dir)));
}

#[test]
fn a_removed_directory_is_an_error() {
    let dir = test_dir("pwd_removed");
    let gone = dir.join("gone");
    fs::create_dir(&gone).unwrap();

    let output = run_shell_in(
        &format!(
            "cd {0}\nrmdir {0}\npwd -P || echo failed\npwd || echo failed\n",
            gone.display()
        ),
        &dir,
    );

    assert!(stderr(&output).contains("rush: pwd: No such file or directory"));
    assert_eq!(stdout(&output), "failed\nfailed\n");
}

#[test]
fn an_invalid_option_fails() {
    let dir = test_dir("pwd_invalid_option");

    let output = run_shell_in("pwd -x || echo failed\n", &dir);

    assert_eq!(stdout(&output), "failed\n");
}
