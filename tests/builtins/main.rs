//! The built-in utilities, one module per built-in. All of them are compiled into
//! a single test binary: `cargo test --test builtins [<builtin>::]`.

#[path = "../common/mod.rs"]
mod common;

mod bg;
mod cd;
mod colon;
mod eval;
mod exec;
mod exit;
mod r#false;
mod fg;
mod jobs;
mod not_implemented;
mod pipeline;
mod pwd;
mod redirections;
mod special;
mod r#true;
