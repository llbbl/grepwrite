use assert_cmd::Command as AssertCommand;
use std::fs;
use std::process::Command;

fn ast_grep_available() -> bool {
    Command::new("ast-grep")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Sample with two `TODO` identifier matches — one inside a `function_declaration`,
/// one at the top level. ast-grep treats `TODO` as an identifier, so we use
/// `TODO()` call expressions rather than `// TODO` comments (comments don't
/// parse as identifiers).
const SAMPLE_TS: &str =
    "function foo() {\n  TODO();\n  console.log(\"hi\");\n}\nTODO();\nconst x = 1;\n";

#[test]
fn find_in_function_filters_to_function_body() {
    if !ast_grep_available() {
        eprintln!("skipping: `ast-grep` not on PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("sample.ts");
    fs::write(&file, SAMPLE_TS).unwrap();

    let output = AssertCommand::cargo_bin("gw")
        .unwrap()
        .args([
            "find",
            "TODO",
            dir.path().to_str().unwrap(),
            "--in",
            "function",
            "-t",
            "ts",
            "-o",
            "caveman",
        ])
        .output()
        .expect("run gw");

    assert!(
        output.status.success(),
        "gw find failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "expected exactly 1 match inside function, got: {stdout}"
    );
    // The TODO inside `function foo() {{ TODO(); ... }}` is on line 2.
    assert!(
        lines[0].ends_with(":2"),
        "expected match on line 2, got: {}",
        lines[0]
    );
}

#[test]
fn find_without_in_scope_uses_rg_and_returns_both() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("sample.ts");
    fs::write(&file, SAMPLE_TS).unwrap();

    let output = AssertCommand::cargo_bin("gw")
        .unwrap()
        .args([
            "find",
            "TODO",
            dir.path().to_str().unwrap(),
            "-o",
            "caveman",
        ])
        .output()
        .expect("run gw");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        2,
        "expected both TODO occurrences without --in, got: {stdout}"
    );
}

#[test]
fn find_in_scope_without_type_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("sample.ts");
    fs::write(&file, SAMPLE_TS).unwrap();

    let output = AssertCommand::cargo_bin("gw")
        .unwrap()
        .args([
            "find",
            "TODO",
            dir.path().to_str().unwrap(),
            "--in",
            "function",
        ])
        .output()
        .expect("run gw");

    // GwError::Engine -> exit code 3.
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--type"),
        "expected stderr to mention --type, got: {stderr}"
    );
}
