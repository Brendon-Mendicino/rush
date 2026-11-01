//! `kill` - terminate or signal processes (regular built-in).
//!
//! ```text
//! kill -s signal_name pid...
//! kill -l [exit_status]
//! kill [-signal_name] pid...
//! kill [-signal_number] pid...
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/kill.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn kill(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
