//! `newgrp` - change to a new group (regular built-in).
//!
//! ```text
//! newgrp [-l] [group]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/newgrp.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn newgrp(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
