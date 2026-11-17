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
- [x] `bg_jobs: Vec<Job>` on `Executor`; before each prompt reap them (re-add a `Waiter::update_status` using `WNOHANG`), print completed ones and drop them (now `jobs: JobTable`, reported in the `jobs` format)
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
- [x] Spawn-type signal setups (`SpawnType`): subshell gets the default signals like a command (bash, not the C++ ignore of TSTP/TTIN/TTOU: Ctrl-Z stops the whole job); async never takes the terminal, and without job control ignores INT/QUIT and reads `/dev/null` (POSIX); a command substitution is not a job: it stays in the shell's group and keeps TSTP/TTIN/TTOU ignored, so Ctrl-Z can't stop it while the shell reads its output (bash, dash)
- [x] Subshell `( list )`: apply redirections, run list in child, `_exit(status)`
- [x] Async list `list &`: child in its own pgid, never takes the terminal; parent prints `<pid>: Background` and stores the job in `bg_jobs`. The child runs without job control, so it waits its commands until they complete (stops are not reported) (the C++ `wait_inside_async`)
- [x] Command substitution `$(...)`: pipe child stdout, run list, read all output, strip **all** trailing `\n` (POSIX; the C++ stripped one), wait the child
- [x] Builtins inside a pipeline run in a forked child (`fork_child`, decided by `PipelineState::in_pipeline`)
- [x] Beyond the C++: forked children run without job control (`is_interactive = false`: their commands stay in the job's group, they never touch the terminal); without job control jobs are waited by pid, not by group (`Waiter::wait_completed`, `update_status`), so a background job sharing the shell's group cannot be reaped by a foreground wait; background jobs are reaped before each command in non-interactive mode too

## 8. Variable substitution
- [x] `$NAME` / `${NAME}` → shell var value, empty if unset (`Executor::parameter`), also inside double quotes
- [x] Substitution in program name, arguments, `VAR=x` values and redirection targets (`Executor::expand_fields`, `expand_text`, `expand_redirects`)
- [x] Special and positional parameters (`$?` `$$` `$!` `$#` `$@` `$*` `$-` `$0`–`$9`, `${10}`; `Parameter`, `Executor::parameter`); `"$@"` and unquoted `$@`/`$*` give a field per parameter, `"$*"` joins with the first character of `IFS`; an unquoted expansion that gives nothing removes its field (`cmd $EMPTY`), a target that isn't one field is an ambiguous redirect
- [x] `${...}` operators (§2.6.2): `${#x}`, `${x-w}` `${x:-w}`, `${x=w}` `${x:=w}`, `${x?w}` `${x:?w}`, `${x+w}` `${x:+w}`, `${x%p}` `${x%%p}` `${x#p}` `${x##p}`; the word inside the braces is parsed like any word (`${x:-$(echo })}`, nested `${...}`); expansion errors (`ExpansionError`) exit a non-interactive shell
- [ ] The pattern operators on `$@`/`$*` apply to the joined value, not to each parameter
- [ ] `\}` inside `"${x:-...}"` keeps its backslash (`DqText` escapes); bash drops it
- [ ] The syntax error of a bad `${...}` says only "syntax error": with the parser error type (see the REPL multi-line item) it should say "bad substitution" like dash
- [ ] Pattern matching (`pattern.rs`): collating symbols `[.x.]`, equivalence classes `[=x=]`, and the pathname expansion rules (leading `.`, `/`)
- [ ] Field splitting of unquoted expansions by `IFS` (inside a parameter, a `$(...)`), pathname expansion (`*`, `?`, `[`), tilde expansion, arithmetic `$(( ))`
- [ ] `set -u`: an unset parameter is an error (the options are only state, see BUILTINS.md 0.9); `push_value` can return an `ExpansionError` for it

## 9. Builtins (`builtin.cpp`)
The detailed plan and the per-built-in checklist are in `BUILTINS.md`; this section only tracks parity with the C++ ones.
- [x] Dispatch: builtins are checked before exec (`builtin::BUILTINS`); run in-process outside pipelines, in a forked child inside one
- [x] `cd [dir|~]`: `$HOME` fallback, `too many arguments`, error message on failure (full POSIX `cd`, see `BUILTINS.md`)
- [x] `exit [n]` (returns `Status::Exit`, exits only a subshell or pipeline child; without `n` the status is `$?`)
- [x] `exec cmd args…`: replace the shell process (`CommandExt::exec`) with the shell env (also `exec` without a command: permanent redirections)
- [x] `jobs`: list the job table (`JobTable`) in the POSIX format
- [x] `fg`: current job or `%` job ID → `Waiter::fg`; put back under the same number if it stops again
- [x] `bg`: current job or `%` job IDs → `Waiter::bg`

## 10. Error handling (application-wide)
- [ ] One `error` module (`src/error.rs`) for every kind of error the shell can meet, and the conversions between them, instead of an error type per module and messages printed where they happen.
  - Today's types to fold in, or convert from:
    - `builtin::Error` (`Usage(clap::Error)` / `Failure { status, message }`) and its `From` for `io::Error`, `String`, `&str`, `clap::Error`, `ReadonlyError`
    - `shell::ReadonlyError`
    - `job::JobIdError`
    - `executor::RedirectError` and `executor::SyntaxError`
    - the spawn failures turned into statuses by `spawn_failure` (127/126)
    - raw `io::Error`s from `nix.rs`
  - Each error carries its kind as POSIX §2.8.1 sorts them, because that decides whether a non-interactive shell exits:
    - shell language syntax error
    - special built-in utility error
    - other utility error
    - redirection error (special built-in, compound command, function, other)
    - variable assignment error
    - expansion error
    - command not found
  - Each error also carries its exit status (1, 2, 126, 127…) and the message, `rush: [<command>: ]<message>`, using the strerror text from `nix::error_message`.
  - `From` impls are the conversions, so `?` works across modules.
  - One place decides the consequence: report, then exit (non-interactive, or a forked child) or go on with the status. It replaces `Executor::builtin_error`, `Executor::assignment_error`, `Executor::compound_redirection_error`, `Executor::expansion_failure`, `redirection_failure` and the ~20 `eprintln!("rush: …")` calls in the executor.

## Beyond parity (not done in testsh either)
- [ ] The grammar rules that are only `unimplemented!` stubs in `syntax.rs` (in the order of §2.10.2): `for_clause`/`name`/`in`/`wordlist`, `case_clause` and its items/`pattern`, `while_clause`/`until_clause`/`do_group`, `brace_group`, `function_definition`/`function_body`/`fname`, `io_here`/`here_end`, `sequential_sep`. They need the AST nodes and the reserved words (§2.4, recognized by the rules in §2.10.2), then wiring into `compound_command` (`CompoundCommand`) and the executor (`Executor::compound_body`). The reserved words are already recognized where a command starts (`ReservedWord`): `for`, `while`, `case`, `{`... are syntax errors there until their clause exists. The `;;`, `<<`, `<<-` and `>|` tokens exist (`>|` acts as `>` until noclobber, `set -C`).
- [ ] Input that spans lines in the REPL. It reads one more line only after `\`, `&&`, `||` and `|`, so input that ends inside a quote, `(`, `$(`, a backquote or a compound command (`if x; then` / ... / `fi`, the usual layout of a script) is a syntax error at the end of its first line, although the tokenizer and the parser handle it over buffered lines (`eval`, `-c` strings once they exist).
  - The parser returns `Option`, so it can't tell a real syntax error from input that stopped too early. Plan an error type for the syntax tree that says "the end was reached and more characters are needed to understand it" (an open quote, `(`, `$(`, `` ` ``, an `if` without its `fi`...), so that the REPL reads another line (prompt `> `) instead of reporting an error, and reports "unexpected end of file" at the real EOF; it would replace `line_has_continuation`.
  - When the time comes, first look at how other shells handle it: bash (its yacc grammar, the PS2 prompt, "syntax error: unexpected end of file"), dash (`parser.c`, reading more input while a command is incomplete), and how they treat an incomplete `-c`/`eval` string.
- [ ] A `#` right after `$(...)` starts a comment (`$(a)#b`), where POSIX keeps it in the word: the tokenizer can't tell the `)` of `$(...)` from the one of a subshell.
- [ ] Heredocs `<<` / `<<-` (`io_here` is `unimplemented!` in both)
- [x] Special parameters `$?`, `$$`, `$!` (and the rest, see §8)
- [x] The shell's arguments (`invocation.rs`): `rush script args` (read whole: no fd for a redirection to clobber; 127 not found, 126 unreadable), `rush -c string [name [args]]`, `rush -s args`, the option letters (refused unless they change nothing), the usage and status 2 on a bad one; the exit status is the last command's
- [ ] `rush -i` (forced interactive) and checking that stderr is a terminal too (POSIX), which the pty tests would need to follow
- [x] `if` (§2.9.4): `if_clause`/`else_part`, the reserved words (§2.4, rule 7a); runs in the shell (assignments and `cd` stay) with its redirections, forked inside a pipeline
- [x] Quoting (§2.2): `'...'`, `"..."` (with `$`, `$(...)` and backquotes inside), `\`; the parts of a word glued into one (`a'b'$(c)`), `# comments`, tabs as blanks
- [x] `export` builtin / stage 4 envs (mark vars external): `export`, `readonly`, `unset` (see `BUILTINS.md`)
- [x] Parser rule 7b (assignment-word handling): `name=` unquoted at the start of the first part, the rest of the word is the value (`X=$(cmd)`, `X='a b'`)
- [x] `fg %n` / `bg %n` job selection (testsh always uses the last job): every XBD §3.204 form
- [x] More tests: `fg`/`bg` and the job table (`tests/builtins/{jobs,fg,bg}.rs`, `tests/job_control.rs`)
