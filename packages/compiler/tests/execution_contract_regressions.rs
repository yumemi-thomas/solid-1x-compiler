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
    compile, CallbackDecision, CompileOptions, ExecutionSiteKind, SemanticTrace, TerminalDecision,
    ValueDecision, Wrapper,
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
fn a_builtin_function_expression_child_is_also_a_control_flow_render() {
    let source = r#"const C = () => <Show>{function () { return value(); }}</Show>;"#;
    assert!(sites_with_built_ins(source, vec!["Show"]).contains(&(
        "function () { return value(); }",
        ExecutionSiteKind::ControlFlowRender,
        TerminalDecision::Callback(CallbackDecision::LaterRender),
    )));
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
// Composition with the owner-establishment trace.
//
// Owner facts are recorded by the lowering wrappers themselves. They do not
// claim what the runtime wrapper means; the consumer maps the identity through
// its audited dialect.
// ---------------------------------------------------------------------------

#[test]
fn the_callback_fragment_shape_records_the_memo_wrapper_at_its_span() {
    let trace = trace(FRAGMENT_IN_A_PROP_CALLBACK);
    assert!(trace.owner_establishments.iter().any(|site| {
        site.wrapper == "memo"
            && &FRAGMENT_IN_A_PROP_CALLBACK[site.span.start as usize..site.span.end as usize]
                == "props.errorState?.({ error: err, reload })"
    }));
}

/// Every memo fact as `(start, end, source text)`, in report order.
fn memos(source: &str) -> Vec<(u32, u32, &str)> {
    trace(source)
        .owner_establishments
        .into_iter()
        .filter(|site| site.wrapper == "memo")
        .map(|site| {
            (
                site.span.start,
                site.span.end,
                &source[site.span.start as usize..site.span.end as usize],
            )
        })
        .collect::<Vec<_>>()
}

/// The emitted `memo(...)` calls, which every memo fact must correspond to
/// one-for-one.
fn emitted_memo_count(source: &str) -> usize {
    emitted(source).matches("_$memo(").count()
}

#[test]
fn component_child_condition_memo_spans_the_booleanized_test() {
    // The memo wraps `!!value()`; `left()` and `right()` run in the child
    // getter's scope, not inside the memo, so the fact covers the test alone.
    let source = "const C = () => <Show>{value() ? left() : right()}</Show>;";
    assert_eq!(memos(source), [(23, 30, "value()")]);
    assert_eq!(emitted_memo_count(source), 1);
}

#[test]
fn native_condition_memos_span_the_booleanized_test() {
    for (source, expected) in [
        (
            "const C = () => <div>{value() ? left() : right()}</div>;",
            (22, 29, "value()"),
        ),
        (
            "const C = () => <div {...props} title={other() ? yes() : no()} />;",
            (39, 46, "other()"),
        ),
        (
            "const C = () => <div>{cond() && x()}</div>;",
            (22, 28, "cond()"),
        ),
    ] {
        assert_eq!(memos(source), [expected], "{source}");
        assert_eq!(emitted_memo_count(source), 1, "{source}");
    }
}

#[test]
fn a_fragment_child_condition_memo_is_brace_free_and_one_fact_per_memo() {
    // The path that reaches `transform_condition_inline` through
    // `dynamic_child_thunk`. Two memos are emitted — the fragment child's own
    // thunk wrapper and the condition test — and each is reported at its own
    // span. Neither covers `{`…`}`.
    let source = "const a = <Show><>{cond() ? <div/> : x()}</></Show>;";
    assert_eq!(
        memos(source),
        [(19, 25, "cond()"), (19, 40, "cond() ? <div/> : x()")]
    );
    assert_eq!(emitted_memo_count(source), 2);
}

#[test]
fn nested_condition_memos_are_reported_one_per_memo() {
    // Two tests are memoized (`!!x()` and `!!y()`), so two facts exist. A
    // fact spanning the whole conditional would collapse them into one and
    // would also claim the branches are memoized, which they are not.
    let source = "const C = () => <div>{x() ? (y() ? a() : b()) : c()}</div>;";
    assert_eq!(memos(source), [(22, 25, "x()"), (29, 32, "y()")]);
    assert_eq!(emitted_memo_count(source), 2);
}

#[test]
fn a_custom_effect_wrapper_keeps_its_identity_for_the_consumer() {
    let source = "const C = (props) => <div title={props.value} />;";
    let options = CompileOptions {
        effect_wrapper: Wrapper::Name("createRenderEffect".into()),
        ..options(true)
    };
    let trace = compile(source, &options)
        .expect("compiles")
        .semantic_trace
        .expect("tracing was requested");
    assert!(trace.owner_establishments.iter().any(|site| {
        site.wrapper == "createRenderEffect"
            && &source[site.span.start as usize..site.span.end as usize] == "props.value"
    }));
}

#[test]
fn the_style_shape_reconciles_without_inventing_an_owner() {
    // This shape emits no compiler wrapper around its four reported values.
    let trace = trace(STYLE_BEFORE_SPREAD);
    assert_eq!(trace.sites.len(), 4);
    assert!(trace.owner_establishments.is_empty());
}

#[test]
fn owner_establishments_cover_the_dom_emission_sites() {
    let source = r#"const C = (props) => <Show when={props.when}>
  <div
    title={props.title}
    id={props.id}
    onClick={props.onClick}
    ref={props.ref}
  >{props.child}</div>
</Show>;"#;
    let trace = trace(source);
    let facts = trace
        .owner_establishments
        .iter()
        .map(|site| {
            (
                site.wrapper.as_str(),
                &source[site.span.start as usize..site.span.end as usize],
            )
        })
        .collect::<Vec<_>>();
    for expected in [
        (
            "createComponent",
            "<Show when={props.when}>\n  <div\n    title={props.title}\n    id={props.id}\n    onClick={props.onClick}\n    ref={props.ref}\n  >{props.child}</div>\n</Show>",
        ),
        ("effect", "props.title"),
        ("effect", "props.id"),
        ("insert", "props.child"),
        ("delegated", "props.onClick"),
        ("ref-apply", "props.ref"),
    ] {
        assert!(facts.contains(&expected), "missing exact owner fact {expected:?}");
    }
}

#[test]
fn dynamic_bindings_in_one_template_share_an_effect_group() {
    let source = r#"const C = (props) => <div title={props.title} id={props.id} />;"#;
    let rendered = trace(source);
    let effects = rendered
        .owner_establishments
        .iter()
        .filter(|site| site.wrapper == "effect")
        .map(|site| {
            (
                &source[site.span.start as usize..site.span.end as usize],
                site.group_id,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(effects, [("props.title", Some(0)), ("props.id", Some(0))]);

    let single = trace("const C = (props) => <div title={props.title} />;");
    let single_effect = single
        .owner_establishments
        .iter()
        .find(|site| site.wrapper == "effect")
        .expect("single dynamic binding has an effect fact");
    assert_eq!(single_effect.group_id, None);
}

#[test]
fn insert_facts_use_each_child_expression_span_once() {
    let source = "const C = (props) => <div>{props.a()}{props.b()}<Foo /></div>;";
    let rendered = trace(source);
    let inserts = rendered
        .owner_establishments
        .iter()
        .filter(|site| site.wrapper == "insert")
        .map(|site| &source[site.span.start as usize..site.span.end as usize])
        .collect::<Vec<_>>();
    assert_eq!(inserts, ["props.a()", "props.b()", "<Foo />"]);
}

#[test]
fn separate_keyed_effects_receive_distinct_group_ids() {
    let source = r#"const A = (props) => <div title={props.a} id={props.b} />; const B = (props) => <span title={props.c} id={props.d} />;"#;
    let rendered = trace(source);
    let effects = rendered
        .owner_establishments
        .iter()
        .filter(|site| site.wrapper == "effect")
        .map(|site| {
            (
                &source[site.span.start as usize..site.span.end as usize],
                site.group_id,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(effects.len(), 4);
    assert_eq!(effects[0].0, "props.a");
    assert_eq!(effects[1].0, "props.b");
    assert_eq!(effects[2].0, "props.c");
    assert_eq!(effects[3].0, "props.d");
    assert_ne!(effects[0].1, effects[2].1);
    assert_eq!(effects[0].1, effects[1].1);
    assert_eq!(effects[2].1, effects[3].1);
}

#[test]
fn discarded_nested_head_lowering_emits_no_trace_facts() {
    let source = r#"const C = () => <div><head><span title={hidden()}>{hiddenChild()}</span></head><p>{shown()}</p></div>;"#;
    let rendered = trace(source);
    let owner_spans = rendered
        .owner_establishments
        .iter()
        .map(|site| &source[site.span.start as usize..site.span.end as usize])
        .collect::<Vec<_>>();
    assert!(!owner_spans.iter().any(|span| span.contains("hidden")));
    assert!(owner_spans.contains(&"shown()"));
}

#[test]
fn component_render_sites_are_recorded_at_component_lowering() {
    let source = r#"const C = () => <Show when={true}><Thing value={1} /></Show>;"#;
    let rendered = trace(source);
    let render_sites = rendered
        .component_render_sites
        .iter()
        .map(|site| &source[site.span.start as usize..site.span.end as usize])
        .collect::<Vec<_>>();
    assert_eq!(
        render_sites,
        [
            "<Show when={true}><Thing value={1} /></Show>",
            "<Thing value={1} />",
        ]
    );

    let member_source = "const C = () => <Thing.Item />;";
    let member_trace = trace(member_source);
    assert_eq!(
        member_trace
            .component_render_sites
            .iter()
            .map(|site| &member_source[site.span.start as usize..site.span.end as usize])
            .collect::<Vec<_>>(),
        ["<Thing.Item />"]
    );

    let native_trace = trace("const C = () => <div />;");
    assert!(native_trace.component_render_sites.is_empty());
}

#[test]
fn custom_memo_and_disabled_wrappers_are_reported_without_inference() {
    let custom = compile(
        FRAGMENT_IN_A_PROP_CALLBACK,
        &CompileOptions {
            memo_wrapper: Wrapper::Name("memoize".into()),
            ..options(true)
        },
    )
    .expect("compiles")
    .semantic_trace
    .expect("tracing was requested");
    assert!(custom
        .owner_establishments
        .iter()
        .any(|site| site.wrapper == "memoize"));

    let disabled = compile(
        FRAGMENT_IN_A_PROP_CALLBACK,
        &CompileOptions {
            effect_wrapper: Wrapper::Disabled,
            memo_wrapper: Wrapper::Disabled,
            ..options(true)
        },
    )
    .expect("compiles")
    .semantic_trace
    .expect("tracing was requested");
    assert!(disabled
        .owner_establishments
        .iter()
        .all(|site| site.wrapper != "effect" && site.wrapper != "memo"));
}

#[test]
fn delegated_event_bindings_record_their_emission_span() {
    let source = "const handler = () => act(); const C = () => <button onClick={handler} />;";
    let trace = trace(source);
    assert!(trace.owner_establishments.iter().any(|site| {
        site.wrapper == "delegated"
            && &source[site.span.start as usize..site.span.end as usize] == "handler"
    }));
}

#[test]
fn event_facts_partition_delegated_direct_and_capture_semantics() {
    let source = r#"const handler = () => act(); const C = () => <div onClick={handler} on:focus={handler} oncapture:blur={handler} />;"#;
    let rendered = trace(source);
    let facts = rendered
        .owner_establishments
        .iter()
        .filter(|site| matches!(site.wrapper.as_str(), "delegated" | "direct" | "capture"))
        .map(|site| {
            (
                site.wrapper.as_str(),
                &source[site.span.start as usize..site.span.end as usize],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        facts,
        [
            ("delegated", "handler"),
            ("direct", "handler"),
            ("capture", "handler")
        ]
    );

    let direct_options = CompileOptions {
        delegate_events: false,
        ..options(true)
    };
    let direct = compile(
        "const C = () => <button onClick={handler} />;",
        &direct_options,
    )
    .expect("compiles")
    .semantic_trace
    .expect("tracing was requested");
    assert!(direct
        .owner_establishments
        .iter()
        .any(|site| site.wrapper == "direct"));
}

#[test]
fn deferred_component_callbacks_record_their_receiver_span() {
    let source =
        r#"const C = (props) => <Thing label={props.label} ref={props.ref} {...props.data} />;"#;
    let rendered = trace(source);
    let callbacks = rendered
        .deferred_callback_sites
        .iter()
        .map(|site| {
            (
                &source[site.span.start as usize..site.span.end as usize],
                &source[site.receiver_span.start as usize..site.receiver_span.end as usize],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        callbacks,
        [
            (
                "props.label",
                "<Thing label={props.label} ref={props.ref} {...props.data} />",
            ),
            (
                "props.ref",
                "<Thing label={props.label} ref={props.ref} {...props.data} />",
            ),
            (
                "props.data",
                "<Thing label={props.label} ref={props.ref} {...props.data} />",
            ),
        ]
    );
    let refs = rendered
        .owner_establishments
        .iter()
        .filter(|site| site.wrapper == "ref-apply")
        .map(|site| &source[site.span.start as usize..site.span.end as usize])
        .collect::<Vec<_>>();
    assert_eq!(refs, ["props.ref"]);
}

#[test]
fn control_flow_child_callbacks_record_their_component_receiver() {
    let source = "const C = () => <Show>{() => value()}</Show>;";
    let rendered = trace(source);
    let callbacks = rendered
        .deferred_callback_sites
        .iter()
        .map(|site| {
            (
                &source[site.span.start as usize..site.span.end as usize],
                &source[site.receiver_span.start as usize..site.receiver_span.end as usize],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        callbacks,
        [("() => value()", "<Show>{() => value()}</Show>")]
    );
}

#[test]
fn ordinary_component_children_are_not_control_flow_callbacks() {
    let source = "const C = () => <Thing>{() => value()}</Thing>;";
    let rendered = trace(source);
    assert!(rendered.deferred_callback_sites.is_empty());
}
