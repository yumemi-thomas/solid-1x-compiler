//! Total original-source execution facts for Solid 1.x JSX.
//!
//! The census is independent from generated operations. Terminal value
//! decisions use the same `Classify` authority as lowering, while callback
//! decisions are fixed by the Solid 1 runtime ABI.

use std::collections::HashSet;

use oxc_ast::ast::{
    Expression, JSXAttributeItem, JSXAttributeName, JSXAttributeValue, JSXChild, JSXElement,
    JSXElementName, JSXExpression, JSXFragment, Program,
};
use oxc_ast_visit::Visit;
use oxc_span::{GetSpan, Span};
use sha2::{Digest, Sha256};

use crate::{
    config::TransformOptions,
    shared::{bindings::BindingTable, classify::Classify, utils::is_component_name},
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum SiteKind {
    JsxChild,
    NativeAttribute,
    NativeSpread,
    ComponentProperty,
    ComponentSpread,
    ComponentChild,
    EventHandler,
    Ref,
    ControlFlowRender,
}

impl SiteKind {
    const fn name(self) -> &'static str {
        match self {
            Self::JsxChild => "jsx-child",
            Self::NativeAttribute => "native-attribute",
            Self::NativeSpread => "native-spread",
            Self::ComponentProperty => "component-property",
            Self::ComponentSpread => "component-spread",
            Self::ComponentChild => "component-child",
            Self::EventHandler => "event-handler",
            Self::Ref => "ref",
            Self::ControlFlowRender => "control-flow-render",
        }
    }

    const fn callback(self) -> bool {
        matches!(
            self,
            Self::EventHandler | Self::Ref | Self::ControlFlowRender
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct Site {
    span: Span,
    kind: SiteKind,
    dynamic: bool,
    elided: bool,
}

impl Site {
    fn id(self) -> String {
        format!("{}:{}:{}", self.kind.name(), self.span.start, self.span.end)
    }
}

struct Census<'a> {
    sites: Vec<Site>,
    built_ins: HashSet<&'a str>,
    bindings: BindingTable,
    source: &'a str,
    static_marker: &'a str,
    parent_component: bool,
}

impl Census<'_> {
    fn attribute_name(name: &JSXAttributeName<'_>) -> String {
        match name {
            JSXAttributeName::Identifier(name) => name.name.to_string(),
            JSXAttributeName::NamespacedName(name) => {
                format!("{}:{}", name.namespace.name, name.name.name)
            }
        }
    }

    fn push_expression(
        &mut self,
        expression: &Expression<'_>,
        leading_from: Option<u32>,
        kind: SiteKind,
        check_tags: bool,
        elided: bool,
    ) {
        let dynamic = Classify::new(&self.bindings, self.source, self.static_marker).is_dynamic(
            leading_from,
            expression,
            check_tags,
        );
        let span = expression.span();
        if span.start < span.end {
            self.sites.push(Site {
                span,
                kind,
                dynamic,
                elided,
            });
        }
    }

    fn expression(
        &mut self,
        expression: &JSXExpression<'_>,
        leading_from: Option<u32>,
        kind: SiteKind,
        check_tags: bool,
        elided: bool,
    ) {
        if let Some(expression) = expression.as_expression() {
            self.push_expression(expression, leading_from, kind, check_tags, elided);
        }
    }

    fn child(&mut self, child: &JSXChild<'_>, component: bool, control_flow: bool) {
        match child {
            JSXChild::ExpressionContainer(container) => {
                let function = matches!(
                    container.expression,
                    JSXExpression::ArrowFunctionExpression(_)
                        | JSXExpression::FunctionExpression(_)
                );
                let kind = if component && control_flow && function {
                    SiteKind::ControlFlowRender
                } else if component {
                    SiteKind::ComponentChild
                } else {
                    SiteKind::JsxChild
                };
                self.expression(
                    &container.expression,
                    Some(container.span.start),
                    kind,
                    false,
                    false,
                );
            }
            JSXChild::Spread(spread) => self.push_expression(
                &spread.expression,
                None,
                if component {
                    SiteKind::ComponentChild
                } else {
                    SiteKind::JsxChild
                },
                false,
                false,
            ),
            _ => {}
        }
    }
}

impl<'b> Visit<'b> for Census<'_> {
    fn visit_program(&mut self, program: &Program<'b>) {
        for statement in &program.body {
            self.bindings.collect(statement);
        }
        oxc_ast_visit::walk::walk_program(self, program);
    }

    fn visit_jsx_element(&mut self, element: &JSXElement<'b>) {
        let component = is_component_name(&element.opening_element.name);
        let control_flow = match &element.opening_element.name {
            JSXElementName::IdentifierReference(name) => {
                self.built_ins.contains(name.name.as_str())
            }
            _ => false,
        };
        let has_children = !element.children.is_empty();
        for item in &element.opening_element.attributes {
            match item {
                JSXAttributeItem::SpreadAttribute(spread) => self.push_expression(
                    &spread.argument,
                    None,
                    if component {
                        SiteKind::ComponentSpread
                    } else {
                        SiteKind::NativeSpread
                    },
                    false,
                    false,
                ),
                JSXAttributeItem::Attribute(attribute) => {
                    let Some(JSXAttributeValue::ExpressionContainer(container)) = &attribute.value
                    else {
                        continue;
                    };
                    let name = Self::attribute_name(&attribute.name);
                    let primitive = matches!(
                        container.expression,
                        JSXExpression::StringLiteral(_)
                            | JSXExpression::NumericLiteral(_)
                            | JSXExpression::BooleanLiteral(_)
                    );
                    let kind = if name == "ref" {
                        SiteKind::Ref
                    } else if !component && name.starts_with("on") && !primitive {
                        SiteKind::EventHandler
                    } else if component {
                        SiteKind::ComponentProperty
                    } else {
                        SiteKind::NativeAttribute
                    };
                    self.expression(
                        &container.expression,
                        Some(container.span.start),
                        kind,
                        component,
                        !component && name == "children" && has_children,
                    );
                }
            }
        }
        for child in &element.children {
            self.child(child, component, control_flow);
        }
        let previous = self.parent_component;
        self.parent_component = component;
        oxc_ast_visit::walk::walk_jsx_element(self, element);
        self.parent_component = previous;
    }

    fn visit_jsx_fragment(&mut self, fragment: &JSXFragment<'b>) {
        for child in &fragment.children {
            self.child(child, self.parent_component, false);
        }
        oxc_ast_visit::walk::walk_jsx_fragment(self, fragment);
    }
}

fn value_decision(site: Site) -> &'static str {
    if site.elided {
        return "elided";
    }
    match site.kind {
        SiteKind::ComponentProperty
        | SiteKind::ComponentSpread
        | SiteKind::ComponentChild
        | SiteKind::NativeSpread => {
            if site.dynamic {
                "caller-context"
            } else {
                "eager-once"
            }
        }
        SiteKind::JsxChild | SiteKind::NativeAttribute => {
            if site.dynamic {
                "reactive-rerun"
            } else {
                "eager-once"
            }
        }
        SiteKind::EventHandler | SiteKind::Ref | SiteKind::ControlFlowRender => unreachable!(),
    }
}

fn callback_decision(kind: SiteKind) -> &'static str {
    match kind {
        SiteKind::EventHandler => "later-event",
        SiteKind::Ref => "ref-apply",
        SiteKind::ControlFlowRender => "later-render",
        _ => unreachable!(),
    }
}

pub(crate) fn execution_contract(
    source: &str,
    program: &Program<'_>,
    options: &TransformOptions,
) -> Result<String, String> {
    if options.generate.as_deref().unwrap_or("dom") != "dom" {
        return Err("Solid 1 execution facts currently support the DOM output mode only".into());
    }
    let built_ins = options.built_ins.clone().unwrap_or_default();
    let static_marker = options.static_marker.as_deref().unwrap_or("@static");
    let mut census = Census {
        sites: Vec::new(),
        built_ins: built_ins.iter().map(String::as_str).collect(),
        bindings: BindingTable::default(),
        source,
        static_marker,
        parent_component: false,
    };
    census.visit_program(program);
    census
        .sites
        .sort_by_key(|site| (site.span.start, site.span.end, site.kind));
    census
        .sites
        .dedup_by_key(|site| (site.span.start, site.span.end, site.kind));

    let sites = census
        .sites
        .iter()
        .map(|site| {
            serde_json::json!({
                "id": site.id(),
                "span": { "start": site.span.start, "end": site.span.end },
                "kind": site.kind.name()
            })
        })
        .collect::<Vec<_>>();
    let values = census
        .sites
        .iter()
        .filter(|site| !site.kind.callback())
        .map(|site| {
            serde_json::json!({
                "site": site.id(),
                "decision": value_decision(*site)
            })
        })
        .collect::<Vec<_>>();
    let callbacks = census
        .sites
        .iter()
        .filter(|site| site.kind.callback())
        .map(|site| {
            serde_json::json!({
                "site": site.id(),
                "decision": callback_decision(site.kind)
            })
        })
        .collect::<Vec<_>>();

    let mut normalized_built_ins = built_ins;
    normalized_built_ins.sort();
    normalized_built_ins.dedup();
    let options_hash = serde_json::to_vec(&serde_json::json!({
        "moduleName": options.module_name.as_deref().unwrap_or("dom"),
        "generate": "dom",
        "hydratable": options.hydratable.unwrap_or(false),
        "dev": false,
        "effectWrapper": options.effect_wrapper.as_ref().map(|_| "configured"),
        "wrapConditionals": options.wrap_conditionals.unwrap_or(true),
        "staticMarker": static_marker,
        "builtIns": normalized_built_ins
    }))
    .map_err(|error| error.to_string())?;
    let identity = serde_json::json!({
        "name": "@dom-expressions/compiler",
        "version": env!("CARGO_PKG_VERSION"),
        "semantics": "execution-v1.1"
    });
    let contract = serde_json::json!({
        "contractKind": "execution",
        "wireVersion": 2,
        "scope": {
            "sourceHash": format!("sha256:{:x}", Sha256::digest(source.as_bytes())),
            "sourceBytes": source.len(),
            "outputMode": "dom",
            "solidSemantics": "1",
            "compiler": identity,
            "optionsHash": format!("sha256:{:x}", Sha256::digest(options_hash))
        },
        "coverage": [
            { "facet": "callback-invocation", "version": 1 },
            { "facet": "execution-sites", "version": 1 },
            { "facet": "value-execution", "version": 1 }
        ],
        "provenance": {
            "producer": identity,
            "artifacts": []
        },
        "facets": {
            "executionSites": { "version": 1, "sites": sites },
            "valueExecution": { "version": 1, "decisions": values },
            "callbackInvocation": { "version": 1, "decisions": callbacks }
        }
    });
    serde_json::to_string(&contract).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    use super::*;

    #[test]
    fn solid_one_contract_is_total() {
        let source = r#"
const view = <button title={label()} onClick={() => act()}>
  {count()}
  <For each={items()}>{item => <span>{item}</span>}</For>
</button>;
"#;
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
        assert!(parsed.errors.is_empty());
        let contract: serde_json::Value = serde_json::from_str(
            &execution_contract(
                source,
                &parsed.program,
                &TransformOptions {
                    module_name: Some("dom".into()),
                    generate: Some("dom".into()),
                    built_ins: Some(vec!["For".into()]),
                    ..TransformOptions::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(contract["scope"]["solidSemantics"], "1");
        let sites = contract["facets"]["executionSites"]["sites"]
            .as_array()
            .unwrap();
        let values = contract["facets"]["valueExecution"]["decisions"]
            .as_array()
            .unwrap();
        let callbacks = contract["facets"]["callbackInvocation"]["decisions"]
            .as_array()
            .unwrap();
        assert!(!sites.is_empty());
        assert_eq!(sites.len(), values.len() + callbacks.len());
    }
}
