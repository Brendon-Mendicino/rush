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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Signal {
    /// Hangup
    SIGHUP = libc::SIGHUP,
    /// Interrupt
    SIGINT = libc::SIGINT,
    /// Quit
    SIGQUIT = libc::SIGQUIT,
    /// Illegal instruction (not reset when caught)
    SIGILL = libc::SIGILL,
    /// Trace trap (not reset when caught)
    SIGTRAP = libc::SIGTRAP,
    /// Abort
    SIGABRT = libc::SIGABRT,
    /// Bus error
    SIGBUS = libc::SIGBUS,
    /// Floating point exception
    SIGFPE = libc::SIGFPE,
    /// Kill (cannot be caught or ignored)
    SIGKILL = libc::SIGKILL,
    /// User defined signal 1
    SIGUSR1 = libc::SIGUSR1,
    /// Segmentation violation
    SIGSEGV = libc::SIGSEGV,
    /// User defined signal 2
    SIGUSR2 = libc::SIGUSR2,
    /// Write on a pipe with no one to read it
    SIGPIPE = libc::SIGPIPE,
    /// Alarm clock
    SIGALRM = libc::SIGALRM,
    /// Software termination signal from kill
    SIGTERM = libc::SIGTERM,
    /// To parent on child stop or exit
    SIGCHLD = libc::SIGCHLD,
    /// Continue a stopped process
    SIGCONT = libc::SIGCONT,
    /// Sendable stop signal not from tty
    SIGSTOP = libc::SIGSTOP,
    /// Stop signal from tty
    SIGTSTP = libc::SIGTSTP,
    /// To readers pgrp upon background tty read
    SIGTTIN = libc::SIGTTIN,
    /// Like TTIN if (tp->t_local&LTOSTOP)
    SIGTTOU = libc::SIGTTOU,
    /// Urgent condition on IO channel
    SIGURG = libc::SIGURG,
    /// Exceeded CPU time limit
    SIGXCPU = libc::SIGXCPU,
    /// Exceeded file size limit
    SIGXFSZ = libc::SIGXFSZ,
    /// Virtual time alarm
    SIGVTALRM = libc::SIGVTALRM,
    /// Profiling time alarm
    SIGPROF = libc::SIGPROF,
    /// Window size changes
    SIGWINCH = libc::SIGWINCH,
    /// Bad system call
    SIGSYS = libc::SIGSYS,
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

impl TryFrom<libc::c_int> for Signal {
    type Error = io::Error;

    fn try_from(signum: libc::c_int) -> Result<Self, Self::Error> {
        use Signal::*;

        let signal = match signum {
            libc::SIGHUP => SIGHUP,
            libc::SIGINT => SIGINT,
            libc::SIGQUIT => SIGQUIT,
            libc::SIGILL => SIGILL,
            libc::SIGTRAP => SIGTRAP,
            libc::SIGABRT => SIGABRT,
            libc::SIGBUS => SIGBUS,
            libc::SIGFPE => SIGFPE,
            libc::SIGKILL => SIGKILL,
            libc::SIGUSR1 => SIGUSR1,
            libc::SIGSEGV => SIGSEGV,
            libc::SIGUSR2 => SIGUSR2,
            libc::SIGPIPE => SIGPIPE,
            libc::SIGALRM => SIGALRM,
            libc::SIGTERM => SIGTERM,
            libc::SIGCHLD => SIGCHLD,
            libc::SIGCONT => SIGCONT,
            libc::SIGSTOP => SIGSTOP,
            libc::SIGTSTP => SIGTSTP,
            libc::SIGTTIN => SIGTTIN,
            libc::SIGTTOU => SIGTTOU,
            libc::SIGURG => SIGURG,
            libc::SIGXCPU => SIGXCPU,
            libc::SIGXFSZ => SIGXFSZ,
            libc::SIGVTALRM => SIGVTALRM,
            libc::SIGPROF => SIGPROF,
            libc::SIGWINCH => SIGWINCH,
            libc::SIGSYS => SIGSYS,
            _ => return Err(io::Error::from_raw_os_error(libc::EINVAL)),
        };

        Ok(signal)
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

pub fn tcsetpgrp<Fd: AsRawFd>(fd: Fd, pgid: Pid) -> io::Result<()> {
    let res = unsafe { libc::tcsetpgrp(fd.as_raw_fd(), pgid.as_raw()) };
    cvt(res)?;
    Ok(())
}

pub fn tcgetattr<Fd: AsRawFd>(fd: Fd) -> io::Result<libc::termios> {
    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    let res = unsafe { libc::tcgetattr(fd.as_raw_fd(), termios.as_mut_ptr()) };
    cvt(res)?;
    Ok(unsafe { termios.assume_init() })
}

pub fn tcsetattr<Fd: AsRawFd>(
    fd: Fd,
    action: libc::c_int,
    termios: &libc::termios,
) -> io::Result<()> {
    let res = unsafe { libc::tcsetattr(fd.as_raw_fd(), action, termios) };
    cvt(res)?;
    Ok(())
}

pub fn isatty<Fd: AsRawFd>(fd: Fd) -> bool {
    unsafe { libc::isatty(fd.as_raw_fd()) == 1 }
}

pub fn getpgrp() -> Pid {
    // getpgrp() is always successful
    Pid(unsafe { libc::getpgrp() })
}

pub fn tcgetpgrp<Fd: AsRawFd>(fd: Fd) -> io::Result<Pid> {
    let res = unsafe { libc::tcgetpgrp(fd.as_raw_fd()) };
    Ok(Pid(cvt(res)?))
}

pub fn setpgid(pid: Pid, pgid: Pid) -> io::Result<()> {
    let res = unsafe { libc::setpgid(pid.as_raw(), pgid.as_raw()) };
    cvt(res)?;
    Ok(())
}

/// Disposition of a signal, see [`signal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigHandler {
    /// `SIG_DFL`
    Default,
    /// `SIG_IGN`
    Ignore,
}

/// Set the disposition of `sig` (implemented with `sigaction(2)`).
///
/// It is async-signal-safe, so it can be used inside a `pre_exec` closure.
pub fn signal(sig: Signal, handler: SigHandler) -> io::Result<()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = match handler {
        SigHandler::Default => libc::SIG_DFL,
        SigHandler::Ignore => libc::SIG_IGN,
    };

    let res = unsafe {
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(sig as libc::c_int, &action, std::ptr::null_mut())
    };
    cvt(res)?;
    Ok(())
}

pub fn kill(pid: Pid, sig: Signal) -> io::Result<()> {
    let res = unsafe { libc::kill(pid.as_raw(), sig as libc::c_int) };
    cvt(res)?;
    Ok(())
}

pub fn waitpid(pid: WaitPid, options: libc::c_int) -> io::Result<WaitRes> {
    let w_pid = match pid {
        WaitPid::Any => Pid(-1),
        WaitPid::CallerGroup => Pid(0),
        WaitPid::Single(pid) => pid,
        WaitPid::Group(pgid) => Pid(-pgid.as_raw()),
    };

    let mut wstatus: i32 = 0;
    let res: pid_t = unsafe { libc::waitpid(w_pid.as_raw(), &mut wstatus, options) };
    let res = cvt(res)?;
    if res == 0 {
        return Ok(WaitRes::StillAlive);
    }
    let pid = Pid::from_raw(res);

    let wait_res = if libc::WIFEXITED(wstatus) {
        WaitRes::Exited(pid, libc::WEXITSTATUS(wstatus))
    } else if libc::WIFSIGNALED(wstatus) {
        let termsig = Signal::try_from(libc::WTERMSIG(wstatus))?;
        let coredump = libc::WCOREDUMP(wstatus);
        WaitRes::Signaled(pid, termsig, coredump)
    } else if libc::WIFSTOPPED(wstatus) {
        let stopsig = Signal::try_from(libc::WSTOPSIG(wstatus))?;
        WaitRes::Stopped(pid, stopsig)
    } else {
        assert!(libc::WIFCONTINUED(wstatus));
        WaitRes::Continued(pid)
    };

    Ok(wait_res)
}
