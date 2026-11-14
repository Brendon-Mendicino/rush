//! The `export` built-in. `$X` can't be expanded yet: the variables are checked
//! with `printenv` and with the listing of `export -p`.

use crate::common::{pty::InteractiveShell, run_shell, stderr, stdout};

/// The lines of `export -p` about `names`.
fn listed(out: &str, names: &[&str]) -> Vec<String> {
    out.lines()
        .filter(|line| {
            let declaration = line.strip_prefix("export ").unwrap_or("");
            let name = declaration.split('=').next().unwrap_or("");
            names.contains(&name)
        })
        .map(str::to_string)
        .collect()
}

#[test]
fn exports_a_variable() {
    let output = run_shell("X=1\nprintenv X || echo local\nexport X\nprintenv X\n");

    assert_eq!(stdout(&output), "local\n1\n");
}

#[test]
fn sets_the_value_too() {
    let output = run_shell("export Y=2\nprintenv Y\nexport Y=\nprintenv Y\n");

    assert_eq!(stdout(&output), "2\n\n");
}

#[test]
fn without_a_value_it_stays_unset_until_assigned() {
    let output = run_shell("export Z\nprintenv Z || echo unset\nZ=3\nprintenv Z\n");

    assert_eq!(stdout(&output), "unset\n3\n");
}

#[test]
fn p_lists_the_exported_variables() {
    let output = run_shell("export Z\nexport Y=2\nL=1\nexport -p\n");
    let out = stdout(&output);

    assert_eq!(
        listed(&out, &["Y", "Z", "L"]),
        ["export Y='2'", "export Z"],
        "{out}"
    );
    // The ones from the environment too, sorted by name.
    assert!(out.contains("export HOME='"), "{out}");
    let names: Vec<_> = out.lines().collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn without_operands_it_lists_like_p() {
    let output = run_shell("export Y=2\nexport -p\necho ---\nexport\n");
    let out = stdout(&output);
    let (p, bare) = out.split_once("---\n").unwrap();

    assert_eq!(p, bare);
}

#[test]
fn p_with_operands_is_a_usage_error() {
    let output = run_shell("export -p X\necho after\n");

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
}

#[test]
fn names_and_values_can_be_utf8() {
    let output =
        run_shell("città=Röma\nexport città ñ=値\nprintenv città\nprintenv ñ\nexport -p\n");
    let out = stdout(&output);

    assert!(out.starts_with("Röma\n値\n"), "{out}");
    assert_eq!(
        listed(&out, &["città", "ñ"]),
        ["export città='Röma'", "export ñ='値'"]
    );
}

#[test]
fn a_name_with_a_symbol_is_invalid() {
    let output = run_shell("export città.x=1\necho after\n");

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("rush: export: città.x: not a valid identifier"));
}

#[test]
fn an_invalid_name_exits_the_shell() {
    let output = run_shell("export 1a\necho after\n");

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("rush: export: 1a: not a valid identifier"));
    assert_eq!(stdout(&output), "");
}

#[test]
fn stops_at_the_first_invalid_name() {
    let mut sh = InteractiveShell::spawn();

    sh.send("export A=1 1b C=3 || echo failed\n");
    sh.stderr.expect("rush: export: 1b: not a valid identifier");
    sh.stdout.expect("failed\r\n");

    sh.send("printenv A\n");
    sh.stdout.expect("1\r\n");
    sh.send("printenv C || echo unset\n");
    sh.stdout.expect("unset\r\n");
}
