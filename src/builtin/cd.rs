//! `cd`, following the steps of the DESCRIPTION in
//! `docs/utilities/cd.html` (POSIX.1-2017).

use std::{env, fs, io, os::unix::fs::MetadataExt, path::Path};

use clap::Parser;

use crate::{
    builtin::{Builtin, BuiltinResult, Context, Error, Status},
    nix,
    shell::ShellEnv,
};

/// Change the working directory.
///
/// Change the working directory of the shell to DIRECTORY. Without DIRECTORY,
/// go to $HOME. With -, go to $OLDPWD and print the new directory, the same as
/// cd "$OLDPWD" && pwd.
///
/// A relative DIRECTORY that does not start with . or .. is first searched for
/// in the colon-separated directories of $CDPATH, where an empty entry stands
/// for the current directory. When it is found through a non-empty entry, the
/// new directory is printed.
///
/// With -L (the default), .. is handled logically: DIRECTORY is taken relative
/// to $PWD and each .. removes the component before it, so cd link; cd .. goes
/// back to where it started. With -P, symbolic links are resolved first and ..
/// goes to the parent of the real directory. When both are given, the last one
/// wins.
///
/// On success $PWD is set to the new directory and $OLDPWD to the previous one,
/// both exported.
///
/// Exit status:
///   0   the directory was changed
///   >0  an error occurred, the working directory is unchanged
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "cd [-L|-P] [directory]\n       cd -"
)]
pub(super) struct Args {
    /// Handle `..` logically, before resolving symbolic links (default).
    #[arg(short = 'L', overrides_with = "physical")]
    logical: bool,
    /// Handle `..` physically, after resolving symbolic links.
    #[arg(short = 'P', overrides_with = "logical")]
    physical: bool,
    /// The new working directory, `-` for `$OLDPWD`.
    directory: Option<String>,
}

pub fn cd(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;

    if let Some(new_dir) = change_dir(&args, &mut bi.shell.env)? {
        writeln!(bi.stdout, "{new_dir}")?;
    }

    Ok(Status::SUCCESS)
}

/// Change directory and update `PWD` and `OLDPWD`. Returns the new directory when
/// it must be printed (`cd -`, or a directory found through a non-empty `CDPATH`
/// entry). On error the working directory is unchanged.
fn change_dir(args: &Args, env: &mut ShellEnv) -> Result<Option<String>, Error> {
    // Steps 1-2.
    let (directory, mut print) = operand(args, env)?;

    // Where `cd` starts from. When it cannot be determined, `..` can only be
    // handled physically.
    let old_pwd = logical_cwd(env).ok();

    let (curpath, from_cdpath) = compute_curpath(&directory, env);
    print |= from_cdpath;

    // Steps 7-8: logically, the path is resolved against `PWD` and `..` removes the
    // previous component, instead of going to the parent of where a symbolic link
    // points to.
    let logical = match (&old_pwd, args.physical) {
        (Some(pwd), false) => {
            let path = canonicalize(&join(pwd, &curpath))
                .map_err(|message| Error::from(format!("{directory}: {message}")))?;
            Some(path)
        }
        _ => None,
    };

    // Steps 9-10.
    let chdir_path = match (&logical, &old_pwd) {
        (Some(path), Some(pwd)) => shorten(path, pwd, &directory),
        _ => &curpath,
    };

    env::set_current_dir(chdir_path).context(&directory)?;

    // Physically, `PWD` is what `pwd -P` prints. When even that fails its value is
    // unspecified: it is left as it is.
    let new_pwd = logical.or_else(|| current_dir().ok());

    // Exported. A readonly one is an error, after the directory changed.
    if let Some(old_pwd) = old_pwd {
        env.export("OLDPWD", Some(old_pwd))
            .map_err(|e| Error::from(e.to_string()))?;
    }
    if let Some(new_pwd) = &new_pwd {
        env.export("PWD", Some(new_pwd.clone()))
            .map_err(|e| Error::from(e.to_string()))?;
    }

    Ok(print.then(|| new_pwd.unwrap_or(curpath)))
}

/// The directory operand, `$HOME` without one and `$OLDPWD` for `-`, and whether
/// the new directory must be printed.
fn operand(args: &Args, env: &ShellEnv) -> Result<(String, bool), Error> {
    match args.directory.as_deref() {
        // Implementation-defined without `HOME`: an error, like other shells.
        None => match env.get("HOME") {
            Some(home) if !home.is_empty() => Ok((home.to_owned(), false)),
            _ => Err("HOME not set".into()),
        },
        // `cd -` is `cd "$OLDPWD" && pwd`.
        Some("-") => match env.get("OLDPWD") {
            Some(old) if !old.is_empty() => Ok((old.to_owned(), true)),
            _ => Err("OLDPWD not set".into()),
        },
        Some(directory) => Ok((directory.to_owned(), false)),
    }
}

// Steps 3-6: absolute paths and paths starting with `.` or `..` are taken as
// they are, the others are looked up in `CDPATH`.
fn compute_curpath(directory: &str, env: &ShellEnv) -> (String, bool) {
    if directory.starts_with('/') {
        return (directory.to_string(), false);
    }

    if starts_with_dot(directory) {
        return (directory.to_string(), false);
    }

    if let Some(found) = search_cdpath(directory, env) {
        return found;
    }

    (directory.to_string(), false)
}

/// The first component of the path is `.` or `..`.
fn starts_with_dot(path: &str) -> bool {
    matches!(path.split('/').next(), Some(".") | Some(".."))
}

/// Step 5: the first directory of `CDPATH` that contains `directory`, and whether
/// it came from a non-empty entry. An empty entry, or an unset `CDPATH`, stands for
/// the current directory.
fn search_cdpath(directory: &str, env: &ShellEnv) -> Option<(String, bool)> {
    let cdpath = env.get("CDPATH").unwrap_or("");

    cdpath.split(':').find_map(|prefix| {
        let candidate = match prefix {
            "" => format!("./{directory}"),
            _ => join(prefix, directory),
        };

        Path::new(&candidate)
            .is_dir()
            .then_some((candidate, !prefix.is_empty()))
    })
}

/// `path` relative to `base`, unless it is already absolute.
fn join(base: &str, path: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else if base.ends_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

/// Step 8 on an absolute path: drop the `.` components and remove each `..`
/// together with the component before it, which must be a directory. Repeated and
/// trailing slashes are dropped too, except for a leading `//`, which may have a
/// meaning of its own. Returns the error message for a `..` that follows something
/// that is not a directory.
fn canonicalize(path: &str) -> Result<String, String> {
    let root = if path.starts_with("//") && !path.starts_with("///") {
        "//"
    } else {
        "/"
    };

    let mut components: Vec<&str> = Vec::new();

    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                // `..` of the root is the root itself.
                if components.is_empty() {
                    continue;
                }

                let preceding = format!("{root}{}", components.join("/"));
                match fs::metadata(&preceding) {
                    Ok(meta) if meta.is_dir() => {}
                    Ok(_) => {
                        let err = io::Error::from_raw_os_error(libc::ENOTDIR);
                        return Err(nix::error_message(&err));
                    }
                    Err(e) => return Err(nix::error_message(&e)),
                }

                components.pop();
            }
            _ => components.push(component),
        }
    }

    Ok(format!("{root}{}", components.join("/")))
}

/// Step 9: a path too long for `chdir` is made relative to the current directory,
/// when it is inside it.
fn shorten<'a>(curpath: &'a str, pwd: &str, operand: &str) -> &'a str {
    let path_max = libc::PATH_MAX as usize;

    // The lengths are compared with the terminating null.
    if curpath.len() < path_max || operand.len() >= path_max {
        return curpath;
    }

    curpath
        .strip_prefix(pwd.trim_end_matches('/'))
        .and_then(|rest| rest.strip_prefix('/'))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(curpath)
}

/// The current directory as the shell knows it: `PWD` when it can be trusted,
/// otherwise the physical one. This is also `pwd -L`.
pub(super) fn logical_cwd(env: &ShellEnv) -> io::Result<String> {
    match env.get("PWD") {
        Some(pwd) if names_cwd(pwd) => Ok(pwd.to_owned()),
        _ => current_dir(),
    }
}

/// `PWD` is an absolute path without `.` or `..` that names the current directory.
/// One inherited from the environment can be stale: the parent may have started
/// the shell in another directory.
fn names_cwd(pwd: &str) -> bool {
    if !pwd.starts_with('/') || pwd.split('/').any(|c| c == "." || c == "..") {
        return false;
    }

    match (fs::metadata(pwd), fs::metadata(".")) {
        (Ok(pwd), Ok(cwd)) => pwd.dev() == cwd.dev() && pwd.ino() == cwd.ino(),
        _ => false,
    }
}

/// The physical current directory, without symbolic links: `pwd -P`.
pub(super) fn current_dir() -> io::Result<String> {
    env::current_dir()?
        .into_os_string()
        .into_string()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "path is not valid UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalize_drops_dots_and_slashes() {
        // Without `..` the components are not looked up: they need not exist.
        assert_eq!(canonicalize("/a/./b//c/").unwrap(), "/a/b/c");
        assert_eq!(canonicalize("/").unwrap(), "/");
        assert_eq!(canonicalize("/./.").unwrap(), "/");
    }

    #[test]
    fn canonicalize_keeps_two_leading_slashes() {
        assert_eq!(canonicalize("//a").unwrap(), "//a");
        assert_eq!(canonicalize("///a").unwrap(), "/a");
    }

    #[test]
    fn canonicalize_folds_dot_dot() {
        assert_eq!(canonicalize("/tmp/../a").unwrap(), "/a");
        assert_eq!(canonicalize("/tmp/./../tmp/..").unwrap(), "/");
        assert_eq!(canonicalize("/..").unwrap(), "/");
        assert_eq!(canonicalize("/../../a").unwrap(), "/a");
    }

    #[test]
    fn canonicalize_needs_a_directory_before_dot_dot() {
        assert_eq!(
            canonicalize("/rush-nonexistent/../tmp"),
            Err("No such file or directory".to_owned())
        );
    }

    #[test]
    fn join_paths() {
        assert_eq!(join("/a", "b"), "/a/b");
        assert_eq!(join("/a/", "b"), "/a/b");
        assert_eq!(join("/a", "/b"), "/b");
        assert_eq!(join("/", "b"), "/b");
    }

    #[test]
    fn dot_prefixes() {
        assert!(starts_with_dot("."));
        assert!(starts_with_dot(".."));
        assert!(starts_with_dot("./a"));
        assert!(starts_with_dot("../a"));
        assert!(!starts_with_dot(".a"));
        assert!(!starts_with_dot("a/.."));
        assert!(!starts_with_dot(""));
    }

    #[test]
    fn short_paths_are_not_shortened() {
        assert_eq!(shorten("/a/b", "/a", "b"), "/a/b");
    }

    #[test]
    fn long_paths_become_relative_to_pwd() {
        let pwd = format!("/{}", "d".repeat(libc::PATH_MAX as usize));
        let curpath = format!("{pwd}/sub");

        assert_eq!(shorten(&curpath, &pwd, "sub"), "sub");
        assert_eq!(shorten(&curpath, "/other", "sub"), curpath);
    }
}
