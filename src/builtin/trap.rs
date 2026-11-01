//! `trap` - trap signals (special built-in).
//!
//! ```text
//! trap n [condition...]
//! trap [action condition...]
//! ```
//!
//! Not implemented yet. Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn trap(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
