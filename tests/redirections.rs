//! Redirections: files (`<`, `>`, `>>`, `<>`), fd duplication (`n>&m`) and
//! closing (`n>&-`).

mod common;

use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    thread,
    time::{Duration, Instant},
};

use common::{run, run_shell, run_shell_in, shell_command, stderr, stdout, test_dir};

#[test]
fn redirects_output_and_input() {
    let dir = test_dir("redirects_output_and_input");

    let output = run_shell_in(
        "echo first > out.txt\necho second >> out.txt\ncat < out.txt\n",
        &dir,
    );

    assert_eq!(
        fs::read_to_string(dir.join("out.txt")).unwrap(),
        "first\nsecond\n"
    );
    assert_eq!(stdout(&output), "first\nsecond\n");
}

#[test]
fn duplicates_file_descriptors() {
    let output = run_shell("echo to-stderr 1>&2\nls /nonexistent 2>&1 | head -c 2\n");

    assert!(stderr(&output).contains("to-stderr"));
    assert_eq!(stdout(&output), "ls");
}

#[test]
fn reports_a_file_that_cannot_be_opened() {
    let dir = test_dir("reports_a_file_that_cannot_be_opened");

    let output = run_shell_in(
        "cat < missing.txt\necho ran < missing.txt\necho x > nodir/out.txt\necho after\n",
        &dir,
    );

    let stderr = stderr(&output);
    assert!(
        stderr.contains("rush: missing.txt: No such file or directory"),
        "{stderr}"
    );
    assert!(
        stderr.contains("rush: nodir/out.txt: No such file or directory"),
        "{stderr}"
    );
    // The commands are not run.
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn rejects_duplicating_a_closed_fd() {
    let dir = test_dir("rejects_duplicating_a_closed_fd");

    let output = run_shell_in(
        "echo x 1>&9\necho y 3>out.txt 3>&- 1>&3\necho after\n",
        &dir,
    );

    let stderr = stderr(&output);
    assert!(stderr.contains("rush: 9: Bad file descriptor"), "{stderr}");
    assert!(stderr.contains("rush: 3: Bad file descriptor"), "{stderr}");
    assert_eq!(stdout(&output), "after\n");
}

#[test]
fn duplicates_an_fd_opened_by_an_earlier_redirection() {
    let dir = test_dir("duplicates_an_fd_opened_by_an_earlier_redirection");

    run_shell_in("echo x 3>out.txt 1>&3\n", &dir);

    assert_eq!(fs::read_to_string(dir.join("out.txt")).unwrap(), "x\n");
}

#[test]
fn applies_redirections_left_to_right() {
    let dir = test_dir("applies_redirections_left_to_right");

    let output = run_shell_in(
        "ls /nonexistent-rush 2>&1 >first.txt\nls /nonexistent-rush >second.txt 2>&1\n",
        &dir,
    );

    // stderr goes where stdout pointed before it was redirected to the file.
    // (Not the path: the syntax tree dump on stderr contains it.)
    let error = "cannot access";
    assert!(stdout(&output).contains(error));
    assert_eq!(fs::read_to_string(dir.join("first.txt")).unwrap(), "");
    // Both go to the file.
    assert!(
        fs::read_to_string(dir.join("second.txt"))
            .unwrap()
            .contains(error)
    );
    assert!(!stderr(&output).contains(error));
}

#[test]
fn redirected_files_do_not_clobber_each_other() {
    let dir = test_dir("redirected_files_do_not_clobber_each_other");

    // Without care, `a.txt` is opened on fd 3, which `3>b.txt` then overwrites.
    run_shell_in("sh -c 'echo A >&4; echo B >&3' 4>a.txt 3>b.txt\n", &dir);

    assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "A\n");
    assert_eq!(fs::read_to_string(dir.join("b.txt")).unwrap(), "B\n");
}

#[test]
fn only_the_redirected_fds_reach_the_program() {
    let dir = test_dir("only_the_redirected_fds_reach_the_program");

    let output = run_shell_in("ls /proc/self/fd\nls /proc/self/fd 10>out.txt\n", &dir);

    let stdout = stdout(&output);
    let mut lists = stdout.split("0\n1\n").skip(1).map(|list| {
        // `ls` lists the fds sorted as strings: 0, 1, 10, 2, 3, ...
        list.lines().collect::<BTreeSet<_>>()
    });
    let plain = lists.next().unwrap();
    let redirected = lists.next().unwrap();

    // fd 10 is opened on itself (no dup2 needed), and the shell's own copy of
    // the file does not leak.
    let extra: Vec<_> = redirected.difference(&plain).collect();
    assert_eq!(extra, [&"10"], "{stdout}");
}

#[test]
fn lessgreat_opens_stdin_read_write() {
    let dir = test_dir("lessgreat_opens_stdin_read_write");

    let output = run_shell_in("echo content > f.txt\ncat <> f.txt\n", &dir);

    assert_eq!(stdout(&output), "content\n");
}

#[test]
fn creates_files_with_mode_0664() {
    let dir = test_dir("creates_files_with_mode_0664");

    let mut command = shell_command(&dir);
    // The umask clears permission bits from every file a process creates, and is
    // inherited by children. `umask(0)`, run in the shell's process before exec,
    // clears nothing, so the files get exactly the mode the shell asked for.
    unsafe {
        command.pre_exec(|| {
            libc::umask(0);
            Ok(())
        });
    }
    run(command, "echo x > a.txt\necho x >> b.txt\ncat <> c.txt\n");

    for file in ["a.txt", "b.txt", "c.txt"] {
        let mode = fs::metadata(dir.join(file)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o664, "{file}");
    }
}

#[test]
fn shell_closes_redirected_files_after_spawning() {
    let dir = test_dir("shell_closes_redirected_files_after_spawning");
    let out = dir.join("out.txt");

    // Keep stdin open, so that the shell is still alive when we look at its fds.
    let mut child = shell_command(&dir).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"echo x > out.txt\n").unwrap();

    let shell_has_file_open = || {
        fs::read_dir(format!("/proc/{}/fd", child.id()))
            .unwrap()
            .filter_map(|entry| fs::read_link(entry.ok()?.path()).ok())
            .any(|target| target == out)
    };

    let deadline = Instant::now() + Duration::from_secs(5);
    while fs::read_to_string(&out).unwrap_or_default() != "x\n" || shell_has_file_open() {
        assert!(
            Instant::now() < deadline,
            "the shell still has out.txt open"
        );
        thread::sleep(Duration::from_millis(10));
    }

    drop(stdin);
    child.wait().unwrap();
}
