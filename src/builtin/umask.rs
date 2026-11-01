//! `umask` - get or set the file mode creation mask (regular built-in).
//!
//! ```text
//! umask [-S] [mask]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/umask.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn umask(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
