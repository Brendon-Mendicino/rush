# Built-in utilities: plan and TODO

Reference: POSIX.1-2017 (Issue 7, 2018 edition), local copies in `docs/`:

| What | Where |
|---|---|
| Shell Command Language, incl. §2.14 Special Built-Ins, §2.9.1 Command Search, §2.8.1 Consequences of Errors, §2.12 Execution Environment | `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html` |
| §1.6 Regular Built-In Utilities table | `docs/utilities/V3_chap01.html` |
| Regular built-ins (one page each) | `docs/utilities/<name>.html` |
| `chmod` symbolic modes (used by `umask`) | `docs/utilities/chmod.html` |
| §3.204 Job Control Job ID (`%%`, `%+`, `%-`, `%n`, `%str`, `%?str`) | `docs/basedefs/V1_chap03.html` |
| §12.2 Utility Syntax Guidelines (option parsing) | `docs/basedefs/V1_chap12.html` |

## The two kinds of built-ins

**Special built-ins (§2.14):** `break` `:` `continue` `.` `eval` `exec` `exit` `export` `readonly` `return` `set` `shift` `times` `trap` `unset`
- Found **before** functions and PATH. They can't be overridden.
- `VAR=x special-builtin`: the assignment **persists** in the shell after the built-in finishes (`x=y :` sets `x`).
- An error, including a redirection error, **exits a non-interactive shell** (§2.8.1). An interactive shell stays up and the status is non-zero.
- They don't need to be exec-able.

**Regular built-ins (§1.6):** `alias` `bg` `cd` `command` `false` `fc` `fg` `getopts` `hash` `jobs` `kill` `newgrp` `pwd` `read` `true` `type` `ulimit` `umask` `unalias` `wait`
- Found after functions and before the PATH search (§2.9.1 step 1d).
- `VAR=x regular-builtin`: the assignment applies **only while the built-in runs**.
- An error never exits the shell.

Command search order (§2.9.1): if the name has no `/`, check special built-in → function → regular built-in → PATH (127 when not found). A name with a `/` is always exec'd.

`echo`, `printf` and `test`/`[` are ordinary utilities that shells usually build in for speed. They're optional, and PATH already finds them.

---

## What exists today (`src/builtin/`)

The dispatch is `Executor::unsub_command` → `BUILTINS.get(name)` → `Executor::run_builtin`, which runs the built-in **in the shell process**, or in a forked child inside a pipeline of more than one command. A built-in has the signature `fn(&mut Builtin) -> BuiltinResult` and gets stdin/stdout/stderr, argv, `&mut Shell` (all the shell variables) and `&mut JobTable` (the jobs). Arguments are parsed with clap (`Builtin::parse`).

Problems with this setup, found by reading the code:
1. ~~Redirections are ignored.~~ Done (0.4): a built-in's redirections are applied to the shell while it runs, then undone.
2. ~~Pipelines are wrong.~~ Done: in a pipeline of more than one command a built-in runs in a forked child (0.5), so `exit 1 | cat` and `cd /tmp | cat` don't affect the shell and its output goes into the pipe. Outside a pipeline it runs in the shell, with its redirections applied for its duration (0.4).
3. ~~`VAR=x` prefixes are dropped~~ Done (0.6): they stay in the shell for a special built-in, and last while a regular one runs.
4. ~~Built-ins can't change shell state.~~ Done: `Builtin` carries `shell: &mut Shell` and `jobs: &mut JobTable` (0.1).
5. ~~Errors are silent and map to odd codes.~~ Done: built-ins return `Result<Status, Error>` and `builtin::run` reports errors centrally (see 0.3).
6. **clap is not POSIX.** It adds `--help`/`--version`, uses its own error format, treats `-1` as an option, and can't parse `+o`, `-9` or `-SIGNAME`.
7. **`exit` calls `std::process::exit`.** That skips cleanup and EXIT traps, and it can't leave just a subshell.

| Built-in | Status |
|---|---|
| `cd` | Complete, apart from options after the operand (`cd dir -P`, see 0.7). Tests are in `tests/builtins/cd.rs`. |
| `exit` | `exit [n]` returns `Status::Exit`, and the executor ends the current process: the shell, or only the forked child in a subshell or a pipeline. With no argument it should use `$?`, not 0. Tests are in `tests/builtins/exit.rs`. |
| `env` | **Not a POSIX built-in.** It ignores its arguments (`env A=1 cmd` doesn't work), prints unexported variables too, and its order is random (HashMap). → Remove it so `/usr/bin/env` runs. Its listing job belongs to `set` and `export -p`. |
| all the others | Stubs in `src/builtin/<name>.rs` (synopsis + spec link) that print `rush: <name>: not implemented yet` and return 1. They're listed in `tests/builtins/not_implemented.rs`; remove a name from there once it's implemented. |
| `exec` | Complete. In a pipeline it replaces only that command's child. Tests are in `tests/builtins/exec.rs`. |
| `eval` | Complete. Tests are in `tests/builtins/eval.rs`. |
| `:` | Complete. Tests are in `tests/builtins/colon.rs`. |
| special vs regular | The prefix and error rules of 0.6 are tested in `tests/builtins/special.rs`. |
| `true`, `false` | Complete. Tests are in `tests/builtins/{true,false}.rs`. |
| `pwd` | Complete. Tests are in `tests/builtins/pwd.rs`. |
| `help` | Not POSIX, like bash's. `help` lists the built-ins with the first line of their help, `help name...` prints their help, the same as `name --help`. Tests are in `tests/builtins/help.rs`. |
| `jobs`, `fg`, `bg` | Complete on top of the job table (0.1), apart from what needs `wait`/`$!`/`kill` (Phase 2). Tests are in `tests/builtins/{jobs,fg,bg}.rs`, the shell's own job reports in `tests/job_control.rs`. |
| `kill`/`newgrp` | The stub exists but is **not registered** in `BUILTINS`. PATH finds `/usr/bin/*`, which is fine for `kill <pid>`, but `kill %1` and similar can't work. |

---

## Plan

### Phase 0: infrastructure (do this first; every built-in depends on it)

- [x] **0.1 Built-in context with mutable state.** `Builtin` carries `shell: &mut Shell` and `jobs: &mut JobTable` (`job.rs`), the executor's table of background and stopped jobs. stdin/stdout/stderr stay `dyn Read`/`dyn Write` so tests can capture them.
  - `JobTable` is a `BTreeMap` from job number to `JobEntry` (the `Job`, its command text, whether its last change was reported). A new job gets the highest number + 1, so numbering starts again from 1 once the table is empty (bash/dash).
  - The current/previous job (`+`/`-`): a stopped job first, then the most recently started, stopped or resumed one (a stamp on each entry).
  - Job IDs (XBD §3.204) are parsed by the built-ins (`builtin::parse_job_id`, a clap `value_parser`) into `JobId` (`Current`, `Previous`, `Number`, `Prefix`, `Contains`); the table only looks them up (`JobTable::find`).
  - The command text is the AST printed back (`Display` for the syntax nodes in `syntax.rs`), like bash does.
  - A forked child keeps a snapshot of the table, so `jobs | cat` and `$(jobs -p)` list the shell's jobs; it can't wait on them, and without job control `fg`/`bg` refuse.
- [x] **0.2 Control-flow result.** `builtin::Status` is `Exited(i32) | Exit(i32) | Return(i32) | Break(n) | Continue(n) | Eval(String) | Exec(Vec<String>)`; `Eval` and `Exec` are already handled. Done: the type. Still to do: the executor unwinds on it (today `Exit`/`Return` map to their code and `Break`/`Continue` to 0), which lets `exit` stop using `process::exit`. `Executor::run` then ends the loop, runs the EXIT trap, and exits with the code.
- [x] **0.3 Error convention.** Built-ins return `Err(builtin::Error)`, and `builtin::run` writes `rush: <name>: <msg>` on the built-in's stderr with status 1, `Error::new(status, …)` for another status, or 2 for clap usage errors (`--help` → stdout, 0). Use `?`, `.context(what)` on `io::Result`, and `bi.report()` for errors after which the built-in continues. The `128+errno` mapping is gone.
- [x] **0.4 Redirections for in-process built-ins.** `RedirectGuard::apply` saves every fd the redirections touch, applies them to the shell with `dup2`, and puts the fds back when the guard is dropped, after the built-in (`run_builtin` owns the guard). `exec` without a command keeps them (`RedirectGuard::keep`). The saved copies, like the files the shell opens, go above the highest fd the command names (`RedirectController::floor`), never on a number the user wrote; the shell's terminal copy is moved out of the way when a redirection targets it (`exec 10>log`).
- [x] **0.5 Built-ins in pipelines.** When a built-in isn't the only command (`PipelineState::in_pipeline`, set once by `schedule_pipeline` for every command of a pipeline of more than one), `unsub_command` runs it through `fork_child`, which sets up the pipe fds, applies its redirections, puts it in the job's process group and ends it with `_exit(status)`. `Executor::run_builtin` itself doesn't know about pipelines. Built-ins inside `( )`, `&` and `$(...)` run in a forked child too. Tests are in `tests/builtins/pipeline.rs`.
  - A built-in's stdin is an unbuffered `nix::FdReader` on fd 0, not `io::stdin()`: the global buffered stdin may hold script lines the REPL read ahead (in the shell, and copied into a forked child), which `read` must not see.
- [x] **0.6 Classification and lookup order.** `BUILTINS` maps each name to a `BuiltinDef { kind: Kind::Special | Kind::Regular, run }`, and `builtin::find(name)` looks it up. The order of §2.9.1 is special → functions → regular → PATH; there are no functions yet, so one lookup covers both kinds (it splits around the functions once they exist).
  - Special built-ins: the `VAR=x` prefixes are assigned to the shell for good (`ShellEnv::assign`, keeping the attributes of an existing variable; a new one is not exported, which §2.9.1 leaves unspecified). An error exits a non-interactive shell with the built-in's status (`Executor::builtin_error`); so do a redirection error on one, a syntax error in `eval` (2) and an `exec` whose command can't run (127/126). In a forked child (subshell, pipeline command, `$(...)`) only the child exits. An interactive shell goes on.
  - Regular built-ins: the prefixes are set exported while the built-in runs and put back afterwards (`Shell::set_temporarily` returns a `TempVarsGuard`, which restores them when dropped). TODO: what happens when the built-in itself changes one of them (`PWD=/x cd`, `VAR=x read VAR`, a `command unset VAR`), see the doc of `TempVarsGuard`. Their errors never exit the shell.
  - "Error" is an `Err` returned by the built-in (`builtin::run` gives back `Err(status)`); `bi.report` followed by `Ok(Status::FAILURE)` is a failure but not an error. The stubs work that way, so a special built-in that isn't implemented yet doesn't exit a script.
  - Still to do: `command` must strip the special properties (§2.9.1); a syntax error in the script itself should exit a non-interactive shell too (§2.8.1), today it is reported and the shell goes on; variable assignment errors (readonly, 0.8).
- [x] **0.7 POSIX option syntax.** clap is kept: it already follows most of the Utility Syntax Guidelines 3–10 (grouped `-abc`, option-arguments attached or separate, `--`, a bare `-` as an operand). The settings every built-in shares are in one place, `builtin::command`: the built-in's name in the usage and errors, `--help` only (`-h` is an invalid option), and repeated options allowed, the last one wins (`pwd -PP`).
  - Every built-in, stubs included, has a clap `Args` struct, also when it doesn't parse its arguments (`:`, `true`, `false`, `eval`, `exec`): its doc comment is the help text. `BuiltinDef.command` keeps it, for `help` (a new built-in, like bash's) and for `--help`, which `builtin::run` handles as the first argument of any built-in.
  - Still to do, with a workaround outside clap: options after an operand are still options (`cd dir -P`), where POSIX takes them as operands; `+x` options for `set`; `getopts` needs its own parser.
- [x] **0.8 Variables model (`shell.rs`).** A `Var` is a `Value` (`Unset` or `Set(String)`, where `Set("")` is null) plus two independent attributes, `VarAttr { exported, readonly }`: every one of the 8 combinations can happen (the table is in the doc of `Var`). `export X`/`readonly X` before X has a value give an `Unset` variable with an attribute; a variable `Unset` without attributes is never stored. `ShellEnv` is a `BTreeMap`, so `iter()` is sorted for `set`/`export -p`/`readonly -p`.
  - Operations: `assign` (keeps the attributes), `export(name, Option<value>)`, `set_readonly(name, Option<value>)`, `unset`, `check_assignable`. Changing or unsetting a readonly variable returns a `ReadonlyError` (`X: readonly variable`).
  - A readonly variable is a *variable assignment error* (§2.8.1, `Executor::assignment_error`): in an assignment, in the prefix of a special or regular built-in and in the prefix of a program (§2.9.1: the program doesn't run). A non-interactive shell exits with 1, an interactive one fails with 1; for a program in a pipeline, only that command fails. `cd` with a readonly `PWD`/`OLDPWD` fails after changing directory.
  - `shell::quote` formats a value for reinput (`'...'` with `'\''`).
  - Still to do: the `export`, `readonly` and `unset` built-ins (Phase 1), and with `readonly` the end-to-end tests of the assignment errors (only unit tests for now: no script can create a readonly variable yet).
- [ ] **0.9 Shell state that's missing:** positional parameters and `$#`/`$@`/`$*`; a last-status field for `$?` (the executor currently only passes it to the prompt); `$$`, `$!`; option flags (`set -e -u -x -C -f -n -v -a -b -m`, `-o name`); an alias table; a trap table; and a hash table. Also set `PWD` at startup (canonical cwd), which the `sh` spec requires.

### Phase 1: built-ins that need no new grammar
`:` `true` `false` `pwd` `cd` `exit` `export` `readonly` `unset` `set` (listing + option flags) `umask` `ulimit` `times` `type` `command` `hash` `eval` `exec`

### Phase 2: job control (needs `&`, a job table, and job IDs)
`jobs` `fg` `bg` `wait` `kill`

`&` works: `Executor::async_list` forks the list and puts the job in the job table (0.1), which is updated before each prompt (and before each command when non-interactive). The job number, the command text, the current/previous (`+`/`-`) job, the "reported" flag and the `%` job IDs are done, and so are `jobs`, `fg` and `bg`.

The shell reports jobs in the `jobs` format: a foreground job that stops (`[1] + Stopped (SIGTSTP) sleep 30`) right away, a background job that completed or changed state before the next prompt, then it drops the completed ones. `cmd &` prints `[1] <pid>` (POSIX Issue 8, bash, dash). All of it only with job control: a non-interactive shell says nothing and keeps the completed jobs until `jobs` reports them.

Still to do for job control:
- `wait`, `$!` and the list of "known" process IDs; `kill %n` (`kill` is not registered yet)
- `set -m`/`+m` to turn job control on and off (today it follows `Shell::is_interactive`), and `set -b`
- a `{CHILD_MAX}` cap on the completed jobs a non-interactive shell keeps
- `fg` returns `Exited(128 + n)` for a job killed by SIGINT, so an and-or list around `fg` goes on, unlike after a foreground job
- `%?string` must be quoted for now: `?` is not a word character of the tokenizer

### Phase 3: built-ins that need language features
| Built-in | Needs |
|---|---|
| `shift`, `set -- args` | positional parameters |
| `getopts` | positional parameters, `OPTIND`/`OPTARG` |
| `read` | IFS field splitting (§2.6.5). Its stdin is already unbuffered (`nix::FdReader`, 0.5), so it doesn't swallow the script lines after its own, as long as it reads byte by byte up to the newline. |
| `.` (dot) | executing a file as a program in the current env (can return `Status::Eval(contents)`; `return` must stop it) |
| `alias`, `unalias` | alias substitution in the tokenizer (§2.3.1), including "value ending in a blank ⇒ check the next word too" |
| `trap` | signal handling (a self-pipe or flag set by a `sigaction` handler, checked between commands), the EXIT trap, and resetting traps in subshells |
| `break`, `continue` | `for`/`while`/`until` loops (not in the grammar yet) |
| `return` | functions and dot scripts |

### Phase 4: low priority
`fc` (needs a command history list), `newgrp` (leave it to `/usr/bin/newgrp`), and optional speed built-ins (`echo`, `printf`, `test`/`[`).

### Tests
Put end-to-end tests in `tests/builtins/<name>.rs`, registered with `mod <name>;` in `tests/builtins/main.rs` and importing helpers from `crate::common`. Use `common::run_shell` for non-interactive tests and `common::pty::InteractiveShell` for `fg`/`bg`/`jobs`/Ctrl-Z. Every built-in should be tested with a redirection (`pwd >f`) and inside a pipeline (`pwd | cat`, `cd / | cat; pwd`).

---

## TODO per built-in

Legend: `[x]` already works · `[ ]` to do. Spec page in brackets.

### Special built-ins [mindex.html §2.14]

#### `:` (colon)
- [x] `: [arg...]`: do nothing, exit 0; arguments are ignored, with no options and no special `--` (the shell expands them once expansion exists)
- [x] `: >file` creates/truncates the file (the executor opens a built-in's redirection files); a failed redirection → status 1
- [x] `x=y :` keeps `x` (special-built-in assignment rule, 0.6)
- [x] a failed redirection exits a non-interactive shell (0.6)

#### `break [n]` / `continue [n]`
- [ ] needs loops (phase 3)
- [ ] `n` defaults to 1; if `n` > the number of enclosing loops, use the outermost one
- [ ] `n` not an integer ≥ 1 → exit >0 (special-built-in error)
- [ ] "enclosing" means lexical: not across a function body, not from a dot script or trap
- [ ] `continue`: re-run the `while`/`until` condition, or take the next `for` value

#### `.` (dot) `. file`
- [ ] if `file` has no `/`, search PATH (the file doesn't need to be executable); never fall back to the cwd
- [ ] run its commands in the **current** environment
- [ ] file not found or unreadable: a non-interactive shell aborts, an interactive one prints an error, status ≠ 0
- [ ] parse error in the file → status ≠ 0 (interactive)
- [ ] status = the last command run, or 0 if nothing ran
- [ ] `return` inside it stops the dot script

#### `eval [arg...]`
- [x] join the arguments with a single space, then parse and execute in the current environment (returns `Status::Eval(program)`, and the executor runs it through `Executor::execute_str`)
- [x] no arguments, or only empty ones → 0
- [x] its status is the status of the program; an interruption (SIGINT) stays visible to the surrounding and-or list
- [x] parse error → `rush: syntax error`, status 2
- [x] parse error exits a non-interactive shell with status 2 (special-built-in error, 0.6)
- [x] `x=y eval …` keeps `x` (0.6)

#### `exec [command [arg...]]`
- [x] returns `Status::Exec(argv)`; the executor does the work because it has the redirections and the prefixes (`Executor::exec`)
- [x] no command: apply the redirections **permanently** to the shell (`exec 3<f`, `exec 4>f`, `exec 5<&0`, `exec 3<&-`, `exec >log`, `exec 2>err`), status 0
- [x] with a command: replace the shell (`execvp`), without forking; exported variables plus prefixes (`foo=bar exec cmd`) go into its environment; it keeps the shell's pid, process group and terminal
- [x] restore the job-control signals to default before exec'ing
- [x] command not found → 127; not executable → 126; the shell goes on with its fds and signals put back (`RedirectGuard`)
- [x] a leading `--` is skipped
- [x] a failed exec (127/126) or a redirection error (1) exits a non-interactive shell (0.6); `x=y exec …` keeps `x` when the exec fails
- [x] inside a multi-command pipeline it replaces only that command's child (`exec echo a | cat`)
- [ ] `exec <file`: the shell reads its own commands through a buffered stdin, so lines already buffered from the old stdin still run first; `exec cmd` loses them (needs unbuffered reading of the input, like `read`)

#### `exit [n]`
- [x] `exit n` ends the shell with status `n`
- [ ] no `n` → status of the last command (`$?`), not 0
- [ ] unwind through a control-flow result instead of `std::process::exit`, flushing stdout
- [x] in a subshell, exit only that subshell (`(exit 3)`: the subshell is a forked child)
- [x] in a pipeline element, exit only that element (`exit 3 | cat` and `cat | exit 3` don't kill the shell)
- [ ] run the EXIT trap first, except when `exit` is called from inside the EXIT trap
- [ ] `n` outside 0–255 or not numeric: the result is unspecified, so print an error and use status 2 (matching dash/bash)
- [ ] drop clap's `--help`

#### `export name[=word]...` / `export -p`
- [ ] give each name the export attribute; `name=word` also assigns
- [ ] `export X` with X unset: mark it exported with no value (needs `Option` value)
- [ ] invalid name (`syntax::is_valid_name`) → error, exit >0
- [ ] assigning to a readonly variable → error
- [ ] `-p`: `export name='value'` for set variables, `export name` for unset ones, sorted, quoted for reinput
- [ ] no arguments: unspecified, so behave like `-p`
- [ ] support `--`

#### `readonly name[=word]...` / `readonly -p`
- [ ] add `readonly` to `VarAttr`; `name=word` assigns first
- [ ] any later assignment (`x=1`, `x=1 cmd`, `read x`, `getopts`, `export x=`, `for x`) → variable assignment error
- [ ] `unset x` on a readonly variable → error
- [ ] `-p`: `readonly name='value'` / `readonly name`, sorted and quoted
- [ ] no arguments → behave like `-p`

#### `return [n]`
- [ ] needs functions / dot scripts (phase 3)
- [ ] stop the current function or dot script; `$?` = `n`, or the last status if `n` is missing
- [ ] in a trap action, the "last command" is the one before the trap

#### `set`
- [ ] no arguments: print every shell variable as `name='value'`, sorted (locale collation; byte order is fine), quoted for reinput
- [ ] `set -- a b c`: replace the positional parameters, update `$#`; a bare `set --` clears them
- [ ] `set a b c` (first argument isn't an option) → set the positional parameters
- [ ] option flags in `-x`/`+x` form, groupable (`-eu`):
  - [ ] `-a` allexport: assignments get the export attribute
  - [ ] `-b` notify: report finished background jobs right away (`[%d]%c %s%s\n`)
  - [ ] `-C` noclobber: `>` fails on an existing file, `>|` overrides (the tokenizer needs a `>|` token)
  - [ ] `-e` errexit, with these exceptions: `if`/`while`/`until` conditions, `!` pipelines, any command of an and-or list except the last, and a non-last pipeline element
  - [ ] `-f` noglob (once pathname expansion exists)
  - [ ] `-h` hash commands when functions are defined (can be a no-op)
  - [ ] `-m` monitor: job control, on by default when interactive (tie it to `Shell::is_interactive`)
  - [ ] `-n` noexec: parse only (an interactive shell may ignore it)
  - [ ] `-u` nounset: expanding an unset variable (other than `@`/`*`) is an error
  - [ ] `-v` verbose: echo input lines to stderr as they're read
  - [ ] `-x` xtrace: print each expanded command to stderr before running it (`+ cmd args`)
- [ ] `-o name` / `+o name`: allexport errexit ignoreeof monitor noclobber noglob noexec nolog notify nounset verbose vi xtrace (`ignoreeof`: Ctrl-D doesn't exit an interactive shell)
- [ ] `set -o`: print the current settings (any format); `set +o`: print them as `set -o x`/`set +o x` commands
- [ ] invalid option → >0 (special-built-in error)

#### `shift [n]`
- [ ] needs positional parameters
- [ ] `n` defaults to 1; `$1 ← $(1+n)` …; update `$#`; `n = 0` changes nothing
- [ ] `n` > `$#` or invalid → error, status ≠ 0 (a non-interactive shell may exit)

#### `times`
- [ ] call `times(2)` (add it to `nix.rs`) and print `"%dm%fs %dm%fs\n%dm%fs %dm%fs\n"`: shell user/sys, then children user/sys (divide by `sysconf(_SC_CLK_TCK)`)

#### `trap`
- [ ] `trap action cond...`: store the action string and run it like `eval` when the condition fires
- [ ] `trap - cond...` and `trap N cond...` (first operand an unsigned integer) → reset to default
- [ ] `trap '' cond...` → ignore the signal
- [ ] conditions: `EXIT`/`0` and signal names without `SIG` (also accept `SIG`-prefixed and lowercase names); XSI numbers 1 2 3 6 9 14 15
- [ ] `$?` after the trap action = its value before the action
- [ ] EXIT trap runs on `exit` and at EOF, in the environment of the last command
- [ ] no operands: print `trap -- 'action' COND` lines, quoted for reinput
- [ ] entering a subshell resets non-ignored traps to default (except for `$(trap)`)
- [ ] non-interactive: signals that were ignored when the shell started can't be trapped
- [ ] interactive shell: a trap on INT/QUIT/TSTP/TTIN/TTOU must coexist with `JOB_CONTROL_SIGNALS` being ignored
- [ ] invalid condition → status ≠ 0; trapping KILL/STOP is undefined, so reject it
- [ ] handling: a `sigaction` handler sets an atomic flag or writes to a self-pipe, and pending traps run between commands and while waiting

#### `unset [-fv] name...`
- [ ] `-v` (default): remove the variable and its export; a name that isn't set is not an error
- [ ] `-f`: remove a function (once functions exist)
- [ ] readonly variable → error, exit >0
- [ ] support `--`

### Regular built-ins [docs/utilities/*.html]

#### `alias [name[=string]...]`
- [ ] alias table in `Shell`
- [ ] `alias name=value` defines; `alias name` prints `name='value'`; no arguments prints all of them, quoted for reinput
- [ ] a `name` with no alias → status >0, and the other operands still get processed
- [ ] alias substitution in the tokenizer for the command-name word (§2.3.1); a value ending in a blank means the next word is checked too; guard against recursion
- [ ] subshells inherit aliases

#### `bg [job_id...]`
- [x] job control must be enabled (interactive), otherwise `rush: bg: no job control`, 1
- [x] default job: the current one (a stopped job first); `%` job IDs
- [x] send SIGCONT to the process group and mark it running (`Waiter::bg`); it becomes the most recent job
- [x] a job that's already running in the background → no-op, 0; a completed one → `job has terminated`, 1
- [x] print `"[%d] %s\n"` (job number, command)
- [ ] the pid becomes "known" (`$!`, `wait`)
- [ ] job control follows `set -m` (today `Shell::is_interactive`)

#### `cd [-L|-P] [directory]` / `cd -`
- [x] `cd dir`, `cd` → `$HOME`, `cd -` → `$OLDPWD`
- [x] step 9: a curpath of `PATH_MAX` or more is made relative to `PWD`
- [x] a stale `PWD` inherited from the environment is ignored (falls back to `getcwd`)
- [x] `cd x | cat` doesn't change the shell's directory (0.5)
- [x] `cd >file`, `cd x 2>err` (0.4)
- [x] HOME unset or empty and no operand: the behavior is implementation-defined; print an error and return 1 instead of calling `chdir("")`
- [x] CDPATH search when the operand doesn't start with `/`, `.` or `..` (an empty entry means `.`); print the new directory when a non-empty CDPATH entry was used
- [x] `-L` (default): build `curpath` from `$PWD` + the operand, canonicalize it logically (drop `.`, fold `x/..`, and check that `x` is a directory before folding)
- [x] `-P`: physical path, resolving symlinks; set PWD to what `pwd -P` would print
- [x] last of `-L`/`-P` wins
- [x] set `PWD` and `OLDPWD` in the shell vars (they're exported, so children see the new values)
- [x] `cd -` ≡ `cd "$OLDPWD" && pwd` (prints the new directory); OLDPWD unset → error
- [x] on failure: `rush: cd: <dir>: <strerror>`, status 1, and the cwd stays the same
- [ ] use the POSIX option parser instead of clap (no `--help`, POSIX-style errors)

#### `command [-p] [-v|-V] command_name [arg...]`
- [ ] no options: run `command_name` while skipping function lookup; a special built-in loses its special properties (assignments don't persist, errors don't exit the shell)
- [ ] `-p`: search with the default PATH (`confstr(_CS_PATH)`, or `/usr/bin:/bin`)
- [ ] `-v`: print the absolute path for a PATH utility; just the name for a built-in, function, or reserved word; the `alias name='…'` line for an alias; nothing and status >0 if not found
- [ ] `-V`: print a description (`ls is /bin/ls`, `cd is a shell builtin`, `exit is a special shell builtin`, `x is an alias for …`, `if is a reserved word`)
- [ ] status: 126 found but not invocable, 127 not found; otherwise the status of the command

#### `false`
- [x] built in, returns 1; arguments are ignored

#### `fc` (low priority, needs history)
- [ ] a history list of entered commands (HISTSIZE, numbering)
- [ ] `fc -l [-nr] [first [last]]`: list (with `-n` no numbers, `-r` reversed); defaults are the last 16
- [ ] `fc [-r] [-e editor] [first [last]]`: write the commands to a temp file, run the editor (`-e` → `$FCEDIT` → `ed`), then re-execute; a non-zero editor status stops it
- [ ] `fc -s [old=new] [first]`: re-run with a substitution
- [ ] `first`/`last` as a number, a negative offset, or a string prefix
- [ ] the `fc` command itself isn't added to history

#### `fg [job_id]`
- [x] job control must be enabled, otherwise `rush: fg: no job control`, 1
- [x] default: the current job (`%+`); `%` job IDs
- [x] print the job's command `"%s\n"`, then hand over the terminal, restore tmodes, SIGCONT and wait (`Waiter::fg`)
- [x] if it stops again, it goes back to the job table under the same number, and is reported
- [x] status = the job's status (`128 + n` for a signal)
- [ ] remove it from the list of "known" async pids (`$!`, `wait`)
- [ ] an interruption (SIGINT) should stop the and-or list around `fg`, like for a foreground job

#### `getopts optstring name [arg...]`
- [ ] needs positional parameters; `OPTIND` initialized to 1 at shell startup, and OPTIND/OPTARG are not exported
- [ ] parse `$@` (or the `arg`s): grouped options, the option argument attached or as the next word, `--` ends the options
- [ ] found option → `name=c`, status 0; an option that takes an argument sets OPTARG, otherwise OPTARG is unset
- [ ] unknown option → `name=?`; with a leading `:` OPTARG=c and nothing printed, otherwise OPTARG unset and a diagnostic printed
- [ ] missing argument → leading `:`: `name=:`, OPTARG=c; otherwise `name=?`, diagnostic printed, status 0
- [ ] end of options → status >0, `name=?`, OPTIND = index of the first operand
- [ ] track the position inside a group of options (`-abc`) between calls; reset it when OPTIND=1

#### `hash [utility...]` / `hash -r`
- [ ] remembered-location table `name → path` in `Shell`; spawn uses it (pass the absolute path to `Command::new`)
- [ ] `hash util...`: look each one up in PATH and remember it; not found → error >0
- [ ] `hash` with no arguments: list the remembered paths (built-ins are never listed)
- [ ] `hash -r`: clear the table; an assignment to `PATH` clears it too
- [ ] if a remembered path fails, search again

#### `jobs [-l|-p] [job_id...]`
- [x] job table with job number, `+`/`-` current/previous (a stopped job takes priority), command text (0.1)
- [x] default: print `"[%d] %c %s %s\n"` (number, current, state, command) for all jobs or the selected ones; a bad job ID is reported and the others still print, status 1
- [x] states: `Running`, `Done`, `Done(code)`, `Stopped (SIGTSTP|SIGSTOP|SIGTTIN|SIGTTOU)`, and `Terminated (SIGxxx)` for a job killed by a signal
- [x] `-l`: also print the process ID of the job (its group leader, `Job::leader`); `-p`: only that process ID
- [x] reporting a finished job removes it from the table; the shell's own reports (`notify_jobs`, `job_stopped`) use the same format
- [x] lists the shell's jobs in a forked child too (`jobs | cat`, `$(jobs -p)`)
- [ ] `-l`: the other processes of the job on their own lines

#### `kill`
- [ ] `kill pid...` sends SIGTERM by default
- [ ] `-s NAME`, `-NAME`, `-NUM`: names are case-insensitive without `SIG` (also accept `SIGxxx`), and `0` means signal 0; use `nix::Signal` (add `TryFrom<&str>` / a name table)
- [ ] a negative first argument is a signal, not a pid group; `--` then a negative pid → process group
- [ ] pid operands: number, `0` (own process group), negative (process group), `%job` (the job's pgid; after SIGCONT/SIGTERM on a stopped job also send SIGCONT so it can die)
- [ ] `kill -l`: print all signal names; `kill -l N`: name of signal N, or of exit status 128+N
- [ ] status 0 if every operand got signalled, otherwise >0 with `rush: kill: (pid) - <strerror>`

#### `newgrp` (low priority)
- [ ] leave it to `/usr/bin/newgrp` through PATH (it replaces the process anyway); optionally make it `exec newgrp`

#### `pwd [-L|-P]`
- [x] built in (shares `logical_cwd`/`current_dir` with `cd`)
- [x] `-L` (default): print `$PWD` if it is absolute, has no `.`/`..`, and names the cwd; otherwise behave like `-P`
- [x] `-P`: `getcwd` (`env::current_dir`) without symlinks
- [x] last of `-L`/`-P` wins; on error, print nothing to stdout, write `rush: pwd: <strerror>` and return 1
- [x] `pwd | cat`, and `pwd >file | cat` (in a pipeline the child applies the redirections, 0.5)
- [x] `pwd >file` outside a pipeline (0.4)

#### `read [-r] var...`
- [ ] read one logical line from fd 0 **without over-reading** (byte by byte, or through the shared REPL reader when stdin is the script)
- [ ] without `-r`: `\<newline>` continues the line (prompt `> `/PS2 when interactive on a tty), and `\c` keeps `c` literally
- [ ] split on IFS (default `<space><tab><newline>`; IFS whitespace vs non-whitespace delimiters); the last var gets the rest with trailing IFS whitespace removed; extra vars are set to empty
- [ ] invalid var name → error
- [ ] assigning to a readonly variable → error
- [ ] EOF before a newline → still assign what was read, then status >0
- [ ] inside a pipeline or subshell it changes only that subshell (falls out of 0.5; mind the stdin caveat there)

#### `true`
- [x] built in, returns 0; arguments are ignored

#### `type name...`
- [ ] for each name, say what it is: alias, reserved word, special built-in, function, built-in, or `name is /path`; not found → `rush: type: name: not found`, status >0
- [ ] share its implementation with `command -V`

#### `ulimit [-f] [blocks]`
- [ ] `ulimit` / `ulimit -f`: print `RLIMIT_FSIZE` in 512-byte blocks, or `unlimited`
- [ ] `ulimit [-f] N`: set the soft and hard limit (`setrlimit`); raising it without privilege → error >0
- [ ] optional extensions: `-a -c -d -n -s -t -v`

#### `umask [-S] [mask]`
- [ ] no mask: print the current mask in octal (`0022`) so that it can be read back in
- [ ] `-S`: `u=rwx,g=rx,o=rx` (the letters are the bits *clear* in the mask)
- [ ] octal mask → `umask(2)`
- [ ] symbolic mask (the chmod grammar `[ugoa]*[+-=][rwxXst]*`, comma-separated): `+` clears bits in the mask, `-` sets them, `=` replaces them
- [ ] invalid mask → error >0, and the mask stays the same
- [ ] add `umask` to `nix.rs` (read it with `umask(0)` and then restore)

#### `unalias alias-name...` / `unalias -a`
- [ ] remove the named aliases; a name that isn't an alias → status >0, and the rest still get processed
- [ ] `-a`: remove all of them

#### `wait [pid...]`
- [ ] needs `&` and `$!` (phase 2)
- [ ] no operands: wait for every known async pid, status 0
- [ ] `wait pid...` / `wait %job`: status of the last operand; killed by a signal → 128+signum; unknown pid → 127
- [ ] must be interruptible by trapped signals (status >128)
- [ ] remove the waited-for jobs from the table
