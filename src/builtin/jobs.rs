//! `jobs` - display status of jobs in the current session (regular built-in).
//!
//! ```text
//! jobs [-l|-p] [job_id...]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/jobs.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn jobs(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
