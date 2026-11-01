//! `fc` - process the command history list (regular built-in).
//!
//! ```text
//! fc [-r] [-e editor] [first [last]]
//! fc -l [-nr] [first [last]]
//! fc -s [old=new] [first]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/fc.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn fc(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
