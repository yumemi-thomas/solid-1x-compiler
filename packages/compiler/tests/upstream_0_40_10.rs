// Integration tests link the rlib, whose Node-API registration constructors
// have no host to resolve against.
#![cfg(not(feature = "node"))]

//! The behavioral changes `babel-plugin-jsx-dom-expressions` made between
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
