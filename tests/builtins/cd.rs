//! The `cd` built-in. `$?` cannot be expanded yet: failures are checked with
//! `|| echo failed`. `pwd -P` is the external utility, it shows the real working
//! directory of the shell, `printenv` the exported `PWD` and `OLDPWD`.

use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

use crate::common::{run, run_shell_in, shell_command, stderr, stdout, test_dir};

/// The path without symbolic links, what `pwd -P` prints.
fn real(path: &Path) -> String {
    fs::canonicalize(path).unwrap().display().to_string()
}

fn mkdir(path: PathBuf) -> PathBuf {
    fs::create_dir_all(&path).unwrap();
    path
}

/// `dir/real/child` and `dir/link` pointing to it.
fn dir_with_symlink(name: &str) -> PathBuf {
    let dir = test_dir(name);
    mkdir(dir.join("real/child"));
    symlink(dir.join("real/child"), dir.join("link")).unwrap();
    dir
}

#[test]
fn changes_to_an_absolute_directory() {
    let dir = test_dir("cd_absolute");
    let target = mkdir(dir.join("target"));

    let output = run_shell_in(&format!("cd {}\npwd -P\n", target.display()), &dir);

    assert_eq!(stdout(&output), format!("{}\n", real(&target)));
}

#[test]
fn changes_to_a_relative_directory() {
    let dir = test_dir("cd_relative");
    let sub = mkdir(dir.join("a/b"));

    let output = run_shell_in("cd a\ncd b\npwd -P\ncd ../..\npwd -P\n", &dir);

    assert_eq!(stdout(&output), format!("{}\n{}\n", real(&sub), real(&dir)));
}

#[test]
fn the_directory_is_kept_by_the_next_commands() {
    let dir = test_dir("cd_kept");
    mkdir(dir.join("sub"));
    fs::write(dir.join("sub/file"), "content\n").unwrap();

    let output = run_shell_in("cd sub\ncat file\n", &dir);

    assert_eq!(stdout(&output), "content\n");
}

#[test]
fn without_operand_goes_home() {
    let dir = test_dir("cd_home");
    let home = mkdir(dir.join("home"));

    let mut command = shell_command(&dir);
    command.env("HOME", &home);
    let output = run(command, "cd\npwd -P\n");

    assert_eq!(stdout(&output), format!("{}\n", real(&home)));
}

#[test]
fn without_operand_and_home_fails() {
    let dir = test_dir("cd_no_home");

    let mut command = shell_command(&dir);
    command.env_remove("HOME");
    let output = run(command, "cd || echo failed\npwd -P\n");

    assert!(stderr(&output).contains("rush: cd: HOME not set"));
    assert_eq!(stdout(&output), format!("failed\n{}\n", real(&dir)));
}

#[test]
fn sets_pwd_and_oldpwd() {
    let dir = test_dir("cd_pwd_oldpwd");
    let a = mkdir(dir.join("a"));
    let b = mkdir(dir.join("b"));

    let output = run_shell_in(
        &format!(
            "cd {}\ncd {}\nprintenv PWD OLDPWD\n",
            a.display(),
            b.display()
        ),
        &dir,
    );

    assert_eq!(
        stdout(&output),
        format!("{}\n{}\n", b.display(), a.display())
    );
}

#[test]
fn dash_goes_back_and_prints_the_directory() {
    let dir = test_dir("cd_dash");
    let a = mkdir(dir.join("a"));
    let b = mkdir(dir.join("b"));

    let output = run_shell_in(
        &format!(
            "cd {}\ncd {}\ncd -\npwd -P\ncd -\n",
            a.display(),
            b.display()
        ),
        &dir,
    );

    assert_eq!(
        stdout(&output),
        format!("{}\n{}\n{}\n", a.display(), real(&a), b.display())
    );
}

#[test]
fn dash_without_oldpwd_fails() {
    let dir = test_dir("cd_dash_no_oldpwd");

    let mut command = shell_command(&dir);
    command.env_remove("OLDPWD");
    let output = run(command, "cd - || echo failed\npwd -P\n");

    assert!(stderr(&output).contains("rush: cd: OLDPWD not set"));
    assert_eq!(stdout(&output), format!("failed\n{}\n", real(&dir)));
}

#[test]
fn a_missing_directory_fails_and_stays() {
    let dir = test_dir("cd_missing");

    let output = run_shell_in("cd nope || echo failed\npwd -P\n", &dir);

    assert!(stderr(&output).contains("rush: cd: nope: No such file or directory"));
    assert_eq!(stdout(&output), format!("failed\n{}\n", real(&dir)));
}

#[test]
fn a_file_is_not_a_directory() {
    let dir = test_dir("cd_file");
    fs::write(dir.join("file"), "").unwrap();

    let output = run_shell_in("cd file || echo failed\n", &dir);

    assert!(stderr(&output).contains("rush: cd: file: Not a directory"));
    assert_eq!(stdout(&output), "failed\n");
}

#[test]
fn success_runs_the_rest_of_an_and_list() {
    let dir = test_dir("cd_and_list");
    mkdir(dir.join("sub"));

    let output = run_shell_in("cd sub && echo ok\n", &dir);

    assert_eq!(stdout(&output), "ok\n");
}

#[test]
fn dot_dot_is_logical_by_default() {
    let dir = dir_with_symlink("cd_logical");

    let output = run_shell_in(
        &format!(
            "cd {}\ncd link\nprintenv PWD\ncd ..\nprintenv PWD\npwd -P\n",
            dir.display()
        ),
        &dir,
    );

    assert_eq!(
        stdout(&output),
        format!("{0}/link\n{0}\n{1}\n", dir.display(), real(&dir))
    );
}

#[test]
fn dot_dot_is_physical_with_p() {
    let dir = dir_with_symlink("cd_physical");

    let output = run_shell_in(
        &format!(
            "cd {}\ncd -P link\nprintenv PWD\ncd -P ..\nprintenv PWD\n",
            dir.display()
        ),
        &dir,
    );

    assert_eq!(
        stdout(&output),
        format!("{0}/real/child\n{0}/real\n", real(&dir))
    );
}

#[test]
fn the_last_of_l_and_p_wins() {
    let dir = dir_with_symlink("cd_last_option");
    let start = format!("cd {}\n", dir.display());

    let logical = run_shell_in(&format!("{start}cd -P -L link\nprintenv PWD\n"), &dir);
    let physical = run_shell_in(&format!("{start}cd -L -P link\nprintenv PWD\n"), &dir);

    assert_eq!(stdout(&logical), format!("{}/link\n", dir.display()));
    assert_eq!(stdout(&physical), format!("{}/real/child\n", real(&dir)));
}

#[test]
fn dot_dot_after_a_file_fails() {
    let dir = test_dir("cd_dot_dot_file");
    mkdir(dir.join("sub"));
    fs::write(dir.join("file"), "").unwrap();

    let output = run_shell_in(
        &format!("cd {}\ncd file/../sub || echo failed\n", dir.display()),
        &dir,
    );

    assert!(stderr(&output).contains("rush: cd: file/../sub: Not a directory"));
    assert_eq!(stdout(&output), "failed\n");
}

#[test]
fn cdpath_is_searched_and_the_directory_printed() {
    let dir = test_dir("cd_cdpath");
    let base = mkdir(dir.join("base"));
    let sub = mkdir(base.join("sub"));
    let other = mkdir(dir.join("other"));

    let output = run_shell_in(
        &format!(
            "CDPATH=/rush-nonexistent:{}\ncd sub\nprintenv PWD\n",
            base.display()
        ),
        &other,
    );

    assert_eq!(stdout(&output), format!("{0}\n{0}\n", sub.display()));
}

#[test]
fn an_empty_cdpath_entry_is_the_current_directory() {
    let dir = test_dir("cd_cdpath_empty");
    let base = mkdir(dir.join("base"));
    mkdir(base.join("sub"));
    let sub = mkdir(dir.join("sub"));

    // Found in the current directory: nothing is printed.
    let output = run_shell_in(
        &format!("CDPATH=:{}\ncd sub\npwd -P\n", base.display()),
        &dir,
    );

    assert_eq!(stdout(&output), format!("{}\n", real(&sub)));
}

#[test]
fn cdpath_is_not_used_for_dot_paths() {
    let dir = test_dir("cd_cdpath_dot");
    let base = mkdir(dir.join("base"));
    mkdir(base.join("sub"));

    let output = run_shell_in(
        &format!("CDPATH={}\ncd ./sub || echo failed\n", base.display()),
        &dir,
    );

    assert_eq!(stdout(&output), "failed\n");
}

#[test]
fn a_stale_pwd_from_the_environment_is_ignored() {
    let dir = test_dir("cd_stale_pwd");
    let sub = mkdir(dir.join("sub"));

    let mut command = shell_command(&dir);
    command.env("PWD", "/");
    let output = run(command, "cd sub\nprintenv PWD\n");

    assert_eq!(stdout(&output), format!("{}\n", real(&sub)));
}

#[test]
fn a_valid_pwd_from_the_environment_is_kept() {
    let dir = dir_with_symlink("cd_valid_pwd");
    let link = dir.join("link");

    // Started in the symlink like a parent shell would: `..` goes back to `dir`.
    let mut command = shell_command(&link);
    command.env("PWD", &link);
    let output = run(command, "cd ..\nprintenv PWD\n");

    assert_eq!(stdout(&output), format!("{}\n", dir.display()));
}
