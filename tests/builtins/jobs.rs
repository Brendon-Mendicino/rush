//! The `jobs` built-in, without job control: the jobs are listed, but the shell
//! doesn't announce them.

use crate::common::{run_shell, stderr, stdout};

#[test]
fn no_jobs_prints_nothing() {
    let output = run_shell("jobs\n");

    assert_eq!(stdout(&output), "");
}

#[test]
fn lists_a_running_job() {
    let output = run_shell("sleep 0.3 &\njobs\n");

    assert_eq!(stdout(&output), "[1] + Running sleep 0.3\n");
}

#[test]
fn the_most_recent_job_is_the_current_one() {
    let output = run_shell("sleep 0.3 &\nsleep 0.4 &\njobs\n");

    assert_eq!(
        stdout(&output),
        "[1] - Running sleep 0.3\n[2] + Running sleep 0.4\n"
    );
}

#[test]
fn a_completed_job_is_reported_once() {
    let output = run_shell("true &\nfalse &\nsleep 0.2\njobs\njobs\necho end\n");

    assert_eq!(
        stdout(&output),
        "[1] - Done true\n[2] + Done(1) false\nend\n"
    );
}

#[test]
fn numbers_start_again_once_every_job_is_reported() {
    let output = run_shell("true &\nsleep 0.2\njobs\nsleep 0.3 &\njobs\n");

    assert_eq!(
        stdout(&output),
        "[1] + Done true\n[1] + Running sleep 0.3\n"
    );
}

#[test]
fn selects_jobs_by_id() {
    // `?` is not a word character of the tokenizer yet: quoted.
    let output = run_shell("sleep 0.3 &\nsleep 0.4 &\njobs %1 '%?0.4' %-\n");

    assert_eq!(
        stdout(&output),
        "[1] - Running sleep 0.3\n[2] + Running sleep 0.4\n[1] - Running sleep 0.3\n"
    );
}

#[test]
fn p_prints_the_process_id_that_l_shows_too() {
    let output = run_shell("sleep 0.3 &\njobs -p\njobs -l\n");
    let out = stdout(&output);
    let lines: Vec<&str> = out.lines().collect();

    let pid: i32 = lines[0].parse().expect(&out);
    assert_eq!(lines[1], format!("[1] + {pid} Running sleep 0.3"));
}

#[test]
fn an_unknown_job_is_an_error() {
    let output = run_shell("sleep 0.3 &\njobs %2 || echo failed\n");

    assert!(stderr(&output).contains("rush: jobs: %2: no such job"));
    assert_eq!(stdout(&output), "failed\n");
}

#[test]
fn a_job_id_needs_the_percent() {
    let output = run_shell("sleep 0.3 &\njobs 1 || echo failed\n");

    assert!(stderr(&output).contains("1: no such job"));
    assert_eq!(stdout(&output), "failed\n");
}

#[test]
fn an_ambiguous_job_id_is_an_error() {
    let output = run_shell("sleep 0.3 &\nsleep 0.4 &\njobs %sleep || echo failed\n");

    assert!(stderr(&output).contains("rush: jobs: %sleep: ambiguous job spec"));
    assert_eq!(stdout(&output), "failed\n");
}

#[test]
fn a_forked_child_lists_the_jobs_of_the_shell() {
    let output = run_shell("sleep 0.3 &\njobs | cat\njobs -p\necho $(jobs -p)\n");
    let out = stdout(&output);
    let lines: Vec<&str> = out.lines().collect();

    assert_eq!(lines[0], "[1] + Running sleep 0.3");
    assert_eq!(lines[1], lines[2], "{out}");
}

#[test]
fn the_shell_does_not_announce_background_jobs() {
    let output = run_shell("true &\nsleep 0.2\necho end\n");

    assert!(
        !stderr(&output).lines().any(|line| line.starts_with('[')),
        "{}",
        stderr(&output)
    );
}
