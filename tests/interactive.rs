//! End-to-end tests of interactive mode: the shell runs on a pseudo-terminal as
//! the leader of its own session, like in a terminal emulator. Its stdout is
//! the pty (with echo turned off, so the output holds only what the commands
//! print) and its stderr a pipe, so the syntax tree dumps stay out of the way.

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

const TIMEOUT: Duration = Duration::from_secs(5);
const PROMPT: &str = "$ \x1b[0m";
const RED: &str = "\x1b[31m";

/// Output of a stream, collected by a background thread.
struct Captured {
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
            // Reading the pty master fails with EIO once the shell is gone.
            while let Ok(n @ 1..) = source.read(&mut buf) {
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });

        Self { data, seen: 0 }
    }

    /// Wait for `needle` in the output not seen yet and return the output up to
    /// the end of it.
    fn expect(&mut self, needle: &str) -> String {
        let deadline = Instant::now() + TIMEOUT;

        loop {
            let unseen = String::from_utf8_lossy(&self.data.lock().unwrap()[self.seen..]).into_owned();

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

struct InteractiveShell {
    child: Child,
    master: File,
    stdout: Captured,
    stderr: Captured,
}

impl InteractiveShell {
    fn spawn() -> Self {
        let (master, slave) = open_pty();

        let mut command = Command::new(env!("CARGO_BIN_EXE_iron-shell"));
        command
            .stdin(slave.try_clone().unwrap())
            .stdout(slave)
            .stderr(Stdio::piped());

        // A new session with the pty as its controlling terminal.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let mut child = command.spawn().expect("failed to start the shell");
        // Dropping `command` closed our copies of the slave.
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

    fn pid(&self) -> i32 {
        self.child.id() as i32
    }

    fn send(&mut self, input: &str) {
        self.master.write_all(input.as_bytes()).unwrap();
    }

    /// Wait for the next prompt, return what was printed before it.
    fn expect_prompt(&mut self) -> String {
        self.stdout.expect(PROMPT)
    }

    /// Process group owning the terminal.
    fn foreground_pgid(&self) -> i32 {
        let pgid = unsafe { libc::tcgetpgrp(self.master.as_raw_fd()) };
        assert!(pgid > 0, "tcgetpgrp: {}", io::Error::last_os_error());
        pgid
    }

    /// Wait until a job owns the terminal and its leader exec'd `program`
    /// (from then on it has the default signal handling). Returns its pgid.
    fn wait_foreground_job(&self, program: &str) -> i32 {
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

impl Drop for InteractiveShell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Open a pty pair, with echo turned off.
fn open_pty() -> (File, File) {
    unsafe {
        let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        assert!(fd >= 0, "posix_openpt: {}", io::Error::last_os_error());
        let master = File::from_raw_fd(fd);

        assert_eq!(libc::grantpt(fd), 0);
        assert_eq!(libc::unlockpt(fd), 0);

        let mut name = [0; 128];
        assert_eq!(libc::ptsname_r(fd, name.as_mut_ptr(), name.len()), 0);
        let name = CStr::from_ptr(name.as_ptr()).to_str().unwrap();

        let slave = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(name)
            .unwrap();

        let mut termios = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(slave.as_raw_fd(), &mut termios), 0);
        termios.c_lflag &= !libc::ECHO;
        assert_eq!(libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &termios), 0);

        (master, slave)
    }
}

/// `pid`, `pgrp` and `tpgid` (foreground group of the terminal) from a line of
/// `/proc/<pid>/stat`.
fn stat_ids(output: &str) -> (i32, i32, i32) {
    let line = output.lines().find(|l| l.contains(" (cat) ")).unwrap();
    let (pid, rest) = line.split_once(" (cat) ").unwrap();
    // state ppid pgrp session tty_nr tpgid
    let fields: Vec<_> = rest.split_whitespace().collect();

    (
        pid.parse().unwrap(),
        fields[2].parse().unwrap(),
        fields[5].parse().unwrap(),
    )
}

/// The `SigIgn` mask from a `/proc/<pid>/status` file.
fn ignored_signals(status: &str) -> u64 {
    let line = status.lines().find(|l| l.starts_with("SigIgn:")).unwrap();
    u64::from_str_radix(line["SigIgn:".len()..].trim(), 16).unwrap()
}

fn job_control_mask() -> u64 {
    [
        libc::SIGINT,
        libc::SIGQUIT,
        libc::SIGTSTP,
        libc::SIGTTIN,
        libc::SIGTTOU,
    ]
    .iter()
    .fold(0, |mask, sig| mask | 1 << (sig - 1))
}

#[test]
fn foreground_job_gets_its_own_group_and_the_terminal() {
    let mut sh = InteractiveShell::spawn();

    sh.send("cat /proc/self/stat\n");
    let (pid, pgrp, tpgid) = stat_ids(&sh.expect_prompt());
    assert_eq!(pgrp, pid, "a single command leads its group");
    assert_ne!(pgrp, sh.pid(), "the job is not in the shell's group");
    assert_eq!(tpgid, pgrp, "the job owns the terminal");

    sh.send("true | cat /proc/self/stat\n");
    let (pid, pgrp, tpgid) = stat_ids(&sh.expect_prompt());
    assert_ne!(pgrp, pid, "the first command of the pipeline leads the group");
    assert_ne!(pgrp, sh.pid());
    assert_eq!(tpgid, pgrp);

    assert_eq!(sh.foreground_pgid(), sh.pid(), "the shell takes the terminal back");
}

#[test]
fn foreground_job_can_read_the_terminal() {
    let mut sh = InteractiveShell::spawn();

    sh.send("cat\n");
    sh.wait_foreground_job("cat");
    sh.send("typed\n");
    sh.stdout.expect("typed\r\n");
    // End of input for cat.
    sh.send("\x04");

    assert!(!sh.expect_prompt().contains(RED), "cat succeeded");
}

#[test]
fn children_restore_the_default_signal_handling() {
    let mut sh = InteractiveShell::spawn();
    let mask = job_control_mask();

    let shell_status = fs::read_to_string(format!("/proc/{}/status", sh.pid())).unwrap();
    assert_eq!(ignored_signals(&shell_status) & mask, mask, "the shell ignores them");

    sh.send("grep SigIgn /proc/self/status\n");
    assert_eq!(ignored_signals(&sh.expect_prompt()) & mask, 0);

    sh.send("grep SigIgn /proc/self/status | cat\n");
    assert_eq!(ignored_signals(&sh.expect_prompt()) & mask, 0);
}

#[test]
fn ctrl_c_interrupts_the_foreground_job() {
    let mut sh = InteractiveShell::spawn();

    sh.send("sleep 30\n");
    sh.wait_foreground_job("sleep");
    sh.send("\x03");

    sh.stderr.expect("Terminated by signal SIGINT");
    assert!(sh.expect_prompt().contains(RED), "the status is 130");

    sh.send("echo alive\n");
    sh.stdout.expect("alive\r\n");
}

#[test]
fn ctrl_z_stops_the_foreground_job() {
    let mut sh = InteractiveShell::spawn();

    sh.send("sleep 30\n");
    let pgid = sh.wait_foreground_job("sleep");
    sh.send("\x1a");

    sh.stderr.expect("stopped by SIGTSTP");
    sh.expect_prompt();
    assert_eq!(sh.foreground_pgid(), sh.pid(), "the shell takes the terminal back");

    let stat = fs::read_to_string(format!("/proc/{pgid}/stat")).unwrap();
    assert!(stat.contains(") T "), "sleep is stopped: {stat}");

    sh.send("echo alive\n");
    sh.stdout.expect("alive\r\n");

    unsafe { libc::kill(-pgid, libc::SIGKILL) };
}

#[test]
fn missing_command_fails_without_killing_the_shell() {
    let mut sh = InteractiveShell::spawn();

    sh.send("nonexistent-rush-cmd\n");
    sh.stderr.expect("nonexistent-rush-cmd: command not found");
    assert!(sh.expect_prompt().contains(RED));
    assert_eq!(sh.foreground_pgid(), sh.pid());

    sh.send("echo alive\n");
    sh.stdout.expect("alive\r\n");
}
