//! `help` - display the help of the built-ins (regular built-in, not POSIX: like
//! bash's).
//!
//! ```text
//! help [name...]
//! ```

use clap::Parser;

use crate::builtin::{BUILTINS, Builtin, BuiltinResult, Status, help_command};

/// Display the help of the built-ins.
///
/// Without NAME, list every built-in with a short description. With NAME,
/// print the help of each built-in named, the same as NAME --help.
///
/// Exit status:
///   0   success
///   >0  a NAME is not a built-in
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "help [name...]")]
pub(super) struct Args {
    /// The built-ins to describe.
    names: Vec<String>,
}

pub fn help(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;

    if args.names.is_empty() {
        list(bi)?;
        return Ok(Status::SUCCESS);
    }

    let mut status = Status::SUCCESS;
    let mut printed = false;
    for name in &args.names {
        let Some(mut cmd) = help_command(name) else {
            bi.report(format!("{name}: no help topics match"))?;
            status = Status::FAILURE;
            continue;
        };

        // A blank line between two helps.
        if printed {
            writeln!(bi.stdout)?;
        }
        write!(bi.stdout, "{}", cmd.render_long_help())?;
        printed = true;
    }

    Ok(status)
}

/// Every built-in, by name, with the first line of its help.
fn list(bi: &mut Builtin) -> BuiltinResult {
    let mut names: Vec<&str> = BUILTINS.keys().copied().collect();
    names.sort_unstable();

    writeln!(
        bi.stdout,
        "rush built-ins. Type 'help name' for more about one of them."
    )?;
    writeln!(bi.stdout)?;

    for name in names {
        let about = help_command(name)
            .and_then(|cmd| cmd.get_about().map(ToString::to_string))
            .unwrap_or_default();
        writeln!(bi.stdout, "  {name:<10} {about}")?;
    }

    Ok(Status::SUCCESS)
}
