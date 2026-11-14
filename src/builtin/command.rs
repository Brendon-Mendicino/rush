//! `command` - execute a simple command (regular built-in).
//!
//! ```text
//! command [-p] command_name [argument...]
//! command [-p][-v|-V] command_name
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/command.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Execute a simple command.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "command [-p] command_name [argument...]\n       command [-p][-v|-V] command_name"
)]
pub(super) struct Args {}

pub fn command(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
