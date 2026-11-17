//! The shell's own command line (`sh` in XCU):
//!
//! ```text
//! rush [-abCefhmnuvx] [-o option]... [+abCefhmnuvx] [+o option]... [command_file [argument...]]
//! rush -c [options] command_string [command_name [argument...]]
//! rush -s [options] [argument...]
//! ```

use std::path::PathBuf;

use crate::shell::ShellOption;

/// Where the shell reads its commands from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Standard input, a line at a time (also `-s`).
    Stdin,
    /// `-c command_string`.
    Command(String),
    /// A script file.
    Script(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub source: Source,
    /// `$0`.
    pub name: String,
    /// `$1`, `$2`...
    pub positional: Vec<String>,
    /// The options to turn on (`-e`) or off (`+e`), in order.
    pub options: Vec<(ShellOption, bool)>,
}

// TODO: make this more descriptive, more like clap help messages
pub const USAGE: &str = "usage: rush [-abCefhmnuvx] [-o option] [command_file [argument...]]
       rush -c [-abCefhmnuvx] [-o option] command_string [command_name [argument...]]
       rush -s [-abCefhmnuvx] [-o option] [argument...]";

impl Invocation {
    /// Parse the arguments, the program name first. The error is the message,
    /// without the `rush: ` prefix.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = args.into_iter().peekable();
        let program = args.next().unwrap_or_else(|| "rush".to_string());

        let mut command = false;
        let mut stdin = false;
        let mut options = Vec::new();

        while let Some(arg) = args.next_if(|arg| is_option(arg)) {
            // `--` and `-` end the options.
            if arg == "--" || arg == "-" {
                break;
            }

            let on = arg.starts_with('-');
            for letter in arg[1..].chars() {
                match letter {
                    'c' if on => command = true,
                    's' if on => stdin = true,
                    'i' => return Err(format!("{arg}: not implemented yet")),
                    'o' => {
                        let name = args.next().ok_or_else(|| {
                            format!("{}o: option requires an argument", &arg[..1])
                        })?;
                        let opt = ShellOption::from_name(&name)
                            .ok_or_else(|| format!("{name}: invalid option name"))?;
                        options.push((opt, on));
                    }
                    letter => {
                        let opt = ShellOption::from_letter(letter)
                            .ok_or_else(|| format!("{}{letter}: invalid option", &arg[..1]))?;
                        options.push((opt, on));
                    }
                }
            }
        }

        let mut operands: Vec<String> = args.collect();

        let (source, name) = if command {
            if operands.is_empty() {
                return Err("-c: option requires an argument".to_string());
            }
            let command = operands.remove(0);
            let name = if operands.is_empty() {
                program
            } else {
                operands.remove(0)
            };
            (Source::Command(command), name)
        } else if stdin || operands.is_empty() {
            (Source::Stdin, program)
        } else {
            let script = operands.remove(0);
            (Source::Script(PathBuf::from(&script)), script)
        };

        Ok(Self {
            source,
            name,
            positional: operands,
            options,
        })
    }
}

/// `-x`, `+x`, `--`, `-`... but not a lone `+`.
fn is_option(arg: &str) -> bool {
    arg.starts_with('-') || (arg.starts_with('+') && arg.len() > 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Invocation, String> {
        Invocation::parse(
            std::iter::once("rush")
                .chain(args.iter().copied())
                .map(String::from),
        )
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn without_operands_it_reads_stdin() {
        let inv = parse(&[]).unwrap();
        assert_eq!(inv.source, Source::Stdin);
        assert_eq!(inv.name, "rush");
        assert!(inv.positional.is_empty());
    }

    #[test]
    fn s_takes_the_operands_as_arguments() {
        let inv = parse(&["-s", "a", "b"]).unwrap();
        assert_eq!(inv.source, Source::Stdin);
        assert_eq!(inv.positional, strings(&["a", "b"]));
    }

    #[test]
    fn a_script_and_its_arguments() {
        let inv = parse(&["dir/script", "a", "-b"]).unwrap();
        assert_eq!(inv.source, Source::Script("dir/script".into()));
        assert_eq!(inv.name, "dir/script");
        assert_eq!(inv.positional, strings(&["a", "-b"]));
    }

    #[test]
    fn c_takes_a_string_then_the_name_and_the_arguments() {
        let inv = parse(&["-c", "echo $0", "name", "a"]).unwrap();
        assert_eq!(inv.source, Source::Command("echo $0".into()));
        assert_eq!(inv.name, "name");
        assert_eq!(inv.positional, strings(&["a"]));

        let inv = parse(&["-c", "echo"]).unwrap();
        assert_eq!(inv.name, "rush");

        assert!(parse(&["-c"]).is_err());
    }

    #[test]
    fn options_are_grouped_or_long() {
        let inv = parse(&["-ec", "+x", "-o", "vi", "+o", "nounset", "--", "cmd", "-n"]).unwrap();
        assert_eq!(inv.source, Source::Command("cmd".into()));
        assert_eq!(inv.name, "-n");
        assert_eq!(
            inv.options,
            [
                (ShellOption::Errexit, true),
                (ShellOption::Xtrace, false),
                (ShellOption::Vi, true),
                (ShellOption::Nounset, false),
            ]
        );
    }

    #[test]
    fn bad_options() {
        assert_eq!(parse(&["-z"]).unwrap_err(), "-z: invalid option");
        assert_eq!(parse(&["+z"]).unwrap_err(), "+z: invalid option");
        assert_eq!(
            parse(&["-o"]).unwrap_err(),
            "-o: option requires an argument"
        );
        assert_eq!(
            parse(&["-o", "nope"]).unwrap_err(),
            "nope: invalid option name"
        );
        assert!(parse(&["-i"]).is_err());
        // `+c` is not `-c`.
        assert!(parse(&["+c", "x"]).is_err());
    }
}
