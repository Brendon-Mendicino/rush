---
name: rust-file-layout
description: Style guide for the order of items in a Rust source file of rush - type definitions (structs, enums, traits) at the top, all impl blocks and functions at the bottom. Use whenever writing a new .rs file, adding a struct/enum/trait or impl block to an existing one, or moving code between files in this project.
---

# Rust file layout

Every `.rs` file in rush reads top-down as **what the module is**, then **how it works**:
all the type definitions first, all the implementations after them. A reader should see
every type of the module, with its fields and variants, without scrolling past method
bodies.

## Order of items

1. **Module doc** (`//!`), including the synopsis and spec link of a built-in.
2. **`mod` declarations** (`mod jobs;`, `pub mod cd;`).
3. **`use` imports**.
4. **Constants, statics and type aliases** (`const`, `static`, `type FnBuiltin = ...`).
5. **Type definitions**: every `struct`, `enum`, `union` and `trait` declaration, with its
   doc comment, `#[derive]`s and other attributes. Bodies of trait declarations
   (signatures, default methods) stay with the trait.
6. **Implementations**: every `impl` block, inherent and trait impls alike.
7. **Free functions** (`pub fn find(...)`, helpers).
8. **Tests**: `#[cfg(test)] mod tests` is always the last item.

No `impl` block or function goes between two type definitions, and no type definition goes
below the first `impl`.

## Order inside the sections

- **Types**: the module's main type first (`JobTable` in `job.rs`, `Executor` in
  `executor.rs`), then the types it is made of or hands out, roughly in the order a reader
  meets them. Public types before private helper types.
- **Impls**: in the same order as the types they implement. For each type, its inherent
  `impl Type` first, then its trait impls (`Display`, `From`, `Deref`, `Drop`, ...).
  One inherent `impl` block per type: merge split blocks instead of adding another one.
- **Free functions**: public before private.

## Example

```rust
//! `jobs` - display status of jobs in the current session (regular built-in).

use std::{collections::BTreeMap, fmt, ops::Deref};

use crate::job::Job;

const NO_JOB: &str = "no such job";

/// The jobs of the shell, by number.
pub struct JobTable {
    jobs: BTreeMap<usize, JobEntry>,
}

pub struct JobEntry {
    pub command: String,
    pub job: Job,
}

/// Shows jobs; dropping it removes the completed ones it showed.
pub struct JobReport<'t> {
    table: &'t mut JobTable,
}

impl JobTable {
    pub fn report(&mut self) -> JobReport<'_> { /* ... */ }
}

impl JobReport<'_> {
    pub fn line(&mut self, number: usize, long: bool) -> Option<String> { /* ... */ }
}

impl Deref for JobReport<'_> { /* ... */ }

impl Drop for JobReport<'_> { /* ... */ }

pub fn parse_job_id(s: &str) -> Result<JobId, String> { /* ... */ }

#[cfg(test)]
mod tests {
    use super::*;
    // ...
}
```

## Applying it

- New files follow the layout from the start.
- When adding a type to a file, put its definition after the last type definition and its
  `impl` blocks in the implementation section, at the position matching the type's.
- Many existing files still interleave types and impls (e.g. `src/job.rs`). Reordering a
  whole file is a pure move: do it as its own change (no edits mixed in, so the diff
  reviews as a move), and only when asked or when that file is already being reworked.
- Moving items never changes behavior, but check `cargo check` and `cargo fmt` afterwards.
