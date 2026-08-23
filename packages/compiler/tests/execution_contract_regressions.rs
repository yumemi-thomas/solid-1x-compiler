// Integration tests link the rlib, whose Node-API registration constructors
// have no host to resolve against. The contract is core surface, so it is
// exercised in the Node-free configuration alongside the corpus census.
#![cfg(not(feature = "node"))]

//! Shapes where the census and lowering used to disagree.
//!
//! The corpus reconciliation in `execution_contract_census.rs` covers whatever
//! the Babel fixtures and parity probes happen to contain. These are the two
//! shapes they did not contain, each pinned together with the negative
//! controls that keep the fix from being a blanket relaxation:
//!
//! - a `style`/`classList` object written *before* a later spread, which
//!   lowering peels back out of the runtime props object and decomposes per
//!   declaration — the census used to record it as one opaque value because
//!   the element had a spread *somewhere*;
//! - a fragment produced inside a component prop's callback, which lowers as
//!   an ordinary JSX child — the census used to call it a component child
//!   because a mutable "nearest enclosing element is a component" flag
//!   survived the walk into the attribute value.
//!
//! Two of them also pin the emitted code. Both fixes are census-only, so the
//! generated output is the same byte-for-byte as before them; a snapshot that
//! moves means the repair went into lowering, which would be a Babel parity
//! break.

use dom_expressions_compiler::{
    compile, CallbackDecision, CompileOptions, ExecutionSiteKind, OwnershipDecision, SemanticTrace,
    TerminalDecision, ValueDecision, Wrapper,
};

fn options(semantic_trace: bool) -> CompileOptions {
    CompileOptions {
        module_name: "r-dom".into(),
        built_ins: vec!["For".into(), "Show".into(), "Switch".into(), "Match".into()],
        static_marker: "@once".into(),
        semantic_trace,
        ..CompileOptions::default()
    }
}

/// The reconciled trace, or the reconciliation failure as the panic message —
/// an unresolved or uncensused site is the regression these tests guard.
fn trace(source: &str) -> SemanticTrace {
    compile(source, &options(true))
        .unwrap_or_else(|error| panic!("{error}"))
        .semantic_trace
        .expect("tracing was requested")
}

/// Every reported site as `(source text, kind, decision)`. Comparing the
/// source text rather than raw offsets keeps the expectations readable and
/// makes a span that slides show up as a diff.
fn sites(source: &str) -> Vec<(&str, ExecutionSiteKind, TerminalDecision)> {
    sites_with_built_ins(source, vec!["For", "Show", "Switch", "Match"])
}

fn sites_with_built_ins<'a>(
    source: &'a str,
    built_ins: Vec<&str>,
) -> Vec<(&'a str, ExecutionSiteKind, TerminalDecision)> {
    compile(
        source,
        &CompileOptions {
            built_ins: built_ins.into_iter().map(str::to_owned).collect(),
            semantic_trace: true,
            ..options(true)
        },
    )
    .expect("compiles")
    .semantic_trace
    .expect("tracing was requested")
    .sites
    .into_iter()
    .map(|site| {
        (
            &source[site.span.start as usize..site.span.end as usize],
            site.kind,
            site.decision,
        )
    })
    .collect()
}

fn value(decision: ValueDecision) -> TerminalDecision {
    TerminalDecision::Value(decision)
}

/// The generated code, asserted to be identical with tracing on and off.
fn emitted(source: &str) -> String {
    let untraced = compile(source, &options(false)).expect("compiles").code;
    let traced = compile(source, &options(true)).expect("compiles").code;
    assert_eq!(untraced, traced, "tracing changed the emitted code");
    untraced
}

// ---------------------------------------------------------------------------
// A `style`/`classList` object before a later spread.
// ---------------------------------------------------------------------------

const STYLE_BEFORE_SPREAD: &str = r#"const C = (props) => (
  <input
    tabIndex={-1}
    style={{ "font-size": "16px", "line-height": LINE_HEIGHT }}
    name={props.name}
    {...props}
  />
);
"#;

#[test]
fn a_static_style_object_before_a_later_spread_decomposes_per_declaration() {
    // The literal declaration folds into the template string; the identifier
    // declaration becomes its own `setStyleProperty`. Neither the whole object
    // nor a `style` prop on the merged props object exists in the output, so
    // neither may be censused.
    assert_eq!(
        sites(STYLE_BEFORE_SPREAD),
        [
            (
                "-1",
                ExecutionSiteKind::NativeAttribute,
                value(ValueDecision::Elided)
            ),
            (
                "LINE_HEIGHT",
                ExecutionSiteKind::NativeAttribute,
                value(ValueDecision::EagerOnce),
            ),
            (
                "props.name",
                ExecutionSiteKind::NativeAttribute,
                value(ValueDecision::CallerContext),
            ),
            (
                "props",
                ExecutionSiteKind::NativeSpread,
                value(ValueDecision::EagerOnce)
            ),
        ]
    );
}

#[test]
fn a_class_list_object_before_a_later_spread_decomposes_per_property() {
    let source = r#"const C = (props) => <input classList={{ active: ACTIVE }} {...props} />;"#;
    assert_eq!(
        sites(source),
        [
            (
                "ACTIVE",
                ExecutionSiteKind::NativeAttribute,
                value(ValueDecision::EagerOnce)
            ),
            (
                "props",
                ExecutionSiteKind::NativeSpread,
                value(ValueDecision::EagerOnce)
            ),
        ]
    );
}

#[test]
fn the_same_style_object_without_a_spread_still_reconciles() {
    let source = r#"const C = (props) => (
  <input
    tabIndex={-1}
    style={{ "font-size": "16px", "line-height": LINE_HEIGHT }}
    name={props.name}
  />
);
"#;
    assert_eq!(
        sites(source),
        [
            (
                "-1",
                ExecutionSiteKind::NativeAttribute,
                value(ValueDecision::Elided)
            ),
            (
                "LINE_HEIGHT",
                ExecutionSiteKind::NativeAttribute,
                value(ValueDecision::EagerOnce),
            ),
            (
                "props.name",
                ExecutionSiteKind::NativeAttribute,
                value(ValueDecision::ReactiveRerun),
            ),
        ]
    );
}

#[test]
fn a_dynamic_style_object_before_a_spread_stays_one_runtime_site() {
    // Dynamic, so it is *not* carved out: it merges into the props object as a
    // `style` getter and stays one value the caller reads.
    let source =
        r#"const C = (props) => <input style={{ "font-size": props.size() }} {...props} />;"#;
    assert_eq!(
        sites(source),
        [
            (
                r#"{ "font-size": props.size() }"#,
                ExecutionSiteKind::NativeAttribute,
                value(ValueDecision::CallerContext),
            ),
            (
                "props",
                ExecutionSiteKind::NativeSpread,
                value(ValueDecision::EagerOnce)
            ),
        ]
    );
}

#[test]
fn a_style_object_after_a_leading_spread_stays_one_opaque_site() {
    // The spread comes first, so the object genuinely does merge into the
    // runtime props object and must keep being censused whole.
    let source = r#"const C = (props) => <input {...props} style={{ "font-size": "16px" }} />;"#;
    assert_eq!(
        sites(source),
        [
            (
                "props",
                ExecutionSiteKind::NativeSpread,
                value(ValueDecision::EagerOnce)
            ),
            (
                r#"{ "font-size": "16px" }"#,
                ExecutionSiteKind::NativeAttribute,
                value(ValueDecision::EagerOnce),
            ),
        ]
    );
}

// ---------------------------------------------------------------------------
// A fragment produced inside a component prop's callback.
// ---------------------------------------------------------------------------

const FRAGMENT_IN_A_PROP_CALLBACK: &str = r#"const C = (props) => (
  <Show
    when={!error() || !props.errorState}
    fallback={(() => {
      const err = error();
      return err ? <>{props.errorState?.({ error: err, reload })}</> : null;
    })()}
  >
    <div>ok</div>
  </Show>
);
"#;

#[test]
fn a_fragment_built_inside_a_control_flow_prop_callback_is_a_jsx_child() {
    assert_eq!(
        sites(FRAGMENT_IN_A_PROP_CALLBACK),
        [
            (
                "!error() || !props.errorState",
                ExecutionSiteKind::ComponentProperty,
                value(ValueDecision::CallerContext),
            ),
            (
                "(() => {\n      const err = error();\n      return err ? <>{props.errorState?.({ error: err, reload })}</> : null;\n    })()",
                ExecutionSiteKind::ComponentProperty,
                value(ValueDecision::CallerContext),
            ),
            (
                "props.errorState?.({ error: err, reload })",
                ExecutionSiteKind::JsxChild,
                value(ValueDecision::ReactiveRerun),
            ),
        ]
    );
}

#[test]
fn the_same_shape_under_switch_and_match_is_a_jsx_child_too() {
    let source = r#"const C = (props) => (
  <Switch fallback={(() => (props.x ? <>{props.a}</> : null))()}>
    <Match when={props.y} fallback={(() => (props.x ? <>{props.b}</> : null))()}>
      <div>ok</div>
    </Match>
  </Switch>
);
"#;
    let kinds = sites(source)
        .into_iter()
        .filter(|(text, ..)| *text == "props.a" || *text == "props.b")
        .map(|(text, kind, _)| (text, kind))
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            ("props.a", ExecutionSiteKind::JsxChild),
            ("props.b", ExecutionSiteKind::JsxChild),
        ]
    );
}

#[test]
fn a_fragment_written_as_a_direct_component_child_stays_a_component_child() {
    // The case `ComponentChild` is for: `component_children` hands this
    // fragment to `lower_fragment` with the component site kind.
    let source = r#"const C = (props) => <Show when={props.a}><>{props.b}</></Show>;"#;
    assert_eq!(
        sites(source),
        [
            (
                "props.a",
                ExecutionSiteKind::ComponentProperty,
                value(ValueDecision::CallerContext),
            ),
            (
                "props.b",
                ExecutionSiteKind::ComponentChild,
                value(ValueDecision::ReactiveRerun),
            ),
        ]
    );
}

#[test]
fn a_fragment_nested_inside_a_direct_component_child_fragment_is_one_too() {
    // `lower_fragment` recurses with the same site kind, so the inheritance
    // the removed flag provided has to survive as a span-set rule.
    let source = r#"const C = (props) => <Show when={1}><><>{props.b}</></></Show>;"#;
    assert_eq!(
        sites(source),
        [(
            "props.b",
            ExecutionSiteKind::ComponentChild,
            value(ValueDecision::ReactiveRerun),
        )]
    );
}

#[test]
fn the_callback_fragment_shape_on_a_native_element_is_a_jsx_child() {
    let source = r#"const C = (props) => <div title={(() => (props.x ? <>{props.b}</> : null))()}>ok</div>;"#;
    assert_eq!(
        sites(source)
            .into_iter()
            .map(|(text, kind, _)| (text, kind))
            .collect::<Vec<_>>(),
        [
            (
                "(() => (props.x ? <>{props.b}</> : null))()",
                ExecutionSiteKind::NativeAttribute,
            ),
            ("props.b", ExecutionSiteKind::JsxChild),
        ]
    );
}

#[test]
fn the_callback_fragment_shape_on_a_plain_component_is_a_jsx_child() {
    let source = r#"const C = (props) => <Thing fallback={(() => (props.x ? <>{props.b}</> : null))()}>ok</Thing>;"#;
    assert_eq!(
        sites(source)
            .into_iter()
            .map(|(text, kind, _)| (text, kind))
            .collect::<Vec<_>>(),
        [
            (
                "(() => (props.x ? <>{props.b}</> : null))()",
                ExecutionSiteKind::ComponentProperty,
            ),
            ("props.b", ExecutionSiteKind::JsxChild),
        ]
    );
}

// ---------------------------------------------------------------------------
// Control-flow callback operation facts.
//
// The lowering owns this classification: a function child of a configured,
// unshadowed built-in is emitted as a later render callback. A consumer can
// therefore consume `ControlFlowRender` directly instead of rebuilding the
// built-in test from its own AST.
// ---------------------------------------------------------------------------

#[test]
fn a_builtin_function_child_is_authoritatively_a_control_flow_render() {
    let source = r#"const C = () => <Show>{() => <span>{value()}</span>}</Show>;"#;
    assert_eq!(
        sites_with_built_ins(source, vec!["Show"]),
        [
            (
                "() => <span>{value()}</span>",
                ExecutionSiteKind::ControlFlowRender,
                TerminalDecision::Callback(CallbackDecision::LaterRender),
            ),
            (
                "value()",
                ExecutionSiteKind::JsxChild,
                value(ValueDecision::ReactiveRerun),
            )
        ]
    );
}

#[test]
fn an_unconfigured_or_shadowed_builtin_stays_a_component_child() {
    let unconfigured = r#"const C = () => <Show>{() => value()}</Show>;"#;
    assert_eq!(
        sites_with_built_ins(unconfigured, Vec::new()),
        [(
            "() => value()",
            ExecutionSiteKind::ComponentChild,
            value(ValueDecision::EagerOnce),
        )]
    );

    let shadowed = r#"const Show = Thing; const C = () => <Show>{() => value()}</Show>;"#;
    assert_eq!(
        sites_with_built_ins(shadowed, vec!["Show"]),
        [(
            "() => value()",
            ExecutionSiteKind::ComponentChild,
            value(ValueDecision::EagerOnce),
        )]
    );
}

// ---------------------------------------------------------------------------
// Parity: both repairs are census-only, so the emitted code did not move.
// ---------------------------------------------------------------------------

#[test]
fn the_style_repro_emits_the_code_it_emitted_before_the_census_fix() {
    assert_eq!(
        emitted(STYLE_BEFORE_SPREAD),
        r#"import { template as _$template } from "r-dom";
import { spread as _$spread } from "r-dom";
import { mergeProps as _$mergeProps } from "r-dom";
import { setStyleProperty as _$setStyleProperty } from "r-dom";
var _tmpl$ = /* @__PURE__ */ _$template(`<input tabindex=-1 style=font-size:16px>`);
const C = (props) => (() => {
	var _el$ = _tmpl$();
	_$setStyleProperty(_el$, "line-height", LINE_HEIGHT);
	_$spread(_el$, _$mergeProps({ get name() {
		return props.name;
	} }, props), false, false);
	return _el$;
})();
"#
    );
}

#[test]
fn the_fragment_repro_emits_the_code_it_emitted_before_the_census_fix() {
    assert_eq!(
        emitted(FRAGMENT_IN_A_PROP_CALLBACK),
        r#"import { template as _$template } from "r-dom";
import { memo as _$memo } from "r-dom";
import { createComponent as _$createComponent } from "r-dom";
import { Show as _$Show } from "r-dom";
var _tmpl$ = /* @__PURE__ */ _$template(`<div>ok`);
const C = (props) => _$createComponent(_$Show, {
	get when() {
		return !error() || !props.errorState;
	},
	get fallback() {
		const err = error();
		return err ? _$memo(() => {
			return props.errorState?.({
				error: err,
				reload
			});
		}) : null;
	},
	get children() {
		return _tmpl$();
	}
});
"#
    );
}

// ---------------------------------------------------------------------------
// Composition with the ownership trace.
//
// `finish()` derives `ownership_sites` from the reconciled execution sites, so
// a census that fails the file yields no ownership evidence at all — which is
// exactly what both shapes above used to do. These pin the composition: the
// fixes are what make ownership reachable for them, and they leave the
// `effect_wrapper` gate that decides whether an owner can be claimed alone.
// ---------------------------------------------------------------------------

#[test]
fn the_callback_fragment_shape_now_yields_an_owned_region() {
    let trace = trace(FRAGMENT_IN_A_PROP_CALLBACK);
    let owned = trace
        .ownership_sites
        .iter()
        .map(|site| {
            (
                &FRAGMENT_IN_A_PROP_CALLBACK[site.span.start as usize..site.span.end as usize],
                site.decision,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        owned,
        [(
            "props.errorState?.({ error: err, reload })",
            OwnershipDecision::Owned,
        )]
    );
}

#[test]
fn a_custom_effect_wrapper_still_makes_no_owner_claim() {
    let options = CompileOptions {
        effect_wrapper: Wrapper::Name("createRenderEffect".into()),
        ..options(true)
    };
    let trace = compile(FRAGMENT_IN_A_PROP_CALLBACK, &options)
        .expect("compiles")
        .semantic_trace
        .expect("tracing was requested");
    assert!(!trace.sites.is_empty());
    assert!(trace.ownership_sites.is_empty());
}

#[test]
fn the_style_shape_reconciles_without_inventing_an_owner() {
    // Every site here is `Elided` or `CallerContext`; none is a
    // `ReactiveRerun`, so no owner is proven and none is reported.
    let trace = trace(STYLE_BEFORE_SPREAD);
    assert_eq!(trace.sites.len(), 4);
    assert!(trace.ownership_sites.is_empty());
}
