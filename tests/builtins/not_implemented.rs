//! The built-ins that are only stubs: they report it and fail, without stopping
//! the shell. Remove a built-in from the list once it is implemented.

use crate::common::{run_shell, stderr, stdout};

const NOT_IMPLEMENTED: [&str; 20] = [
    // Special built-ins.
    "break", "continue", ".", "return", "set", "shift", "times", "trap",
    // Regular built-ins.
    "alias", "command", "fc", "getopts", "hash", "read", "type", "ulimit", "umask", "unalias",
    "wait", // Also with arguments.
    "set -e",
];

#[test]
fn stubs_report_and_fail() {
    for command in NOT_IMPLEMENTED {
        let output = run_shell(&format!("{command} || echo failed\necho after\n"));
        let name = command.split(' ').next().unwrap();

        assert!(
            stderr(&output).contains(&format!("rush: {name}: not implemented yet")),
            "{command}: {}",
            stderr(&output)
        );
        assert_eq!(stdout(&output), "failed\nafter\n", "{command}");
    }
}
