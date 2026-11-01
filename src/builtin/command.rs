//! `command` - execute a simple command (regular built-in).
//!
//! ```text
//! command [-p] command_name [argument...]
//! command [-p][-v|-V] command_name
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/command.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn command(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
