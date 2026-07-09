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
