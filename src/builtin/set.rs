//! `set` - set or unset options and positional parameters (special built-in).
//!
//! ```text
//! set [-abCefhmnuvx] [-o option] [argument...]
//! set [+abCefhmnuvx] [+o option] [argument...]
//! set -- [argument...]
//! set -o
//! set +o
//! ```
//!
//! The positional parameters and the listings work; the options are only
//! state for now, so changing one is refused. Spec: §2.14 Special Built-In
//! Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::{
    builtin::{Builtin, BuiltinResult, Error, Status},
    shell::{self, ShellOption, Value},
};

/// Set or unset options and positional parameters.
///
/// Without arguments, print every variable that is set as name='value',
/// sorted by name, in a form the shell reads back.
///
/// With ARGUMENTs (after --, or when the first one is not an option), they
/// replace the positional parameters $1, $2...: set -- alone clears them.
///
/// -o alone prints the options and whether they are on; +o prints them as set
/// commands that restore them. -x turns option x on and +x off, -o name and
/// +o name the same by name. No option can be changed yet: they are not
/// implemented, and a set that would change one changes nothing.
///
/// Exit status:
///   0   success
///   1   an option can't be changed (not implemented yet)
///   2   an invalid option
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "set [-abCefhmnuvx] [-o option] [argument...]\n       set [+abCefhmnuvx] [+o option] [argument...]\n       set -- [argument...]\n       set -o\n       set +o"
)]
pub(super) struct Args {
    /// Options to turn on (-x, -o name) or off (+x, +o name), then the new
    /// positional parameters.
    #[arg(allow_hyphen_values = true)]
    arguments: Vec<String>,
}

/// What `set` was asked to do, parsed by hand: clap has no `+x` options.
#[derive(Debug, Default, PartialEq, Eq)]
struct Request {
    options: Vec<(ShellOption, bool)>,
    /// `-o` (`true`) or `+o` (`false`) without a name: print the options.
    listings: Vec<bool>,
    /// The new positional parameters, after `--` or the first operand.
    positional: Option<Vec<String>>,
}

pub fn set(bi: &mut Builtin) -> BuiltinResult {
    // A copy of the reference: `bi` stays free for `report`.
    let args: &[&str] = bi.args;

    if args.len() == 1 {
        for (name, var) in bi.shell.env.iter() {
            if let Value::Set(value) = &var.value {
                writeln!(bi.stdout, "{name}={}", shell::quote(value))?;
            }
        }
        return Ok(Status::SUCCESS);
    }

    let request = parse(&args[1..])?;

    // Nothing changes when an option can't be (none can yet, unless it already
    // is as asked): a script must not go on believing `-e` is on.
    let refused: Vec<String> = request
        .options
        .iter()
        .filter_map(|&(opt, on)| bi.shell.set_option(opt, on).err())
        .collect();
    if !refused.is_empty() {
        for message in refused {
            bi.report(message)?;
        }
        return Ok(Status::FAILURE);
    }

    for on in request.listings {
        print_options(bi, on)?;
    }

    if let Some(positional) = request.positional {
        bi.shell.positional = positional;
    }

    Ok(Status::SUCCESS)
}

/// The arguments of `set`, after its name.
fn parse(args: &[&str]) -> Result<Request, Error> {
    let mut request = Request::default();
    let mut args = args.iter().copied().peekable();

    while let Some(arg) = args.next() {
        // `--` and `-` end the options; the first operand starts the
        // positional parameters.
        if arg == "--" || arg == "-" {
            request.positional = Some(args.map(String::from).collect());
            break;
        }
        let Some(sign) = arg.chars().next().filter(|&c| c == '-' || c == '+') else {
            request.positional = Some(std::iter::once(arg).chain(args).map(String::from).collect());
            break;
        };

        let on = sign == '-';
        for letter in arg[1..].chars() {
            if letter != 'o' {
                let opt = ShellOption::from_letter(letter)
                    .ok_or_else(|| Error::new(2, format!("{sign}{letter}: invalid option")))?;
                request.options.push((opt, on));
                continue;
            }

            // `-o name`, or `-o` alone (followed by nothing or by an option).
            match args.next_if(|next| !next.starts_with(['-', '+'])) {
                Some(name) => {
                    let opt = ShellOption::from_name(name)
                        .ok_or_else(|| Error::new(2, format!("{name}: invalid option name")))?;
                    request.options.push((opt, on));
                }
                None => request.listings.push(on),
            }
        }
    }

    Ok(request)
}

/// `set -o`: each option and whether it is on, by name; `set +o`: the `set`
/// commands that restore them.
fn print_options(bi: &mut Builtin, human: bool) -> Result<(), Error> {
    let mut options = ShellOption::ALL;
    options.sort_by_key(|opt| opt.name());

    for opt in options {
        let on = bi.shell.options.contains(&opt);
        if human {
            let state = if on { "on" } else { "off" };
            writeln!(bi.stdout, "{:<12}{state}", opt.name())?;
        } else {
            let sign = if on { '-' } else { '+' };
            writeln!(bi.stdout, "set {sign}o {}", opt.name())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn operands_are_the_positional_parameters() {
        assert_eq!(
            parse(&["a", "-b"]).unwrap().positional,
            Some(strings(&["a", "-b"]))
        );
        assert_eq!(
            parse(&["--", "-a"]).unwrap().positional,
            Some(strings(&["-a"]))
        );
        assert_eq!(parse(&["--"]).unwrap().positional, Some(Vec::new()));
        assert_eq!(
            parse(&["-", "x"]).unwrap().positional,
            Some(strings(&["x"]))
        );
        assert_eq!(parse(&["-e"]).unwrap().positional, None);
    }

    #[test]
    fn options_by_letter_and_by_name() {
        let request = parse(&["-eu", "+x", "-o", "vi", "+o", "noglob", "--", "a"]).unwrap();
        assert_eq!(
            request.options,
            [
                (ShellOption::Errexit, true),
                (ShellOption::Nounset, true),
                (ShellOption::Xtrace, false),
                (ShellOption::Vi, true),
                (ShellOption::Noglob, false),
            ]
        );
        assert_eq!(request.positional, Some(strings(&["a"])));
    }

    #[test]
    fn o_alone_lists() {
        assert_eq!(parse(&["-o"]).unwrap().listings, [true]);
        assert_eq!(parse(&["+o", "-o"]).unwrap().listings, [false, true]);
    }

    #[test]
    fn invalid_options() {
        assert!(parse(&["-z"]).is_err());
        assert!(parse(&["+i"]).is_err());
        assert!(parse(&["-o", "nope"]).is_err());
    }
}
