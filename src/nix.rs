use libc::pid_t;
use std::{
    io,
    os::fd::{AsRawFd, RawFd},
};

#[doc(hidden)]
trait IsMinusOne {
    fn is_minus_one(&self) -> bool;
}

macro_rules! impl_is_minus_one {
    ($($t:ident)*) => ($(impl IsMinusOne for $t {
        fn is_minus_one(&self) -> bool {
            *self == -1
        }
    })*)
}

impl_is_minus_one! { i8 i16 i32 i64 isize }

/// Process identifier
///
/// Newtype pattern around `pid_t` (which is just alias). It prevents bugs caused by accidentally
/// passing wrong value.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Pid(pid_t);

/// Types of operating system signals
// Currently there is only one definition of c_int in libc, as well as only one
// type for signal constants.
// We would prefer to use the libc::c_int alias in the repr attribute. Unfortunately
// this is not (yet) possible.
#[repr(i32)]
#[non_exhaustive]
pub enum Signal {
    /// Hangup
    SIGHUP,
    /// Interrupt
    SIGINT,
    /// Quit
    SIGQUIT,
    /// Illegal instruction (not reset when caught)
    SIGILL,
    /// Trace trap (not reset when caught)
    SIGTRAP,
    /// Abort
    SIGABRT,
    /// Bus error
    SIGBUS,
    /// Floating point exception
    SIGFPE,
    /// Kill (cannot be caught or ignored)
    SIGKILL,
    /// User defined signal 1
    SIGUSR1,
    /// Segmentation violation
    SIGSEGV,
    /// User defined signal 2
    SIGUSR2,
    /// Write on a pipe with no one to read it
    SIGPIPE,
    /// Alarm clock
    SIGALRM,
    /// Software termination signal from kill
    SIGTERM,
    /// To parent on child stop or exit
    SIGCHLD,
    /// Continue a stopped process
    SIGCONT,
    /// Sendable stop signal not from tty
    SIGSTOP,
    /// Stop signal from tty
    SIGTSTP,
    /// To readers pgrp upon background tty read
    SIGTTIN,
    /// Like TTIN if (tp->t_local&LTOSTOP)
    SIGTTOU,
    /// Urgent condition on IO channel
    SIGURG,
    /// Exceeded CPU time limit
    SIGXCPU,
    /// Exceeded file size limit
    SIGXFSZ,
    /// Virtual time alarm
    SIGVTALRM,
    /// Profiling time alarm
    SIGPROF,
    /// Window size changes
    SIGWINCH,
    /// Bad system call
    SIGSYS,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitPid {
    Any,
    CallerGroup,
    Single(Pid),
    Group(Pid),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitRes {
    /// The process exited normally (as with `exit()` or returning from
    /// `main`) with the given exit code. This case matches the C macro
    /// `WIFEXITED(status)`; the second field is `WEXITSTATUS(status)`.
    Exited(Pid, i32),
    /// The process was killed by the given signal. The third field
    /// indicates whether the signal generated a core dump. This case
    /// matches the C macro `WIFSIGNALED(status)`; the last two fields
    /// correspond to `WTERMSIG(status)` and `WCOREDUMP(status)`.
    Signaled(Pid, Signal, bool),
    /// The process is alive, but was stopped by the given signal. This
    /// is only reported if `WaitPidFlag::WUNTRACED` was passed. This
    /// case matches the C macro `WIFSTOPPED(status)`; the second field
    /// is `WSTOPSIG(status)`.
    Stopped(Pid, Signal),
    /// The process was previously stopped but has resumed execution
    /// after receiving a `SIGCONT` signal. This is only reported if
    /// `WaitPidFlag::WCONTINUED` was passed. This case matches the C
    /// macro `WIFCONTINUED(status)`.
    Continued(Pid),
    /// There are currently no state changes to report in any awaited
    /// child process. This is only returned if `WaitPidFlag::WNOHANG`
    /// was used (otherwise `wait()` or `waitpid()` would block until
    /// there was something to report).
    StillAlive,
}

impl Pid {
    /// Get the pid of this process (see
    /// [getpid(2)](https://pubs.opengroup.org/onlinepubs/9699919799/functions/getpid.html)).
    ///
    /// Since you are running code, there is always a pid to return, so there
    /// is no error case that needs to be handled.
    #[inline]
    fn getpid() -> Pid {
        Pid(unsafe { libc::getpid() })
    }

    /// Get the pid of this processes' parent (see
    /// [getpid(2)](https://pubs.opengroup.org/onlinepubs/9699919799/functions/getppid.html)).
    ///
    /// There is always a parent pid to return, so there is no error case that needs
    /// to be handled.
    #[inline]
    fn getppid() -> Pid {
        Pid(unsafe { libc::getppid() }) // no error handling, according to man page: "These functions are always successful."
    }

    /// Creates `Pid` from raw `pid_t`.
    pub const fn from_raw(pid: pid_t) -> Self {
        Pid(pid)
    }

    /// Returns PID of calling process
    pub fn this() -> Self {
        Self::getpid()
    }

    /// Returns PID of parent of calling process
    pub fn parent() -> Self {
        Self::getppid()
    }

    /// Get the raw `pid_t` wrapped by `self`.
    pub const fn as_raw(self) -> pid_t {
        self.0
    }
}

impl From<Pid> for pid_t {
    fn from(pid: Pid) -> Self {
        pid.0
    }
}

/// Converts native return values to Result using the *-1 means error is in `errno`*  convention.
/// Non-error values are `Ok`-wrapped.
fn cvt<T: IsMinusOne>(t: T) -> io::Result<T> {
    if t.is_minus_one() {
        Err(io::Error::last_os_error())
    } else {
        Ok(t)
    }
}

pub fn close<Fd: AsRawFd>(fd: Fd) -> io::Result<()> {
    let res = unsafe { libc::close(fd.as_raw_fd()) };
    cvt(res)?;
    Ok(())
}

pub fn dup2_raw<Fd: AsRawFd>(oldfd: Fd, newfd: Fd) -> io::Result<RawFd> {
    let res = unsafe { libc::dup2(oldfd.as_raw_fd(), newfd.as_raw_fd()) };
    cvt(res)
}

pub fn waitpid(pid: WaitPid) {
    let w_pid = match pid {
        WaitPid::Any => Pid(-1),
        WaitPid::CallerGroup => Pid(0),
        WaitPid::Single(pid) => pid,
        WaitPid::Group(pgid) => Pid(-pgid.as_raw()),
    };

    let mut wstatus: i32 = 0;
    let res = unsafe { libc::waitpid(w_pid.as_raw(), &mut wstatus, libc::WUNTRACED) };
    let pid = cvt(res)?;

    let wait_res = if libc::WIFEXITED(wstatus) {
        WaitRes::Exited(pid, libc::WEXITSTATUS(wstatus))
    } else if libc::WIFSIGNALED(wstatus) {
        WaitRes::Signaled(pid, term_signal(status)?, dumped_core(status))
    } else if stopped(status) {
        cfg_if! {
            if #[cfg(linux_android)] {
                fn decode_stopped(pid: Pid, status: i32) -> Result<WaitStatus> {
                    let status_additional = stop_additional(status);
                    Ok(if syscall_stop(status) {
                        WaitStatus::PtraceSyscall(pid)
                    } else if status_additional == 0 {
                        WaitStatus::Stopped(pid, stop_signal(status)?)
                    } else {
                        WaitStatus::PtraceEvent(pid, stop_signal(status)?,
                                                stop_additional(status))
                    })
                }
            } else {
                fn decode_stopped(pid: Pid, status: i32) -> Result<WaitStatus> {
                    Ok(WaitStatus::Stopped(pid, stop_signal(status)?))
                }
            }
        }
        return decode_stopped(pid, status);
    } else {
        assert!(continued(status));
        WaitStatus::Continued(pid)
    };

    Ok(wait_res)
}
