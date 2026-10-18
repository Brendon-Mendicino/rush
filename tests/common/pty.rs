//! The shell in interactive mode: it runs on a pseudo-terminal as the leader of
//! its own session, like in a terminal emulator. Its stdout is the pty (with
//! echo turned off, so the output holds only what the commands print) and its
//! stderr a pipe, so the syntax tree dumps stay out of the way.
//!
//! A pty is a pair of connected fds:
//! - the *slave* (`/dev/pts/N`) behaves like a real terminal: it is the shell's
//!   stdin/stdout, so `isatty()` is true and `tcsetpgrp` works on it;
//! - the *master* is the side a terminal emulator holds: bytes written to it
//!   arrive on the slave as if typed on a keyboard, and what programs write to
//!   the slave can be read from it.
//!
//! In between sits the kernel's terminal driver (the line discipline). It turns
//! `\x03` (Ctrl-C) into SIGINT, `\x1a` (Ctrl-Z) into SIGTSTP and `\x04` (Ctrl-D)
//! into end of input, and sends the signals to the terminal's foreground process
//! group. So when a test sends `"\x03"`, the kernel does the signalling, exactly
//! as with a real keyboard.

use std::{
    ffi::CStr,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::OpenOptionsExt, process::CommandExt},
    },
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

/// How long `expect` & co. wait before failing the test.
pub const TIMEOUT: Duration = Duration::from_secs(5);
/// End of the prompt, the same after a success and a failure.
pub const PROMPT: &str = "$ \x1b[0m";
/// Printed right before `$` when the previous command failed (red prompt).
pub const RED: &str = "\x1b[31m";

/// Output of a stream, collected by a background thread so that the test never
/// blocks on a read. Tests wait for it with [`Captured::expect`], which polls
/// the buffer: no fixed sleeps.
pub struct Captured {
    data: Arc<Mutex<Vec<u8>>>,
    /// Everything before this offset was already returned by `expect`.
    seen: usize,
}

impl Captured {
    fn new(mut source: impl Read + Send + 'static) -> Self {
        let data = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&data);

        thread::spawn(move || {
            let mut buf = [0; 4096];
            // Stops at EOF (the stderr pipe) or at an error: reading the pty
            // master fails with EIO once nobody has the slave open any more,
            // i.e. the shell and all its children are gone.
            while let Ok(n @ 1..) = source.read(&mut buf) {
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });

        Self { data, seen: 0 }
    }

    /// Wait for `needle` in the output not seen yet and return the output up to
    /// the end of it. The next call only looks after it, so successive calls
    /// follow the output in order. Panics with the unseen output on timeout.
    pub fn expect(&mut self, needle: &str) -> String {
        let deadline = Instant::now() + TIMEOUT;

        loop {
            let unseen =
                String::from_utf8_lossy(&self.data.lock().unwrap()[self.seen..]).into_owned();

            if let Some(pos) = unseen.find(needle) {
                let end = pos + needle.len();
                self.seen += unseen[..end].len();
                return unseen[..end].to_owned();
            }

            assert!(
                Instant::now() < deadline,
                "timed out waiting for {needle:?}, got {unseen:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// The shell running on a pty. Type into it with [`InteractiveShell::send`],
/// read what it prints through `stdout` (the pty) and `stderr` (a pipe).
pub struct InteractiveShell {
    pub child: Child,
    /// Our end of the pty: writes are keyboard input, reads are the output.
    pub master: File,
    pub stdout: Captured,
    pub stderr: Captured,
}

impl InteractiveShell {
    /// Start the shell on a fresh pty and wait for its first prompt.
    pub fn spawn() -> Self {
        let (master, slave) = open_pty();

        // stdin and stdout on the slave make the shell interactive (`isatty(0)`).
        // stderr stays a pipe: the syntax tree dumps and the job messages
        // ("stopped by SIGTSTP", ...) don't mix with what the commands print.
        let mut command = Command::new(env!("CARGO_BIN_EXE_rush"));
        command
            .stdin(slave.try_clone().unwrap())
            .stdout(slave)
            .stderr(Stdio::piped());

        // Runs in the child between fork and exec. Gives the shell what a terminal
        // emulator gives it: a session of its own, with the pty as its terminal.
        //
        // - `setsid()`: new session and new process group, with the child as the
        //   leader of both. It also detaches the child from the terminal of the
        //   test process, if it had one.
        // - `ioctl(0, TIOCSCTTY, 0)`: make fd 0 (the slave) the controlling
        //   terminal of the session. Only a session leader without a terminal
        //   can do that, hence `setsid()` first. From now on the keys typed on the
        //   master signal the terminal's foreground group, and background groups
        //   touching it get SIGTTIN/SIGTTOU.
        //
        // As a session leader the shell already leads its own process group, so
        // it skips `setpgid` (see `Shell::init_job_control`).
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let mut child = command.spawn().expect("failed to start the shell");
        // Dropping `command` closed our copies of the slave: only the shell and
        // its children have it open, so the master reports EIO once they are
        // gone and the reader thread ends.
        drop(command);

        let stderr = Captured::new(child.stderr.take().unwrap());
        let stdout = Captured::new(master.try_clone().unwrap());

        let mut shell = Self {
            child,
            master,
            stdout,
            stderr,
        };
        shell.expect_prompt();
        shell
    }

    pub fn pid(&self) -> i32 {
        self.child.id() as i32
    }

    /// Type `input` on the terminal. Control characters work like the keys:
    /// `"\x03"` Ctrl-C, `"\x1a"` Ctrl-Z, `"\x04"` Ctrl-D.
    pub fn send(&mut self, input: &str) {
        self.master.write_all(input.as_bytes()).unwrap();
    }

    /// Wait for the next prompt, return what was printed before it.
    pub fn expect_prompt(&mut self) -> String {
        self.stdout.expect(PROMPT)
    }

    /// Process group owning the terminal. `tcgetpgrp` on the master returns the
    /// foreground group of the slave (Linux), so this tells whether a job or the
    /// shell has the terminal.
    pub fn foreground_pgid(&self) -> i32 {
        let pgid = unsafe { libc::tcgetpgrp(self.master.as_raw_fd()) };
        assert!(pgid > 0, "tcgetpgrp: {}", io::Error::last_os_error());
        pgid
    }

    /// Wait until a job owns the terminal and its leader exec'd `program`
    /// (from then on it has the default signal handling). Returns its pgid.
    ///
    /// Waiting for the exec matters before sending Ctrl-C/Ctrl-Z: the child takes
    /// the terminal in `pre_exec` *before* it resets SIGINT/SIGTSTP to default,
    /// so a key sent in that window would be ignored. `/proc/<pid>/comm` holds
    /// the name of the program the process is running, it changes at exec.
    pub fn wait_foreground_job(&self, program: &str) -> i32 {
        let deadline = Instant::now() + TIMEOUT;

        loop {
            let pgid = self.foreground_pgid();
            let comm = fs::read_to_string(format!("/proc/{pgid}/comm")).unwrap_or_default();

            if pgid != self.pid() && comm.trim_end() == program {
                return pgid;
            }

            assert!(
                Instant::now() < deadline,
                "{program} never got the terminal"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Kill and reap the shell. Its children go too: when the session leader dies
/// the kernel sends SIGHUP to the terminal's foreground group, and SIGHUP +
/// SIGCONT to stopped groups left without their shell (orphaned groups).
impl Drop for InteractiveShell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Open a pty pair `(master, slave)`, with echo turned off.
fn open_pty() -> (File, File) {
    unsafe {
        // Open `/dev/ptmx`: returns a new master, the kernel creates its slave.
        // - O_NOCTTY: the pty must not become the test process's terminal.
        // - O_CLOEXEC: tests run in parallel; a copy of this master leaking into
        //   another test's shell would keep this pty alive after the test.
        let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        assert!(fd >= 0, "posix_openpt: {}", io::Error::last_os_error());
        let master = File::from_raw_fd(fd);

        // Give the slave device to the current user (mostly a no-op on modern
        // Linux, but POSIX requires it).
        assert_eq!(libc::grantpt(fd), 0);
        // A new slave is locked, it cannot be opened until this call.
        assert_eq!(libc::unlockpt(fd), 0);

        // The path of the slave, e.g. `/dev/pts/7`. The `_r` version writes into
        // our buffer instead of a static one: the tests run on several threads.
        let mut name = [0; 128];
        assert_eq!(libc::ptsname_r(fd, name.as_mut_ptr(), name.len()), 0);
        let name = CStr::from_ptr(name.as_ptr()).to_str().unwrap();

        // O_NOCTTY again: the shell takes it as its terminal (TIOCSCTTY), not us.
        let slave = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(name)
            .unwrap();

        // Turn echo off. With echo on, everything we type would also come back
        // on the master, and `expect("alive")` would match the typed
        // `echo alive` before the command ran. Line buffering (ICANON) and the
        // keys that send signals (ISIG) stay on. TCSANOW: apply immediately.
        let mut termios = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(slave.as_raw_fd(), &mut termios), 0);
        termios.c_lflag &= !libc::ECHO;
        assert_eq!(
            libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &termios),
            0
        );

        (master, slave)
    }
}
