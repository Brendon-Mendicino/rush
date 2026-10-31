# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`rush` is a POSIX-style shell in Rust (edition 2024), being ported from an earlier C++ implementation. Several files still carry the original C++ in comments (e.g. the top of `src/waiter.rs`, blocks in `src/executor.rs`) as the reference for what the Rust code must do. Job control follows the GNU libc manual's "Implementing a Shell" design (linked from `src/job.rs`).

The port is mid-way: simple commands, pipelines, `;`/`&&`/`||`/`!` lists, `&`, subshells, command substitution, redirections, `VAR=x` prefixes and assignments run end to end; `$VAR` (`todo!()`) and builtins are still missing. Expect dead-code warnings.

Porting progress is tracked in `TODO.md` (sections mirror the C++ sources). Tick items there as they are completed.

## Commands

```sh
cargo check            # fastest feedback; the main verification step
cargo build
cargo run              # interactive REPL on stdin; parser dumps the syntax tree to stderr
cargo clippy
cargo test                             # unit tests + end-to-end tests
cargo test --test redirections <name>  # one end-to-end test (files in tests/, one per feature)
printf 'echo hi | tr a-z A-Z\n' | cargo run -q 2>/dev/null   # quick manual check
```

Dependencies: `libc` (raw syscalls) and `regex` (tokenizer).

End-to-end tests run the binary and live in `tests/`, one file per feature (`repl`, `commands`, `pipelines`, `lists`, `subshells`, `async_lists`, `substitutions`, `redirections`, `environment`, `job_control`); put new tests in the file of the feature they exercise, or add a file for a new feature. Shared helpers are in `tests/common/` (each test file declares `mod common;`):
- `common/mod.rs`: the shell non-interactive, input on a piped stdin, assertions on stdout/stderr (`run_shell`, `run_shell_in` + `test_dir` for tests that touch files). stderr also carries the syntax tree dump, which contains the command's tokens: assert on the program's own message, not on words from the command line.
- `common/pty.rs`: `InteractiveShell`, the shell on a pty as the leader of its own session. Echo is off, so stdout holds only what commands print; stderr is a separate pipe. Wait for state with `expect`/`expect_prompt`/`wait_foreground_job` (`wait_foreground_process` when the program is not the group leader, e.g. inside a subshell), never with sleeps. `job_control.rs` checks process groups and terminal ownership through `/proc/<pid>/stat` and `tcgetpgrp` on the master.

To poke at interactive mode by hand: `script -qec target/debug/rush /dev/null`.

## Architecture

Pipeline: stdin lines → `Executor` → `BufTokenizer` → `SyntaxTree` (parser) → `Executor` walks the AST and spawns processes → `Job` → `Waiter`.

- **`tokenizer.rs`**: regex-driven tokenizer; the token text is capture group 1 of each spec (the specs are the C++ ones, except that words also accept `_ : , + % @`; the C++ `R"(...)"` delimiters are not part of the regex). `UnbufferedTokenizer` works on one line, `BufTokenizer` spans the multi-line input buffer. Both implement the `Tokenizer` trait with interior mutability (`Cell`/`RefCell`), so parser functions take `&T` and backtrack by cloning the tokenizer and calling `merge` on success.
- **`syntax.rs`**: recursive-descent parser. Each `SyntaxTree::<rule>` fn corresponds to a production of the POSIX shell grammar, with the BNF in its doc comment. They return `Option<Node>` (`None` = rule didn't match). AST nodes borrow from the input (`'a` lifetime on `Token<'a>`).
- **`executor.rs`**: REPL loop (prompt, line continuation via trailing `\`, `&&`, `||`, `|`) and AST evaluation. Programs are spawned with `std::process::Command`; shell code that needs its own process (subshell, `&`, `$(...)`) is run by `fork_child`: `nix::fork`, the child sets itself up (`enter_child`: process group, terminal, signals per `SpawnType`, stdin/stdout, redirections), drops job control (`is_interactive = false`, `pgid = getpgrp()`, empty `bg_jobs`), runs the body under `catch_unwind` and ends in `nix::_exit`. Unlike `Command::spawn`, `fork` returns before the child ran, so the parent repeats `setpgid`/`tcsetpgrp`. Redirections are prepared in the parent by `RedirectController` (opens the files, checks `n>&m` fds, so errors are reported before spawning) into an ordered list of `RedirectOp`s, applied left to right in the child through `pre_exec` using a raw-pointer `RedirectionApplyer` that is freed right after `spawn`. Opened files are moved to fds >= 10 (close-on-exec) so a redirection cannot overwrite a file a later one still needs. Each command of a pipeline returns a `CommandRun`: `Spawned` (pid, the pgid it ended up in, and the read end of its output pipe inside a pipeline) or `Done(JobExit)` for a command that finished without a process (an assignment, or a spawn/redirection failure: 127/126/1, error already printed). The child's environment is built from scratch (`env_clear`): `ShellVars::exported()` plus the command's `VAR=x` prefixes. A bare assignment upserts into `ShellVars` (kept exported if it was), and is a no-op inside a pipeline. `schedule_pipeline()` builds a `Job` via `JobBuilder`, and the **last command of the pipeline becomes the `job_master`**, whose status is the job's exit status (`JobMaster::NotSpawned` when it failed to spawn).
- **`nix.rs`**: a small hand-rolled subset of the `nix` crate on top of `libc`: `Pid` newtype, `Signal` enum (discriminants are the real `libc::SIG*` values, so `sig as c_int` is valid), `waitpid` returning `WaitRes`, and `close`/`dup2`/`kill`/`isatty`/`getpgrp`/`setpgid`/`tc{get,set}attr`/`tc{get,set}pgrp` wrappers, and `signal()` (via `sigaction`, async-signal-safe so usable in `pre_exec`). Syscall results go through `cvt()` (`-1` → `io::Error::last_os_error()`). User-facing error messages use `nix::error_message(&err)` (plain `strerror` text, without the `(os error N)` suffix of `io::Error`'s `Display`). Add new syscall wrappers here rather than calling `libc` directly elsewhere.
- **`shell.rs`**: `Shell` (owned by `Executor`) holds the shell's pgid, terminal fd, `is_interactive`, its saved terminal modes and `ShellVars` (initialised from the environment, `external` = exported). `Shell::new()` does the GNU-style interactive init: wait to be in the foreground, ignore job-control signals, `setpgid` itself (skipped when it already leads its group, e.g. as a session leader under `script`), grab the terminal. The terminal fd is a close-on-exec dup of stdin at fd >= 10, so children can still reach it in `pre_exec` after std replaced their stdin.
- **`job.rs`**: `Job` holds pids only (spawned programs and forked shells alike; a dropped `process::Child` neither waits nor kills), a per-pid `ProcStatus`, and the job's saved terminal modes (`tmodes`). Job-level state (`completed`, `stopped`, `completed_status`) is derived from the per-process statuses.
- **`waiter.rs`**: job control; `Waiter<'s>` borrows the `Shell` (`wait`, `update_status`, `fg`, `bg`). Waiting consumes the `Job` and returns `WaitResult::Completed(JobExit)` or `WaitResult::Stopped { job, sig_stop }` (the stopped job is handed back for the job table). With job control it waits on the job's group with `WUNTRACED`; without (non-interactive, forked children) it waits each pid until it completes (`wait_completed`), because the jobs share the shell's group. In interactive mode, `wait` always hands the terminal back to the shell, even when waiting fails. A job's `tmodes` are saved when it stops and `take()`n when `fg` resumes it.

### Invariants to keep

- Children are reaped only through `nix::waitpid`. Never call `Child::wait()`/`try_wait()` on a job's children: they will already have been reaped and the call fails with `ECHILD`.
- With job control `waitpid` is called on the process group (`WaitPid::Group(pgid)`), so children must be in the job's process group; without job control (and in `update_status`) it is called per pid. Interactive: each pipeline gets its own group (`process_group`, the first command is the leader); foreground children take the terminal and reset the job-control signals to default in `pre_exec` (in that order: SIGTTOU must still be ignored for `tcsetpgrp`). Non-interactive: children stay in the shell's group and `job.pgid` is the shell's pgid.
- The shell must stay single-threaded: forked children (`fork_child`) run ordinary Rust code, which is only sound when no other thread could hold a lock at the fork. A forked child never returns to its caller (`catch_unwind` + `nix::_exit`), and the parent flushes stdout before forking.
- The status of every AST node is an `ExecStatus` (`executor.rs`); `ExecStatus::code()` gives `$?` (`128 + signum` for signals/stops). `failure()` is anything but `Exited(0)` (signals and stops included); an `interrupted()` lhs (SIGINT) stops an and-or list whatever the operator, and `not()` keeps it as is.
