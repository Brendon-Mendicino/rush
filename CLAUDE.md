# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`rush` is a POSIX-style Unix shell written in Rust (edition 2024). It is a single binary crate with no library target. Its dependencies are `libc`, `regex` (the tokenizer) and `clap` (argument parsing for builtins). The grammar follows the POSIX Shell Command Language. A local copy of the spec is in `docs/The Open Group Base Specifications Issue 7, 2018 edition.html`, and the parser's doc comments quote its BNF.

## Commands

    cargo build
    cargo run                         # interactive when stdin is a tty
    echo 'echo hi | cat' | cargo run  # non-interactive
    cargo test                        # unit tests + end-to-end tests in tests/
    cargo test --test redirections    # one integration test file
    cargo test --test lists and_or    # filter by test name within a file
    cargo test --test builtins cd::   # one built-in's tests (tests/builtins/)
    cargo test --bin rush             # only the unit tests inside src/
    cargo clippy
    cargo fmt

The shell writes a debug dump of each parsed command to **stderr** (`=== SYNTAX TREE ===` followed by `{program:#?}`). Tests depend on this: they look at stdout, and they match stderr with `contains`, never with exact equality.

## Architecture

The pipeline is `main` → `Executor::run` (REPL loop) → tokenizer → recursive-descent parser → tree-walking executor → `Waiter`.

- **`tokenizer.rs`**: a regex table (`SPECS`, tried in order, first match wins) that produces `Token`s borrowing from the input. The `Tokenizer` trait relies on interior mutability (`Cell`/`RefCell`). The parser backtracks by `clone()`ing a tokenizer, trying a production on the clone, and `merge()`ing it back on success. `BufTokenizer` reads across the line buffer, and `UnbufferedTokenizer` reads a single `&str`.
- **`syntax.rs`**: `SyntaxTree` holds one associated fn per grammar production (`program`, `complete_commands`, `and_or`, `pipeline`, `simple_command`, `io_redirect`, …). Each returns `Option<Node>` and takes `&T: Tokenizer` with the clone/merge pattern above. AST nodes borrow tokens (`'a`). `Word` is either a `Token` or a `Substitution` (`$VAR` / `$(…)`), and `UnsubCommand` is a command whose words haven't been expanded yet.
- **`executor.rs`**: `Executor` owns the `Shell`, the pending input lines, and `bg_jobs`. `update()` reads one line. If the line ends in a continuation (`\`, `&&`, `||`, `|`), it shows the `> ` prompt and waits for the rest. Otherwise it joins the buffered lines, parses them (the whole input must be consumed, or the result is a syntax error with status 2), and walks the tree: `program` → `list` → `op_list` (`&&`/`||` short-circuit) → `wait_pipeline` → `schedule_pipeline` → `command`. `unsub_command` expands words into a `SimpleCommand` and dispatches it either to a builtin (in-process) or to `simple_command` (spawned through `std::process::Command`).
  - Redirections: `RedirectController` opens the files in the parent. Then a `RedirectionApplyer` is passed to the child's `pre_exec` closure as a raw pointer (`ApplyerPtr`), and the parent frees it after `spawn()`. The code in `pre_exec` must stay async-signal-safe and must not allocate.
  - Status: `ExecStatus` (`Exited`/`Signaled`/`Stopped`). `code()` gives `$?` (128+signum for signals). `interrupted()` stops the rest of an and-or list. Spawn errors map to 127 (not found) and 126 (not executable).
- **`job.rs` / `waiter.rs`**: a pipeline becomes a `Job`, which is a process group whose status comes from its last process (`JobMaster`; it is `NotSpawned` when the last command failed to spawn). `Waiter` calls `waitpid` on the group with `WUNTRACED`, gives the terminal back to the shell, and saves or restores the job's termios. It also implements `fg`/`bg`. A stopped foreground job moves into `Executor::bg_jobs`, and finished jobs are reaped before each prompt.
- **`shell.rs`**: the `Shell` struct holds job-control state plus `env: ShellEnv`, which holds *all* shell variables (not only exported ones) as a name → `Var { value, attr: VarAttr { exported } }` map seeded from the process environment, which marks those variables as exported. `upsert(.., None)` keeps the attributes a variable already has. A child's environment is built with `env_clear()` + `shell.env.exported()` + the command's `VAR=x` prefixes. A bare `a=1` (a `SimpleAssignment`) sets a shell variable, and does nothing inside a pipeline.
- **`nix.rs`**: thin, hand-written libc wrappers (`Pid`, `Signal`, `waitpid`, `tcsetpgrp`, `setpgid`, `dup_cloexec`, …) that turn errors into `io::Error` through `cvt`. Add new syscalls here rather than calling `libc` directly from other modules.
- **`builtin/`**: `BUILTINS` maps a name to `FnBuiltin = fn(&mut Builtin) -> BuiltinResult`, where `BuiltinResult = Result<Status, Error>`. `Builtin` carries stdin/stdout/stderr as trait objects, the argv (which includes the name), and `shell: &mut Shell`, so a built-in can change shell state (e.g. `cd` updates `PWD`/`OLDPWD`). The executor builds the `Builtin` and calls `builtin::run`, which handles the result the way Rust handles `main`'s `Result`: an `Err` is reported on the built-in's stderr as `rush: <name>: <message>` and becomes the exit status (1 by default, `Error::new(status, msg)` for another one, 2 for clap usage errors, while `--help` goes to stdout with 0). `run` also flushes stdout. So built-ins don't print their own errors or flush. They use `?` (with `From` impls for `io::Error` → strerror text, `String`/`&str`, `clap::Error`), `.context(what)` on `io::Result` for `what: strerror`, and `bi.report(msg)` for a diagnostic that doesn't stop the built-in. `Status` also has `Exit`/`Return`/`Break`/`Continue` for control flow, which the executor doesn't unwind on yet (`exit` still calls `process::exit`), and `Eval(program)`, which makes the executor parse and run `program` in the current environment (`Executor::execute_str`); its result becomes the built-in's status, and `Exec(argv)`, with which the executor replaces the shell (`Executor::exec`: redirections applied in the shell, fds saved and restored by `SavedFds` if `execvp` fails) or, with an empty `argv`, makes the redirections permanent. A built-in that needs the executor (running shell code, the command's redirections or `VAR=x` prefixes) returns such a variant instead of touching it. Builtins parse their arguments with a `clap::Parser` struct through `bi.parse()?`. To add one, create a module and register it in the `BUILTINS` array.

### Interactive vs non-interactive
`Shell::new` checks `isatty(stdin)`. In interactive mode the shell dups the terminal to an fd ≥ 10 with CLOEXEC, loops until it is in the foreground, ignores `JOB_CONTROL_SIGNALS`, becomes its own process-group leader, and takes the terminal. Each job gets its own process group. Children take the terminal and reset the ignored signals in `pre_exec`, before exec, to avoid a SIGTTIN race. In non-interactive mode there is no job control: children stay in the shell's process group and no prompt is printed.

### Not yet implemented
`&` async lists (prints "not implemented"), subshells and substitution expansion (`todo!()` in `executor.rs`), here-docs (`io_here`), and double-quoted strings. `cmd_runner.rs` is empty.

## Tests

The end-to-end tests in `tests/*.rs` run the real binary (`CARGO_BIN_EXE_rush`). Each file declares `mod common;`. Built-in tests live in `tests/builtins/`, one module per built-in (`cd.rs`, …). They're compiled as the single `builtins` target through `tests/builtins/main.rs`, which pulls in `common` with `#[path = "../common/mod.rs"]`. Modules import it as `crate::common`, and a new built-in's tests need a `mod <name>;` line in `main.rs`.
- Non-interactive tests: `common::run_shell(input)` / `run_shell_in(input, dir)` pipe the input to the shell's stdin and return an `Output`. Use `common::test_dir(name)` for a fresh directory under `CARGO_TARGET_TMPDIR`.
- Interactive tests: `common::pty::InteractiveShell::spawn()` runs the shell on a pseudo-terminal as a session leader, with echo off. `send("…\n")` types input (`"\x03"` = Ctrl-C, `"\x1a"` = Ctrl-Z), and `stdout.expect(..)` / `stderr.expect(..)` poll with a 5 s timeout instead of sleeping. Match the end of the prompt with `pty::PROMPT`; `pty::RED` is printed before it after a failure.
- Job-control tests read `/proc/<pid>/stat` and `/proc/<pid>/status` (pgrp, tpgid, SigIgn), so they only run on Linux.

Unit tests go in `#[cfg(test)] mod tests` at the bottom of the source file.
