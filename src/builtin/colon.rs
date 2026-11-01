//! `:` - null utility (special built-in).
//!
//! ```text
//! : [argument...]
//! ```
//!
//! Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use crate::builtin::{Builtin, BuiltinResult, Status};

/// Do nothing and exit with a zero status. Its arguments are only expanded, by the
/// shell, and its redirections performed: `: >file` creates or truncates `file`.
/// There are no options, `--` is not special either.
pub fn colon(_bi: &mut Builtin) -> BuiltinResult {
    Ok(Status::SUCCESS)
}
