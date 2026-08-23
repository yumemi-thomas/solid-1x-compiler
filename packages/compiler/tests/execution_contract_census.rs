// Integration tests link the rlib, whose Node-API registration constructors
// have no host to resolve against. The contract is core surface, so it is
// exercised in the Node-free configuration alongside the interface test.
#![cfg(not(feature = "node"))]

//! Corpus-wide reconciliation of the execution contract.
//!
//! Compiling with `CompileOptions::semantic_trace` fails closed when the
//! independent source census and the decisions recorded by lowering disagree —
//! an unresolved site, a conflicting decision, or a decision aimed at a site
//! the census never enumerated. Running it over every Babel fixture the parity
//! suite
//! compares against turns that reconciliation into corpus-wide coverage: any
//! lowering path that forgets to report, or reports something the census
//! doesn't recognize, fails here rather than shipping a stale contract.

use std::path::{Path, PathBuf};

use dom_expressions_compiler::{compile, CompileOptions};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../babel-plugin-jsx-dom-expressions/test")
        .canonicalize()
        .expect("the Babel fixture corpus is a workspace sibling")
}

/// Every fixture source in the corpus, from all mode directories. The
/// cross-mode parity suite already compiles this union through every generate,
/// so each source is valid DOM input.
fn fixture_sources() -> Vec<(String, String)> {
    let mut sources = Vec::new();
    let root = fixture_root();
    let mut dirs = std::fs::read_dir(&root)
        .expect("fixture root is readable")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("__").then(|| (name, entry.path()))
        })
        .collect::<Vec<_>>();
    dirs.sort();
    for (dir_name, dir) in dirs {
        let mut fixtures = std::fs::read_dir(&dir)
            .expect("fixture directory is readable")
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let code = entry.path().join("code.js");
                code.exists().then(|| {
                    (
                        format!("{dir_name}/{}", entry.file_name().to_string_lossy()),
                        code,
                    )
                })
            })
            .collect::<Vec<_>>();
        fixtures.sort();
        for (id, path) in fixtures {
            sources.push((id, std::fs::read_to_string(path).expect("fixture is utf-8")));
        }
    }
    sources
}

/// The adversarial probe corpus, read out of the parity suite so the two stay
/// in step. Each case is a `  "name": `backtick source`,` entry; the count
/// assertion below fails loudly if that shape ever changes, rather than
/// silently reconciling nothing.
fn probe_sources() -> Vec<(String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("__tests__/parity-probes.test.js");
    let text = std::fs::read_to_string(path).expect("the probe corpus is readable");
    let body = {
        let start = text
            .find("const cases = {")
            .expect("probe corpus has cases");
        let end = text
            .find("const expectedDir")
            .expect("probe corpus has an end");
        &text[start..end]
    };

    let mut cases = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find("\n  \"") {
        let after = &rest[open + 4..];
        let Some(name_end) = after.find("\": `") else {
            rest = after;
            continue;
        };
        let name = &after[..name_end];
        let source_start = &after[name_end + 4..];
        // The closing backtick is the first unescaped one: several probes
        // embed template literals, and stopping at `\`` truncates the source
        // into something neither compiler can parse — which would silently
        // drop the probe from the reconciliation instead of failing.
        let mut source_end = None;
        let bytes = source_start.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' => index += 2,
                b'`' => {
                    source_end = Some(index);
                    break;
                }
                _ => index += 1,
            }
        }
        let Some(source_end) = source_end else {
            break;
        };
        let source = source_start[..source_end]
            .replace("\\`", "`")
            .replace("\\${", "${");
        cases.push((name.to_string(), source));
        rest = &source_start[source_end..];
    }
    cases
}

fn options(built_ins: Vec<String>) -> CompileOptions {
    CompileOptions {
        module_name: "r-dom".into(),
        built_ins,
        static_marker: "@once".into(),
        semantic_trace: true,
        ..CompileOptions::default()
    }
}

#[test]
fn every_fixture_reconciles_census_against_lowering() {
    let sources = fixture_sources();
    assert!(
        sources.len() > 50,
        "expected the full fixture corpus, found {}",
        sources.len()
    );

    let mut failures = Vec::new();
    let mut reconciled = 0;
    for (id, source) in &sources {
        match compile(source, &options(vec!["For".into(), "Show".into()])) {
            Ok(output) => {
                assert!(
                    output.semantic_trace.is_some(),
                    "{id}: tracing was requested but no trace came back"
                );
                reconciled += 1;
            }
            Err(error) => {
                let message = error.to_string();
                // Oxc rejects a handful of fixture inputs outright (the parity
                // harness carves out the same ones). Only reconciliation
                // failures are this test's business.
                if message.contains("semantic ") {
                    failures.push(format!("{id}: {message}"));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} fixtures failed contract reconciliation:\n{}",
        failures.len(),
        sources.len(),
        failures.join("\n")
    );
    assert!(
        reconciled > 50,
        "expected most fixtures to produce a contract, got {reconciled}"
    );
}

#[test]
fn every_parity_probe_reconciles_census_against_lowering() {
    let sources = probe_sources();
    assert!(
        sources.len() > 400,
        "expected the full probe corpus, extracted {}",
        sources.len()
    );
    // Guards the escape handling above: a truncated source parses as garbage
    // and would be skipped rather than reconciled.
    assert!(
        sources.iter().any(|(_, source)| source.contains('`')),
        "template-literal probes were truncated during extraction"
    );

    let mut failures = Vec::new();
    let mut reconciled = 0;
    for (name, source) in &sources {
        match compile(source, &options(vec!["For".into(), "Show".into()])) {
            Ok(output) => {
                assert!(
                    output.semantic_trace.is_some(),
                    "{name}: tracing was requested but no trace came back"
                );
                reconciled += 1;
            }
            Err(error) => {
                let message = error.to_string();
                if message.contains("semantic ") {
                    failures.push(format!("{name}: {message}"));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} probes failed contract reconciliation:\n{}",
        failures.len(),
        sources.len(),
        failures.join("\n")
    );
    assert!(
        reconciled > 400,
        "expected most probes to produce a contract, got {reconciled}"
    );
}

/// Reporting must not change what the compiler emits: the same source compiles
/// byte-identically with tracing on and off, so enabling the contract can
/// never be the reason output moved.
#[test]
fn tracing_does_not_change_generated_output() {
    let built_ins = || vec!["For".into(), "Show".into()];
    for (id, source) in fixture_sources() {
        let untraced = CompileOptions {
            semantic_trace: false,
            ..options(built_ins())
        };
        let Ok(plain) = compile(&source, &untraced) else {
            continue;
        };
        let traced = compile(&source, &options(built_ins()))
            .expect("a fixture that compiled once compiles again");
        assert_eq!(plain.code, traced.code, "output changed for {id}");
        assert!(plain.semantic_trace.is_none());
        assert!(traced.semantic_trace.is_some());
    }

    for (name, source) in probe_sources() {
        let untraced = CompileOptions {
            semantic_trace: false,
            ..options(built_ins())
        };
        let Ok(plain) = compile(&source, &untraced) else {
            continue;
        };
        let traced = compile(&source, &options(built_ins()))
            .unwrap_or_else(|error| panic!("{name}: tracing failed: {error}"));
        assert_eq!(plain.code, traced.code, "output changed for probe {name}");
        assert!(plain.semantic_trace.is_none());
        assert!(traced.semantic_trace.is_some());
    }
}
