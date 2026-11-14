//! `newgrp` - change to a new group (regular built-in).
//!
//! ```text
//! newgrp [-l] [group]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/newgrp.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Change to a new group.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "newgrp [-l] [group]")]
pub(super) struct Args {}

pub fn newgrp(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
