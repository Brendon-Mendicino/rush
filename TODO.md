# Porting TODO (testsh → rush)

Reference implementation: `~/projects/testsh/src`. Tick items as they land.

## 0. Already ported
- [x] Tokenizer (`tokenizer.cpp`)
- [x] Parser / AST (`syntax.cpp`), except heredocs (see Beyond parity)
- [x] `nix` wrappers: `Pid`, `Signal`, `waitpid`, `close`, `dup2`, `kill`, `tc{get,set}attr`, `tcsetpgrp`
- [x] `Job` per-process status, `Waiter::{wait, fg, bg}` (`executor.cpp` Waiter)

## 1. Shell state (`shell.h/.cpp`)
- [x] `Shell` struct: `pgid`, `tmodes`, `terminal`, `is_interactive`, `vars`; owned by `Executor`
- [x] Interactive init: `isatty`, loop `kill(-pgid, SIGTTIN)` until in foreground, ignore SIGINT/QUIT/TSTP/TTIN/TTOU, `setpgid` self, `tcsetpgrp`, save `tmodes`
- [x] New `nix` wrappers for the above: `isatty`, `getpgrp`, `tcgetpgrp`, `setpgid`, `signal`/`sigaction`
- [x] `Waiter` built from / borrowing `Shell` instead of duplicating its fields
- [x] Shell variables (`ShellVars`): `upsert(name=value, attr)`, `get(name)`, `external` attribute, initialised from `environ`

## 2. REPL wiring (`Executor::{execute, update, loop}`)
- [x] `execute()`: skip empty input (`next_is_eof`), report parse failure instead of `todo!()`, run `program`
- [x] `program()` returns the last list's exit status; feed it to `TerminalState.exit_code` (red prompt)
- [x] Prompt only when interactive
- [x] `bg_jobs: Vec<Job>` on `Executor`; before each prompt reap them (re-add a `Waiter::update_status` using `WNOHANG`), print completed ones and drop them
- [x] A Rust equivalent of `ExecStats` for results that are not a waited job (builtins, assignments, errors: `ERROR` / `SHALLOW`)

## 3. Spawning external commands (`Spawner::spawn_async`, command case)
- [x] Put children in the pipeline's process group (`CommandExt::process_group`, interactive only). No parent-side `setpgid` needed: `spawn()` returns after the child exec'd
- [x] Foreground jobs: `tcsetpgrp` to the job pgid (interactive only). Done in the child's `pre_exec` only, the parent call is redundant for the same reason as `setpgid`; the shell keeps the terminal on its own close-on-exec fd so children reach it after their stdin was replaced
- [x] Child resets job-control signals to `SIG_DFL` (`command_singnal`)
- [x] Non-interactive shell: pgid = `getpgrp()`, no terminal handover
- [x] `CommandState`-like context: `is_foreground`, `inside_pipeline`, `pipeline_pgid` (`PipelineState`)
- [x] Spawn failure (`command not found`) → error status instead of `unwrap()` panic (127 not found, 126 not executable; the pipeline goes on, the next command reads an empty input)

## 4. Redirections (`RedirectController`)
- [x] Open errors → print `rush: <file>: <err>` and return status 1 without running the command (no `unwrap`)
- [x] `n>&m`: check `m` is a valid fd (`fcntl(F_GETFD)`) before spawning, or opened/closed by an earlier redirection of the same command (`3>f 1>&3`, `3>&- 1>&3`)
- [x] Created files use mode 0664 (`S_IRUSR|S_IWUSR|S_IRGRP|S_IWGRP|S_IROTH`)
- [x] Parent closes its copy of opened files after spawn (`drop(redirect_controller)` right after `spawn`)
- [x] Beyond the C++: redirections apply left to right (`2>&1 >f` ≠ `>f 2>&1`); opened files are moved to fds >= 10 so they cannot be clobbered by a later redirection; `n>f` where the file landed on `n` clears close-on-exec; `<>` defaults to fd 0 (was 1, a port bug)

## 5. Environment for exec (`exec_prog.cpp`)
- [x] Child env = external shell vars not overridden + the command's `VAR=x` prefixes (last one wins), via `env_clear()` + `envs()` (`ShellVars::exported`); the program is looked up in the resulting `PATH`
- [x] `SimpleAssignment` (`a=1` with no command): upsert into shell vars, after its redirections (a failed redirection leaves the vars untouched); inside a pipeline it is skipped instead of forked (the change would be thrown away with the child)
- [x] Beyond the C++: word tokens accept `_` and `: , + % @` (`MY_VAR=1`, `PATH=/a:/b`); an assignment needs a valid name (`[A-Za-z_][A-Za-z0-9_]*`), otherwise the word is a command (`=x`)

## 6. Lists and pipelines
- [x] `wait_pipeline`: wait the job with `Waiter::wait`; a `Stopped` job goes to `bg_jobs`; return the job master's status
- [x] Pipeline negation `!` flips the exit status (`ExecStatus::not`; a SIGINT is kept, so the list still stops)
- [x] `AndList` / `OrList`: short-circuit on exit status; do not run the rhs if the lhs was killed by SIGINT (`ExecStatus::interrupted`); a stopped lhs counts as a failure
- [x] `SequentialList` returns the last status

## 7. Shell-forking constructs
- [x] **Decide** how to spawn things that run shell code in a child: `nix::fork` for shell code (`Executor::fork_child`), `Command` for programs. `Job` holds pids only (no `process::Child`). Requires the shell to stay single-threaded
- [x] Spawn-type signal setups (`SpawnType`): subshell gets the default signals like a command (bash, not the C++ ignore of TSTP/TTIN/TTOU: Ctrl-Z stops the whole job); async never takes the terminal, and without job control ignores INT/QUIT and reads `/dev/null` (POSIX)
- [x] Subshell `( list )`: apply redirections, run list in child, `_exit(status)`
- [x] Async list `list &`: child in its own pgid, never takes the terminal; parent prints `<pid>: Background` and stores the job in `bg_jobs`. The child runs without job control, so it waits its commands until they complete (stops are not reported) (the C++ `wait_inside_async`)
- [x] Command substitution `$(...)`: pipe child stdout, run list, read all output, strip **all** trailing `\n` (POSIX; the C++ stripped one), wait the child
- [ ] Builtins inside a pipeline run in a forked child (with `fork_child`, once §9 has builtins)
- [x] Beyond the C++: forked children run without job control (`is_interactive = false`: their commands stay in the job's group, they never touch the terminal); without job control jobs are waited by pid, not by group (`Waiter::wait_completed`, `update_status`), so a background job sharing the shell's group cannot be reaped by a foreground wait; background jobs are reaped before each command in non-interactive mode too

## 8. Variable substitution
- [ ] `$NAME` → shell var value, empty if unset (`Executor::varsub`)
- [ ] Substitution in program name and arguments (`unsub_command`, replace the `todo!()`s)

## 9. Builtins (`builtin.cpp`)
The detailed plan and the per-built-in checklist are in `BUILTINS.md`; this section only tracks parity with the C++ ones.
- [ ] Dispatch: builtins are checked before exec (`builtin::BUILTINS`, done); run in-process outside pipelines, in a forked child inside one (`BUILTINS.md` 0.5, still in-process)
- [x] `cd [dir|~]`: `$HOME` fallback, `too many arguments`, error message on failure (full POSIX `cd`, see `BUILTINS.md`)
- [ ] `exit [n]` (works through `process::exit`; `$?` default and pipelines still missing)
- [x] `exec cmd args…`: replace the shell process (`CommandExt::exec`) with the shell env (also `exec` without a command: permanent redirections)
- [ ] `jobs`: list `bg_jobs` with Stopped/Background state
- [ ] `fg`: last job → `Waiter::fg`; re-store it if it stops again
- [ ] `bg`: last job → `Waiter::bg`

## Beyond parity (not done in testsh either)
- [ ] Heredocs `<<` / `<<-` (`io_here` is `unimplemented!` in both)
- [ ] Special parameters `$?`, `$$`, `$!`
- [ ] Double-quoted strings (tokenizer has it commented out)
- [ ] `export` builtin / stage 4 envs (mark vars external)
- [ ] Parser rule 7b (assignment-word handling, `syntax.rs` TODO)
- [ ] `fg %n` / `bg %n` job selection (testsh always uses the last job)
- [ ] More tests: `fg`/`bg` and the job table once the builtins exist (the pty harness is in `tests/common/pty.rs`)
