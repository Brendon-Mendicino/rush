use std::{
    collections::{HashMap, hash_map::Entry},
    env, io,
    ops::{Deref, DerefMut},
    os::fd::{AsRawFd, RawFd},
};

use crate::nix::{self, Pid, SigHandler, Signal};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VarAttr {
    /// The variable is exported to the environment of the child processes.
    pub exported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Var {
    pub value: String,
    pub attr: VarAttr,
}

#[derive(Debug, Clone, Default)]
pub struct ShellEnv {
    env: HashMap<String, Var>,
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
///   Readonly variables also have to be refused as prefixes in the first place.
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

impl ShellEnv {
    /// Create the shell variables from the environment of the process. All of them
    /// are marked as external.
    pub fn from_env() -> Self {
        let mut vars = Self::default();

        // Variables that are not valid UTF-8 are skipped, the rest of the shell
        // works only with `str`.
        for (name, value) in env::vars_os() {
            if let (Ok(name), Ok(value)) = (name.into_string(), value.into_string()) {
                vars.upsert(name, value, Some(VarAttr { exported: true }));
            }
        }

        vars
    }

    /// Insert or update a variable. When `attr` is `None` the attributes of an
    /// already existing variable are kept, a new variable gets the default ones.
    pub fn upsert(
        &mut self,
        name: impl Into<String>,
        value: impl Into<String>,
        attr: Option<VarAttr>,
    ) {
        let value = value.into();

        match self.env.entry(name.into()) {
            Entry::Occupied(mut entry) => {
                let var = entry.get_mut();
                var.value = value;
                if let Some(attr) = attr {
                    var.attr = attr;
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(Var {
                    value,
                    attr: attr.unwrap_or_default(),
                });
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.env.get(name).map(|var| var.value.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Var)> {
        self.env.iter().map(|(name, var)| (name.as_str(), var))
    }

    /// The variables passed down to the environment of the children.
    pub fn exported(&self) -> impl Iterator<Item = (&str, &str)> {
        self.iter()
            .filter(|(_, var)| var.attr.exported)
            .map(|(name, var)| (name, var.value.as_str()))
    }
}

impl Shell {
    /// Set variables, exported, until the returned guard is dropped (the
    /// `VAR=x` prefixes of a regular built-in, §2.9.1).
    pub fn set_temporarily<'a>(
        &mut self,
        vars: impl IntoIterator<Item = (&'a str, String)>,
    ) -> TempVarsGuard<'_> {
        let mut saved = Vec::new();

        for (name, value) in vars {
            saved.push((name.to_string(), self.env.env.get(name).cloned()));
            self.env
                .upsert(name, value, Some(VarAttr { exported: true }));
        }

        TempVarsGuard { shell: self, saved }
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
        let env = &mut self.shell.env.env;

        // Backwards: with `x=1 x=2 cmd` the first saved value is the original.
        for (name, var) in self.saved.drain(..).rev() {
            match var {
                Some(var) => env.insert(name, var),
                None => env.remove(&name),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_keeps_attributes_when_none() {
        let mut vars = ShellEnv::default();

        vars.upsert("A", "1", Some(VarAttr { exported: true }));
        vars.upsert("A", "2", None);
        vars.upsert("B", "3", None);

        assert_eq!(vars.get("A"), Some("2"));
        assert_eq!(vars.get("B"), Some("3"));
        assert_eq!(vars.get("C"), None);

        let attr = |name| vars.iter().find(|(n, _)| *n == name).unwrap().1.attr;
        assert!(attr("A").exported);
        assert!(!attr("B").exported);
    }

    #[test]
    fn env_vars_are_external() {
        let vars = ShellEnv::from_env();
        let path = env::var("PATH").expect("PATH is set in the test environment");

        assert_eq!(vars.get("PATH"), Some(path.as_str()));
        assert!(vars.iter().all(|(_, var)| var.attr.exported));
    }

    #[test]
    fn only_external_vars_are_exported() {
        let mut vars = ShellEnv::default();

        vars.upsert("ENV", "1", Some(VarAttr { exported: true }));
        vars.upsert("LOCAL", "2", None);
        // An assignment to an exported variable keeps it exported.
        vars.upsert("ENV", "3", None);

        let exported: Vec<_> = vars.exported().collect();
        assert_eq!(exported, [("ENV", "3")]);
    }

    #[test]
    fn temporary_variables_are_restored_on_drop() {
        let mut env = ShellEnv::default();
        env.upsert("KEPT", "old", None);
        let mut shell = Shell {
            pgid: Pid::from_raw(0),
            terminal: libc::STDIN_FILENO,
            is_interactive: false,
            tmodes: None,
            env,
        };

        let var = |env: &ShellEnv, name| env.env.get(name).cloned();

        {
            let guard = shell.set_temporarily([
                ("KEPT", "new".to_string()),
                ("NEW", "1".to_string()),
                ("NEW", "2".to_string()),
            ]);

            assert_eq!(guard.env.get("KEPT"), Some("new"));
            assert_eq!(guard.env.get("NEW"), Some("2"));
            // Exported while the command runs.
            assert!(var(&guard.env, "KEPT").unwrap().attr.exported);
            assert!(var(&guard.env, "NEW").unwrap().attr.exported);
        }

        assert_eq!(
            var(&shell.env, "KEPT"),
            Some(Var {
                value: "old".into(),
                attr: VarAttr { exported: false },
            })
        );
        assert_eq!(shell.env.get("NEW"), None);
    }
}
