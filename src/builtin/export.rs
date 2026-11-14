//! `export` - set the export attribute for variables (special built-in).
//!
//! ```text
//! export name[=word]...
//! export -p
//! ```
//!
//! Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, Status, declaration, print_declarations};

/// Set the export attribute for variables.
///
/// Give the export attribute to each NAME, so that it is in the environment of
/// the commands run afterwards, and set its value to WORD when there is one.
/// A variable exported without a value stays unset, and is passed to the
/// commands once it gets one.
///
/// With -p, or without operands, print every exported variable as
/// export name='value', or export name when it is unset, sorted by name.
///
/// Exit status:
///   0   success
///   >0  a NAME is invalid, or readonly with a WORD (the next ones are skipped)
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "export name[=word]...\n       export -p"
)]
pub(super) struct Args {
    /// Print the exported variables.
    #[arg(short = 'p', conflicts_with = "names")]
    print: bool,
    /// name[=word]
    names: Vec<String>,
}

pub fn export(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;

    // Without operands the result is unspecified: like -p, as other shells do.
    if args.print || args.names.is_empty() {
        print_declarations(bi, "export", |attr| attr.exported)?;
        return Ok(Status::SUCCESS);
    }

    // The first error stops: a special built-in's error exits a script anyway.
    for arg in &args.names {
        let (name, value) = declaration(arg)?;
        bi.shell.env.export(name, value)?;
    }

    Ok(Status::SUCCESS)
}
