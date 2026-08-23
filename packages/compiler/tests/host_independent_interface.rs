//! Public compiler-interface coverage without the Node/N-API adapter.
//!
//! Compiled only when the `node` feature is off, so it fails if anything the
//! Rust-native surface needs has drifted back behind the adapter.
#![cfg(not(feature = "node"))]

use dom_expressions_compiler::{
    compile, CompileErrorKind, CompileOptions, ExecutionSiteKind, Generate, TerminalDecision,
    ValueDecision,
};

#[test]
fn compiles_through_the_public_rust_interface() {
    let output = compile(
        "const view = <div>{signal()}</div>;",
        &CompileOptions::default(),
    )
    .expect("compile through the public Rust interface");

    assert!(output.code.contains("template("));
    assert!(output.code.contains("insert("));
}

#[test]
fn supports_every_generate_mode_without_node_types() {
    for generate in [
        Generate::Dom,
        Generate::Ssr,
        Generate::Universal,
        Generate::Dynamic,
    ] {
        compile(
            "const view = <div />;",
            &CompileOptions {
                generate,
                ..CompileOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("{generate:?}: {error}"));
    }
}

#[test]
fn returns_owned_source_maps_and_typed_errors() {
    let output = compile(
        "const view = <div />;",
        &CompileOptions {
            source_map: true,
            ..CompileOptions::default()
        },
    )
    .expect("compile with a source map");
    assert!(output.source_map.is_some());

    let parse = compile("const view = <", &CompileOptions::default()).unwrap_err();
    assert_eq!(parse.kind(), CompileErrorKind::Parse);

    let configuration = compile(
        "const view = <div />;",
        &CompileOptions {
            module_name: String::new(),
            ..CompileOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(configuration.kind(), CompileErrorKind::Configuration);
}

/// The execution trace is the other half of the Rust-facing surface, and the
/// reason this crate is consumed as a library at all. It comes back as typed
/// data from the same call that produced the code.
#[test]
fn returns_a_typed_execution_trace_without_node_types() {
    let source = "const view = <div id={id()}>{count()}</div>;";
    let output = compile(
        source,
        &CompileOptions {
            semantic_trace: true,
            ..CompileOptions::default()
        },
    )
    .expect("compile with tracing");

    let trace = output.semantic_trace.expect("tracing was requested");
    assert_eq!(
        trace.version,
        dom_expressions_compiler::SEMANTIC_TRACE_VERSION
    );
    let decisions = trace
        .sites
        .iter()
        .map(|site| {
            (
                &source[site.span.start as usize..site.span.end as usize],
                site.kind,
                site.decision,
            )
        })
        .collect::<Vec<_>>();
    assert!(decisions.contains(&(
        "id()",
        ExecutionSiteKind::NativeAttribute,
        TerminalDecision::Value(ValueDecision::ReactiveRerun)
    )));
    assert!(decisions.contains(&(
        "count()",
        ExecutionSiteKind::JsxChild,
        TerminalDecision::Value(ValueDecision::ReactiveRerun)
    )));

    let unsupported = compile(
        "const view = <div />;",
        &CompileOptions {
            generate: Generate::Ssr,
            semantic_trace: true,
            ..CompileOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(unsupported.kind(), CompileErrorKind::Configuration);
}

/// The trace types are transport-ready, so a sidecar can define whatever
/// envelope its consumer expects without the compiler owning a wire format.
#[test]
fn the_trace_round_trips_through_serde() {
    let trace = compile(
        "const view = <div onClick={() => act()}>{count()}</div>;",
        &CompileOptions {
            semantic_trace: true,
            ..CompileOptions::default()
        },
    )
    .expect("compile with tracing")
    .semantic_trace
    .expect("tracing was requested");

    let json = serde_json::to_string(&trace).expect("serialize");
    assert!(json.contains("\"version\":2"));
    assert!(json.contains("jsx-child"));
    assert!(json.contains("later-event"));
    let parsed: dom_expressions_compiler::SemanticTrace =
        serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, trace);
}

#[test]
fn the_replaced_ownership_trace_field_is_not_silently_accepted() {
    let legacy = r#"{"sites":[],"ownership_sites":[]}"#;
    assert!(serde_json::from_str::<dom_expressions_compiler::SemanticTrace>(legacy).is_err());

    let nested_unknown = r#"{
        "version": 2,
        "sites": [{
            "span": {"start": 0, "end": 1},
            "kind": "jsx-child",
            "decision": {"value": "eager-once"},
            "extra": true
        }]
    }"#;
    assert!(
        serde_json::from_str::<dom_expressions_compiler::SemanticTrace>(nested_unknown).is_err()
    );
}
