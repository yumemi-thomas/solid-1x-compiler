// Integration tests link the rlib, whose Node-API registration constructors
// have no host to resolve against.
#![cfg(not(feature = "node"))]

//! The four behavioral changes `babel-plugin-jsx-dom-expressions` made between
//! 0.40.7 and 0.40.10, pinned on this side of the port.
//!
//! The live differential harness (`__tests__/parity.test.js`,
//! `__tests__/parity-probes.test.js`) already compares every one of these
//! shapes against the vendored oracle. These tests pin the *emitted code* so
//! the delta is legible in review and so a regression names the change it
//! broke rather than showing up as an anonymous diff.

use dom_expressions_compiler::{compile, CompileOptions, Generate};

fn ssr() -> CompileOptions {
    CompileOptions {
        module_name: "r-server".into(),
        generate: Generate::Ssr,
        built_ins: vec!["For".into(), "Show".into()],
        static_marker: "@once".into(),
        ..CompileOptions::default()
    }
}

fn dom() -> CompileOptions {
    CompileOptions {
        module_name: "r-dom".into(),
        built_ins: vec!["For".into(), "Show".into()],
        static_marker: "@once".into(),
        ..CompileOptions::default()
    }
}

fn emitted(source: &str, options: &CompileOptions) -> String {
    compile(source, options).expect("compiles").code
}

// ---------------------------------------------------------------------------
// Change 3 — template-literal quasis are HTML-escaped in attribute position.
//
// The security fix. Before it, a `"` written into the static text of a
// template literal reached the attribute unescaped and closed the quoted
// value: `style={`url("${x}")`}` served `style="url("` followed by attacker-
// positioned markup.
// ---------------------------------------------------------------------------

#[test]
fn an_attribute_template_literal_escapes_its_static_quasis() {
    assert_eq!(
        emitted(r#"const a = <div title={`a"b&c ${x()} d`} />;"#, &ssr()),
        r#"import { escape as _$escape } from "r-server";
import { ssr as _$ssr } from "r-server";
var _tmpl$ = ["<div title=\"", "\"></div>"];
const a = _$ssr(_tmpl$, `a&quot;b&amp;c ${_$escape(x(), true)} d`);
"#
    );
}

#[test]
fn the_injection_shape_no_longer_closes_the_attribute() {
    let source = r#"const a = <div data-src={`url("${u()}")`} />;"#;
    let code = emitted(source, &ssr());
    assert!(
        code.contains(r#"`url(&quot;${_$escape(u(), true)}&quot;)`"#),
        "{code}"
    );
    // The literal `"` that closed the attribute is gone from the value.
    assert!(!code.contains(r#"`url("$"#), "{code}");
}

#[test]
fn escaping_a_quasi_re_escapes_the_template_delimiters() {
    // The case a naive port corrupts: the quasi already carries an escaped
    // backtick and an escaped `${`. Escaping is applied to the cooked text,
    // but `raw` is what the printer emits, so both delimiters have to be
    // re-escaped on the way back or the literal terminates early / opens an
    // interpolation.
    let source = "const a = <div title={`a\\`b \\${c} \"d\" &e ${x()}`} />;";
    assert_eq!(
        emitted(source, &ssr()),
        r#"import { escape as _$escape } from "r-server";
import { ssr as _$ssr } from "r-server";
var _tmpl$ = ["<div title=\"", "\"></div>"];
const a = _$ssr(_tmpl$, `a\`b \${c} &quot;d&quot; &amp;e ${_$escape(x(), true)}`);
"#
    );
}

#[test]
fn a_text_position_template_literal_keeps_its_quasis_verbatim() {
    // `escapeTemplateQuasis` is called only under `attr`. Text position still
    // relies on the runtime `escape(...)` for the interpolations and leaves
    // the static text alone.
    assert_eq!(
        emitted(r#"const a = <div>{`a"b&c<d ${x()}`}</div>;"#, &ssr()),
        r#"import { escape as _$escape } from "r-server";
import { ssr as _$ssr } from "r-server";
var _tmpl$ = ["<div>", "</div>"];
const a = _$ssr(_tmpl$, `a"b&c<d ${_$escape(x())}`);
"#
    );
}

// ---------------------------------------------------------------------------
// Change 4 — `wrapConditionals` no longer excludes ssr for component props.
//
// Only `shared/component.js`'s gate lost `config.generate !== "ssr"`.
// `shared/transform.js`'s gate — child and fragment holes — kept it.
// ---------------------------------------------------------------------------

#[test]
fn an_ssr_component_prop_collapses_a_conditional_through_transform_condition() {
    assert_eq!(
        emitted(r#"const a = <Comp value={cond() ? x() : y()} />;"#, &ssr()),
        r#"import { memo as _$memo } from "r-server";
import { createComponent as _$createComponent } from "r-server";
const a = _$createComponent(Comp, { get value() {
	return _$memo(() => {
		return !!cond();
	})() ? x() : y();
} });
"#
    );
}

#[test]
fn an_ssr_component_prop_collapses_a_logical_expression_too() {
    let code = emitted(r#"const a = <Comp value={cond() && x()} />;"#, &ssr());
    assert!(code.contains("return _$memo(() => {"), "{code}");
    assert!(code.contains("})() && x();"), "{code}");
}

#[test]
fn an_ssr_child_hole_still_bypasses_transform_condition() {
    // The control: `shared/transform.js` still carries its ssr exclusion, so
    // a conditional in child position keeps the plain escaped-branch shape.
    assert_eq!(
        emitted(r#"const a = <div>{cond() ? x() : y()}</div>;"#, &ssr()),
        r#"import { escape as _$escape } from "r-server";
import { ssr as _$ssr } from "r-server";
var _tmpl$ = ["<div>", "</div>"];
const a = _$ssr(_tmpl$, cond() ? _$escape(x()) : _$escape(y()));
"#
    );
}

#[test]
fn wrap_conditionals_false_still_disables_the_ssr_prop_collapse() {
    let options = CompileOptions {
        wrap_conditionals: false,
        ..ssr()
    };
    let code = emitted(
        r#"const a = <Comp value={cond() ? x() : y()} />;"#,
        &options,
    );
    assert!(!code.contains("_$memo"), "{code}");
    assert!(code.contains("return cond() ? x() : y();"), "{code}");
}

#[test]
fn the_dom_component_prop_shape_is_unchanged() {
    assert_eq!(
        emitted(r#"const a = <Comp value={cond() ? x() : y()} />;"#, &dom()),
        r#"import { memo as _$memo } from "r-dom";
import { createComponent as _$createComponent } from "r-dom";
const a = _$createComponent(Comp, { get value() {
	return _$memo(() => {
		return !!cond();
	})() ? x() : y();
} });
"#
    );
}

// ---------------------------------------------------------------------------
// Changes 1 + 2 — `omitServerOnlyTemplates`, default `true`.
//
// An opt-out knob, so the default must emit exactly what 0.40.7 emitted.
// ---------------------------------------------------------------------------

const SERVER_ONLY: &str = "const a = <div $ServerOnly>{x()}</div>;";

#[test]
fn a_hydratable_server_only_element_skips_its_template_by_default() {
    let options = CompileOptions {
        hydratable: true,
        ..dom()
    };
    assert_eq!(
        emitted(SERVER_ONLY, &options),
        r#"import { getNextElement as _$getNextElement } from "r-dom";
import { insert as _$insert } from "r-dom";
var _el$ = _$getNextElement();
_$insert(_el$, x);
const a = _el$;
"#
    );
    // The default is the pre-0.40.10 behavior, spelled out.
    assert_eq!(
        emitted(SERVER_ONLY, &options),
        emitted(
            SERVER_ONLY,
            &CompileOptions {
                omit_server_only_templates: true,
                ..options.clone()
            }
        )
    );
}

#[test]
fn opting_out_keeps_the_template_and_still_drops_the_attribute() {
    let options = CompileOptions {
        hydratable: true,
        omit_server_only_templates: false,
        ..dom()
    };
    let code = emitted(SERVER_ONLY, &options);
    assert_eq!(
        code,
        r#"import { template as _$template } from "r-dom";
import { getNextElement as _$getNextElement } from "r-dom";
import { insert as _$insert } from "r-dom";
var _tmpl$ = /* @__PURE__ */ _$template(`<div>`);
var _el$ = _$getNextElement(_tmpl$);
_$insert(_el$, x);
const a = _el$;
"#
    );
    // Only the `skipTemplate` assignment moved inside the new conditional;
    // the `return` that drops the attribute from the markup did not.
    assert!(!code.contains("$ServerOnly"), "{code}");
}

#[test]
fn opting_out_does_not_revive_document_shell_templates() {
    // `html`/`head`/`body` have their own unconditional `skipTemplate` in
    // `dom/element.js`, untouched by 0.40.10.
    let options = CompileOptions {
        hydratable: true,
        omit_server_only_templates: false,
        ..dom()
    };
    let code = emitted("const a = <body>{x()}</body>;", &options);
    assert!(!code.contains("_$template"), "{code}");
}

#[test]
fn the_knob_is_inert_outside_hydratable_compilation() {
    let on = CompileOptions { ..dom() };
    let off = CompileOptions {
        omit_server_only_templates: false,
        ..dom()
    };
    assert_eq!(emitted(SERVER_ONLY, &on), emitted(SERVER_ONLY, &off));
}
