//! `pwd` - return working directory name (regular built-in).
//!
//! ```text
//! pwd [-L|-P]
//! ```
//!
//! Spec: `docs/utilities/pwd.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, ENDL, Status, cd};

/// Print the name of the working directory.
///
/// Print the absolute pathname of the working directory, without . or ..
/// components.
///
/// With -L (the default), print $PWD, as set by cd, when it is an absolute
/// pathname of the working directory without . or .. components; otherwise
/// behave as -P. With -P, print the pathname with every symbolic link
/// resolved. When both are given, the last one wins.
///
/// Exit status:
///   0   success
///   >0  an error occurred, nothing is written to standard output
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "pwd [-L|-P]")]
struct Args {
    /// Print $PWD if it names the working directory (default).
    #[arg(short = 'L', overrides_with = "physical")]
    logical: bool,
    /// Print the pathname without symbolic links.
    #[arg(short = 'P', overrides_with = "logical")]
    physical: bool,
}

pub fn pwd(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;

    let cwd = if args.physical {
        cd::current_dir()?
    } else {
        cd::logical_cwd(&bi.shell.env)?
    };

    write!(bi.stdout, "{cwd}{ENDL}")?;

    Ok(Status::SUCCESS)
}
