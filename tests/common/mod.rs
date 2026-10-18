//! Helpers shared by the end-to-end tests. Each test file is its own crate and
//! uses only some of them.
#![allow(dead_code)]

pub mod pty;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

/// The shell, non-interactive: every stream is a pipe.
pub fn shell_command(cwd: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rush"));
    command
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

pub fn run_shell_in(input: &str, cwd: &Path) -> Output {
    run(shell_command(cwd), input)
}

pub fn run(mut command: Command, input: &str) -> Output {
    let mut child = command.spawn().expect("failed to start the shell");

    // Dropping stdin closes it: the shell sees EOF and terminates.
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();

    child.wait_with_output().unwrap()
}

pub fn run_shell(input: &str) -> Output {
    run_shell_in(input, Path::new(env!("CARGO_TARGET_TMPDIR")))
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A fresh, empty directory for a test.
pub fn test_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}
