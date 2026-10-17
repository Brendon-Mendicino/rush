use std::{
    collections::{
        HashMap,
        hash_map::{self, Entry},
    },
    env, io,
    os::fd::{AsRawFd, RawFd},
};

use crate::nix::{self, Pid, SigHandler, Signal};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VarAttr {
    /// The variable is exported to the environment of the child processes.
    pub external: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Var {
    pub value: String,
    pub attr: VarAttr,
}

#[derive(Debug, Clone, Default)]
pub struct ShellVars {
    vars: HashMap<String, Var>,
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
    pub vars: ShellVars,
}

/// Signals ignored by an interactive shell, the children restore them to the default.
pub const JOB_CONTROL_SIGNALS: [Signal; 5] = [
    Signal::SIGINT,
    Signal::SIGQUIT,
    Signal::SIGTSTP,
    Signal::SIGTTIN,
    Signal::SIGTTOU,
];

impl ShellVars {
    /// Create the shell variables from the environment of the process. All of them
    /// are marked as external.
    pub fn from_env() -> Self {
        let mut vars = Self::default();

        // Variables that are not valid UTF-8 are skipped, the rest of the shell
        // works only with `str`.
        for (name, value) in env::vars_os() {
            if let (Ok(name), Ok(value)) = (name.into_string(), value.into_string()) {
                vars.upsert(name, value, Some(VarAttr { external: true }));
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

        match self.vars.entry(name.into()) {
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
        self.vars.get(name).map(|var| var.value.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Var)> {
        self.vars.iter().map(|(name, var)| (name.as_str(), var))
    }
}

impl Shell {
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
            vars: ShellVars::from_env(),
        };

        if is_interactive {
            shell.init_job_control();
        }

        shell
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_keeps_attributes_when_none() {
        let mut vars = ShellVars::default();

        vars.upsert("A", "1", Some(VarAttr { external: true }));
        vars.upsert("A", "2", None);
        vars.upsert("B", "3", None);

        assert_eq!(vars.get("A"), Some("2"));
        assert_eq!(vars.get("B"), Some("3"));
        assert_eq!(vars.get("C"), None);

        let attr = |name| vars.iter().find(|(n, _)| *n == name).unwrap().1.attr;
        assert!(attr("A").external);
        assert!(!attr("B").external);
    }

    #[test]
    fn env_vars_are_external() {
        let vars = ShellVars::from_env();
        let path = env::var("PATH").expect("PATH is set in the test environment");

        assert_eq!(vars.get("PATH"), Some(path.as_str()));
        assert!(vars.iter().all(|(_, var)| var.attr.external));
    }
}
