use std::{
    collections::BTreeMap,
    env, fmt, io,
    ops::{Deref, DerefMut},
    os::fd::{AsRawFd, RawFd},
};

use crate::nix::{self, Pid, SigHandler, Signal};

/// The value of a shell variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// No value, only attributes: `export X` or `readonly X` before `X` is
    /// assigned. `$X` expands to nothing, `${X-word}` to `word`, and it is not
    /// passed to the children.
    Unset,
    /// A value, possibly empty: after `X=` the variable is set but null,
    /// `${X-word}` expands to nothing and `${X:-word}` to `word`.
    Set(String),
}

/// The attributes of a shell variable, independent of each other and of the
/// value. None of them can be removed, except by `unset`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VarAttr {
    /// Passed to the environment of the child processes, when it has a value
    /// (`export`).
    pub exported: bool,
    /// The value can't change and the variable can't be unset (`readonly`). The
    /// export attribute can still be added.
    pub readonly: bool,
}

/// A shell variable. Every combination of value and attributes can happen:
///
/// | value  | exported | readonly | reached by                  | children | assign | unset |
/// |--------|----------|----------|-----------------------------|----------|--------|-------|
/// | Unset  |          |          | not stored: never set, or `unset X` | no | yes | yes |
/// | Unset  | x        |          | `export X`                  | no       | yes, then exported | yes |
/// | Unset  |          | x        | `readonly X`                | no       | error  | error |
/// | Unset  | x        | x        | `export X; readonly X`      | never    | error  | error |
/// | Set(v) |          |          | `X=v`                       | no       | yes    | yes   |
/// | Set(v) | x        |          | `export X=v`, the environment | yes    | yes, stays exported | yes |
/// | Set(v) |          | x        | `readonly X=v`              | no       | error  | error |
/// | Set(v) | x        | x        | `export X=v; readonly X`    | yes      | error  | error |
///
/// An "error" is a variable assignment error ([`ReadonlyError`], POSIX §2.8.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Var {
    pub value: Value,
    pub attr: VarAttr,
}

/// All the shell variables, exported or not, sorted by name (for `set`,
/// `export -p`, `readonly -p`). A variable `Unset` without attributes is never
/// stored: it is the same as one the shell never heard of.
#[derive(Debug, Clone, Default)]
pub struct ShellEnv {
    vars: BTreeMap<String, Var>,
}

/// An assignment to, or the `unset` of, a readonly variable: a variable
/// assignment error (§2.8.1), which exits a non-interactive shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadonlyError {
    pub name: String,
}

#[derive(Debug)]
pub struct Shell {
    pub pgid: Pid,
    pub terminal: RawFd,
    pub is_interactive: bool,
    // TODO: decide if tmodes should be incorporated in a safe struct
    /// Terminal modes of the shell, saved at startup. `None` when the shell is
    /// not interactive or they could not be read.
    pub tmodes: Option<libc::termios>,
    pub env: ShellEnv,
}

/// Variables set for the duration of a command (the `VAR=x` prefixes of a
/// regular built-in, §2.9.1), created by [`Shell::set_temporarily`]. It hands
/// out the shell, for the command to run with; dropping it puts back what the
/// variables were, the old value and attributes, or nothing for a variable that
/// didn't exist.
///
/// TODO: decide what happens to these variables when the command itself
/// changes them. Restoring blindly undoes the command's own assignment:
/// - `PWD=/x cd /tmp` leaves the old `PWD` behind, wrong after `cd` changed it
///   (`OLDPWD` too);
/// - `VAR=x read VAR` (BUILTINS.md) loses what `read` assigned, and the same for
///   `getopts` with `OPTIND`/`OPTARG`;
/// - `export`, `readonly`, `unset` and `set` are special built-ins, so their
///   prefixes are not temporary, but a function or `command` (which strips the
///   special properties) could run them while a guard is alive: an `unset VAR`
///   or `export VAR` would be undone, and `readonly VAR` would be overwritten.
#[must_use = "the variables are put back as soon as the guard is dropped"]
#[derive(Debug)]
pub struct TempVarsGuard<'s> {
    shell: &'s mut Shell,
    saved: Vec<(String, Option<Var>)>,
}

/// Signals ignored by an interactive shell, the children restore them to the default.
pub const JOB_CONTROL_SIGNALS: [Signal; 5] = [
    Signal::SIGINT,
    Signal::SIGQUIT,
    Signal::SIGTSTP,
    Signal::SIGTTIN,
    Signal::SIGTTOU,
];

impl Value {
    /// The value, when it is set.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Unset => None,
            Self::Set(value) => Some(value),
        }
    }
}

impl ShellEnv {
    /// Create the shell variables from the environment of the process. All of them
    /// are set and exported.
    pub fn from_env() -> Self {
        let mut env = Self::default();

        // Variables that are not valid UTF-8 are skipped, the rest of the shell
        // works only with `str`.
        for (name, value) in env::vars_os() {
            if let (Ok(name), Ok(value)) = (name.into_string(), value.into_string()) {
                let attr = VarAttr {
                    exported: true,
                    readonly: false,
                };
                env.vars.insert(
                    name,
                    Var {
                        value: Value::Set(value),
                        attr,
                    },
                );
            }
        }

        env
    }

    /// The value of a variable, when it is set.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).and_then(|var| var.value.as_str())
    }

    /// The variable, also when it has only attributes.
    pub fn var(&self, name: &str) -> Option<&Var> {
        self.vars.get(name)
    }

    /// The variable can be assigned or unset: it is not readonly.
    pub fn check_assignable(&self, name: &str) -> Result<(), ReadonlyError> {
        match self.vars.get(name) {
            Some(var) if var.attr.readonly => Err(ReadonlyError {
                name: name.to_string(),
            }),
            _ => Ok(()),
        }
    }

    /// `name=value`: set the value, keeping the attributes. A new variable gets
    /// none, an exported one stays exported.
    pub fn assign(&mut self, name: &str, value: impl Into<String>) -> Result<(), ReadonlyError> {
        self.check_assignable(name)?;
        self.entry(name).value = Value::Set(value.into());
        Ok(())
    }

    /// `export name[=value]`: add the export attribute, and set the value when
    /// there is one. Without a value an unset variable stays unset.
    pub fn export(&mut self, name: &str, value: Option<String>) -> Result<(), ReadonlyError> {
        if value.is_some() {
            self.check_assignable(name)?;
        }

        let var = self.entry(name);
        var.attr.exported = true;
        if let Some(value) = value {
            var.value = Value::Set(value);
        }
        Ok(())
    }

    /// `readonly name[=value]`: add the readonly attribute, and set the value
    /// when there is one. A variable that is already readonly can't get a value.
    pub fn set_readonly(&mut self, name: &str, value: Option<String>) -> Result<(), ReadonlyError> {
        if value.is_some() {
            self.check_assignable(name)?;
        }

        let var = self.entry(name);
        var.attr.readonly = true;
        if let Some(value) = value {
            var.value = Value::Set(value);
        }
        Ok(())
    }

    /// `unset name`: remove the variable, value and attributes. A variable that
    /// doesn't exist is not an error.
    pub fn unset(&mut self, name: &str) -> Result<(), ReadonlyError> {
        self.check_assignable(name)?;
        self.vars.remove(name);
        Ok(())
    }

    /// Every variable, sorted by name, also the ones that have only attributes.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Var)> {
        self.vars.iter().map(|(name, var)| (name.as_str(), var))
    }

    /// The variables passed down to the environment of the children: exported
    /// and set.
    pub fn exported(&self) -> impl Iterator<Item = (&str, &str)> {
        self.iter()
            .filter(|(_, var)| var.attr.exported)
            .filter_map(|(name, var)| Some((name, var.value.as_str()?)))
    }

    /// The variable, created unset and without attributes when missing. The
    /// caller gives it a value or an attribute right away.
    fn entry(&mut self, name: &str) -> &mut Var {
        self.vars.entry(name.to_string()).or_insert(Var {
            value: Value::Unset,
            attr: VarAttr::default(),
        })
    }
}

impl Shell {
    /// Set variables, exported, until the returned guard is dropped (the
    /// `VAR=x` prefixes of a regular built-in, §2.9.1). A readonly variable is
    /// an error, and the ones already set are put back.
    pub fn set_temporarily<'a>(
        &mut self,
        vars: impl IntoIterator<Item = (&'a str, String)>,
    ) -> Result<TempVarsGuard<'_>, ReadonlyError> {
        // Filled one variable at a time: on an error, dropping it restores the
        // ones before.
        let mut guard = TempVarsGuard {
            shell: self,
            saved: Vec::new(),
        };

        for (name, value) in vars {
            let env = &mut guard.shell.env;
            env.check_assignable(name)?;

            // Not readonly, it was checked.
            let attr = VarAttr {
                exported: true,
                readonly: false,
            };
            let old = env.vars.insert(
                name.to_string(),
                Var {
                    value: Value::Set(value),
                    attr,
                },
            );
            guard.saved.push((name.to_string(), old));
        }

        Ok(guard)
    }

    /// Make sure the shell is running interactively as the foreground job
    /// before proceeding.
    pub fn new() -> Self {
        // See if we are running interactively.
        let terminal = io::stdin().as_raw_fd();
        let is_interactive = nix::isatty(terminal);

        let mut shell = Self {
            pgid: nix::getpgrp(),
            terminal,
            is_interactive,
            tmodes: None,
            env: ShellEnv::from_env(),
        };

        if is_interactive {
            shell.init_job_control();
        }

        shell
    }

    /// The fds the shell opened for itself (the copy of the terminal). They are
    /// not the user's: redirections must not use them (`n>&m`).
    pub fn internal_fds(&self) -> Vec<RawFd> {
        // Without job control the "terminal" is just stdin.
        if self.terminal > libc::STDERR_FILENO {
            vec![self.terminal]
        } else {
            Vec::new()
        }
    }

    fn init_job_control(&mut self) {
        // Keep the terminal on an fd of its own: children take the terminal in
        // `pre_exec`, when their stdin may already be a pipe or /dev/null.
        // Close-on-exec keeps it from leaking into the programs.
        match nix::dup_cloexec(self.terminal, 10) {
            Ok(fd) => self.terminal = fd,
            Err(e) => eprintln!("fcntl({}, F_DUPFD_CLOEXEC): {e}", self.terminal),
        }

        // Loop until we are in the foreground.
        loop {
            self.pgid = nix::getpgrp();

            match nix::tcgetpgrp(self.terminal) {
                Ok(fg_pgid) if fg_pgid == self.pgid => break,
                Ok(_) => {
                    let _ = nix::kill(Pid::from_raw(-self.pgid.as_raw()), Signal::SIGTTIN);
                }
                Err(e) => {
                    eprintln!("tcgetpgrp({}): {e}", self.terminal);
                    break;
                }
            }
        }

        // Ignore interactive and job-control signals.
        // TODO: check how to handle SIGCHLD
        for sig in JOB_CONTROL_SIGNALS {
            if let Err(e) = nix::signal(sig, SigHandler::Ignore) {
                eprintln!("signal({sig:?}): {e}");
            }
        }

        // Put ourselves in our own process group. A session leader (e.g. a shell
        // started by `script`) already leads its group, and setpgid would fail
        // with EPERM.
        let pid = Pid::this();
        if self.pgid != pid {
            match nix::setpgid(pid, pid) {
                Ok(()) => self.pgid = pid,
                Err(e) => {
                    eprintln!("Couldn't put the shell in its own process group: {e}");
                    self.pgid = nix::getpgrp();
                }
            }
        }

        // Grab control of the terminal.
        if let Err(e) = nix::tcsetpgrp(self.terminal, self.pgid) {
            eprintln!("tcsetpgrp({}, {}): {e}", self.terminal, self.pgid.as_raw());
        }

        // Save default terminal attributes for shell.
        self.tmodes = match nix::tcgetattr(self.terminal) {
            Ok(tmodes) => Some(tmodes),
            Err(e) => {
                eprintln!("tcgetattr({}): {e}", self.terminal);
                None
            }
        };
    }
}

impl Deref for TempVarsGuard<'_> {
    type Target = Shell;

    fn deref(&self) -> &Shell {
        self.shell
    }
}

impl DerefMut for TempVarsGuard<'_> {
    fn deref_mut(&mut self) -> &mut Shell {
        self.shell
    }
}

impl Drop for TempVarsGuard<'_> {
    fn drop(&mut self) {
        let vars = &mut self.shell.env.vars;

        // Backwards: with `x=1 x=2 cmd` the first saved value is the original.
        for (name, var) in self.saved.drain(..).rev() {
            match var {
                Some(var) => vars.insert(name, var),
                None => vars.remove(&name),
            };
        }
    }
}

impl fmt::Display for ReadonlyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: readonly variable", self.name)
    }
}

/// Quote a value so that the shell reads it back as it is: between single
/// quotes, each `'` written as `'\''`. For the listings of `set`, `export -p`
/// and `readonly -p`.
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPORTED: VarAttr = VarAttr {
        exported: true,
        readonly: false,
    };

    fn set(value: &str, attr: VarAttr) -> Option<Var> {
        Some(Var {
            value: Value::Set(value.into()),
            attr,
        })
    }

    fn unset(attr: VarAttr) -> Option<Var> {
        Some(Var {
            value: Value::Unset,
            attr,
        })
    }

    fn shell(env: ShellEnv) -> Shell {
        Shell {
            pgid: Pid::from_raw(0),
            terminal: libc::STDIN_FILENO,
            is_interactive: false,
            tmodes: None,
            env,
        }
    }

    #[test]
    fn assign_keeps_the_attributes() {
        let mut env = ShellEnv::default();

        env.export("A", Some("1".into())).unwrap();
        env.assign("A", "2").unwrap();
        env.assign("B", "3").unwrap();

        assert_eq!(env.var("A").cloned(), set("2", EXPORTED));
        assert_eq!(env.var("B").cloned(), set("3", VarAttr::default()));
        assert_eq!(env.var("C"), None);
    }

    #[test]
    fn null_is_not_unset() {
        let mut env = ShellEnv::default();

        env.assign("X", "").unwrap();

        assert_eq!(env.get("X"), Some(""));
        assert_eq!(env.var("X").cloned(), set("", VarAttr::default()));
    }

    #[test]
    fn env_vars_are_external() {
        let env = ShellEnv::from_env();
        let path = env::var("PATH").expect("PATH is set in the test environment");

        assert_eq!(env.get("PATH"), Some(path.as_str()));
        assert!(env.iter().all(|(_, var)| var.attr == EXPORTED));
        assert!(
            env.iter()
                .all(|(_, var)| matches!(var.value, Value::Set(_)))
        );
    }

    #[test]
    fn only_exported_set_vars_are_passed_down() {
        let mut env = ShellEnv::default();

        env.export("ENV", Some("1".into())).unwrap();
        env.assign("LOCAL", "2").unwrap();
        env.export("NO_VALUE", None).unwrap();
        // An assignment to an exported variable keeps it exported.
        env.assign("ENV", "3").unwrap();

        let exported: Vec<_> = env.exported().collect();
        assert_eq!(exported, [("ENV", "3")]);
    }

    #[test]
    fn export_without_a_value_keeps_it_unset() {
        let mut env = ShellEnv::default();

        env.export("X", None).unwrap();

        assert_eq!(env.get("X"), None);
        assert_eq!(env.var("X").cloned(), unset(EXPORTED));

        // Exported as soon as it gets a value.
        env.assign("X", "1").unwrap();
        assert_eq!(env.var("X").cloned(), set("1", EXPORTED));
        assert_eq!(env.exported().collect::<Vec<_>>(), [("X", "1")]);
    }

    #[test]
    fn unset_removes_the_value_and_the_attributes() {
        let mut env = ShellEnv::default();
        env.export("X", Some("1".into())).unwrap();

        env.unset("X").unwrap();
        assert_eq!(env.var("X"), None);

        env.assign("X", "2").unwrap();
        assert_eq!(env.var("X").cloned(), set("2", VarAttr::default()));

        // Nothing to unset is not an error.
        assert_eq!(env.unset("NOPE"), Ok(()));
    }

    #[test]
    fn a_readonly_variable_cant_change() {
        let mut env = ShellEnv::default();
        env.set_readonly("X", Some("1".into())).unwrap();
        let readonly = VarAttr {
            exported: false,
            readonly: true,
        };
        let error = Err(ReadonlyError { name: "X".into() });

        assert_eq!(env.assign("X", "2"), error);
        assert_eq!(env.unset("X"), error);
        assert_eq!(env.export("X", Some("2".into())), error);
        assert_eq!(env.set_readonly("X", Some("2".into())), error);
        assert_eq!(env.var("X").cloned(), set("1", readonly));
        let message = ReadonlyError { name: "X".into() }.to_string();
        assert_eq!(message, "X: readonly variable");

        // The attributes can still be added.
        assert_eq!(env.set_readonly("X", None), Ok(()));
        assert_eq!(env.export("X", None), Ok(()));
        let both = VarAttr {
            exported: true,
            readonly: true,
        };
        assert_eq!(env.var("X").cloned(), set("1", both));
    }

    #[test]
    fn a_readonly_unset_variable_stays_unset() {
        let mut env = ShellEnv::default();

        env.set_readonly("X", None).unwrap();

        assert!(env.assign("X", "1").is_err());
        assert!(env.unset("X").is_err());
        let readonly = VarAttr {
            exported: false,
            readonly: true,
        };
        assert_eq!(env.var("X").cloned(), unset(readonly));
    }

    #[test]
    fn variables_are_sorted() {
        let mut env = ShellEnv::default();
        for name in ["b", "C", "a", "B"] {
            env.assign(name, "").unwrap();
        }

        let names: Vec<_> = env.iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["B", "C", "a", "b"]);
    }

    #[test]
    fn temporary_variables_are_restored_on_drop() {
        let mut env = ShellEnv::default();
        env.assign("KEPT", "old").unwrap();
        let mut shell = shell(env);

        {
            let guard = shell
                .set_temporarily([
                    ("KEPT", "new".to_string()),
                    ("NEW", "1".to_string()),
                    ("NEW", "2".to_string()),
                ])
                .unwrap();

            // Exported while the command runs.
            assert_eq!(guard.env.var("KEPT").cloned(), set("new", EXPORTED));
            assert_eq!(guard.env.var("NEW").cloned(), set("2", EXPORTED));
        }

        assert_eq!(
            shell.env.var("KEPT").cloned(),
            set("old", VarAttr::default())
        );
        assert_eq!(shell.env.var("NEW"), None);
    }

    #[test]
    fn a_readonly_temporary_variable_is_refused() {
        let mut env = ShellEnv::default();
        env.assign("A", "old").unwrap();
        env.set_readonly("RO", Some("1".into())).unwrap();
        let mut shell = shell(env);

        let result = shell.set_temporarily([
            ("A", "new".to_string()),
            ("B", "new".to_string()),
            ("RO", "2".to_string()),
        ]);

        assert_eq!(result.err(), Some(ReadonlyError { name: "RO".into() }));
        // The ones set before the error are put back.
        assert_eq!(shell.env.var("A").cloned(), set("old", VarAttr::default()));
        assert_eq!(shell.env.var("B"), None);
        assert_eq!(shell.env.get("RO"), Some("1"));
    }

    #[test]
    fn values_are_quoted_for_reinput() {
        assert_eq!(quote(""), "''");
        assert_eq!(quote("abc"), "'abc'");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote("$x `y` \\"), r"'$x `y` \'");
    }
}
