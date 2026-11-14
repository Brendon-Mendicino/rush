//! `readonly` - set the readonly attribute for variables (special built-in).
//!
//! ```text
//! readonly name[=word]...
//! readonly -p
//! ```
//!
//! Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, Status, declaration, print_declarations};

/// Set the readonly attribute for variables.
///
/// Give the readonly attribute to each NAME, after setting its value to WORD
/// when there is one. The value of a readonly variable can't be changed, by an
/// assignment or by a VAR=x prefix, and the variable can't be unset; it can
/// still be exported. A variable made readonly without a value stays unset.
///
/// With -p, or without operands, print every readonly variable as
/// readonly name='value', or readonly name when it is unset, sorted by name.
///
/// Exit status:
///   0   success
///   >0  a NAME is invalid, or readonly with a WORD (the next ones are skipped)
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "readonly name[=word]...\n       readonly -p"
)]
pub(super) struct Args {
    /// Print the readonly variables.
    #[arg(short = 'p', conflicts_with = "names")]
    print: bool,
    /// name[=word]
    names: Vec<String>,
}

pub fn readonly(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;

    // Without operands the result is unspecified: like -p, as other shells do.
    if args.print || args.names.is_empty() {
        print_declarations(bi, "readonly", |attr| attr.readonly)?;
        return Ok(Status::SUCCESS);
    }

    // The first error stops: a special built-in's error exits a script anyway.
    for arg in &args.names {
        let (name, value) = declaration(arg)?;
        bi.shell.env.set_readonly(name, value)?;
    }

    Ok(Status::SUCCESS)
}
