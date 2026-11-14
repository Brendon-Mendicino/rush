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

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Terminate or signal processes.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "kill -s signal_name pid...\n       kill -l [exit_status]\n       kill [-signal_name] pid...\n       kill [-signal_number] pid..."
)]
pub(super) struct Args {}

pub fn kill(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
