use crate::error::{Error, Result};
use oxc_allocator::CloneIn;
use oxc_ast::ast::{JSXAttributeItem, JSXAttributeValue, ObjectPropertyKind, Statement};
use oxc_span::{GetSpan, Span};

use crate::dom::element::AstDomTransform;
use crate::shared::ast::arrow_return_expression;
use crate::shared::condition::{
    is_condition_shape, transform_condition_inline, zero_arg_call_thunk,
};
use crate::shared::constants::{dom_properties, svg_elements};
use crate::shared::utils::{decode_html_entities, source_from_span};

/// Babel main's `canNativeSpread`: refs and namespaces that require
/// compile-time Solid 1.x lowering stay outside the runtime spread object.
pub(crate) fn can_native_spread(attr: &oxc_ast::ast::JSXAttribute<'_>) -> bool {
    match &attr.name {
        oxc_ast::ast::JSXAttributeName::Identifier(name) => name.name != "ref",
        oxc_ast::ast::JSXAttributeName::NamespacedName(name) => !matches!(
            name.namespace.name.as_str(),
            "class" | "style" | "use" | "prop" | "attr" | "bool"
        ),
    }
}

/// Where one attribute of a spread-bearing element ends up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SpreadRoute {
    /// A `{...expr}` token: always an operand of the merge.
    Spread,
    /// Peeled back out of the merge and handed to the ordinary attribute
    /// pipeline (`AttrPlanner` → template text, static setters, dynamics).
    Planned,
    /// Merged whole into the runtime props object `_$spread` receives.
    Merged,
}

/// Babel 0.40.x's positional carve-out in `processSpreads`, in one place.
///
/// A non-dynamic attribute written *before* the first spread token cannot be
/// clobbered by any spread, so Babel leaves it to the ordinary attribute
/// pipeline — where a `style`/`classList` object decomposes per declaration
/// and literal declarations fold into the template string. Everything else on
/// a spread-bearing element merges into the runtime props object, where the
/// object stays one opaque value. On an element with no spread at all every
/// attribute is planned.
///
/// [`crate::dom::attrs`]'s filtered planner list,
/// [`AstDomTransform::spread_attribute_statement`]'s merge loop, and the
/// execution census in [`crate::semantic_trace`] all read this function. The
/// rule is positional and per-attribute; expressing it a second time as an
/// element-wide "has a spread anywhere" test is what let the census enumerate
/// a `style` object at a granularity lowering never uses.
pub(crate) fn spread_routes(
    attributes: &[JSXAttributeItem<'_>],
    classify: &crate::shared::classify::Classify<'_>,
) -> std::vec::Vec<SpreadRoute> {
    let has_spread = attributes
        .iter()
        .any(|item| matches!(item, JSXAttributeItem::SpreadAttribute(_)));
    let mut seen_spread = false;
    attributes
        .iter()
        .map(|item| match item {
            JSXAttributeItem::SpreadAttribute(_) => {
                seen_spread = true;
                SpreadRoute::Spread
            }
            JSXAttributeItem::Attribute(attr) => {
                if !has_spread {
                    return SpreadRoute::Planned;
                }
                let dynamic = matches!(
                    &attr.value,
                    Some(JSXAttributeValue::ExpressionContainer(container))
                        if container.expression.as_expression().is_some_and(|expression| {
                            classify.is_dynamic(Some(container.span.start), expression, false)
                        })
                );
                if !can_native_spread(attr) || (!seen_spread && !dynamic) {
                    SpreadRoute::Planned
                } else {
                    SpreadRoute::Merged
                }
            }
        })
        .collect()
}

impl<'a> AstDomTransform<'a, '_> {
    /// Port of Babel's `processSpreads` (dom/element.ts).
    pub(crate) fn spread_attribute_statement(
        &mut self,
        attributes: &[JSXAttributeItem<'a>],
        element_id: &str,
        tag_name: &str,
        skip_children: bool,
        children_from_attribute: bool,
    ) -> Result<Statement<'a>> {
        self.template_state.uses_spread = true;
        // A spread may carry delegated event handlers, which can't be known at
        // compile time; hydratable roots must replay events (Babel parity).
        if self.hydratable {
            self.has_hydratable_event = true;
        }
        let mut prop_objects = std::vec::Vec::new();
        let mut running_props = std::vec::Vec::new();
        let mut dynamic_spread = false;
        let routes = spread_routes(attributes, &self.classify());
        for (attr, route) in attributes.iter().zip(routes) {
            match attr {
                JSXAttributeItem::SpreadAttribute(spread) => {
                    if !running_props.is_empty() {
                        prop_objects.push(self.ast().expression_object(
                            spread.span,
                            self.ast().vec_from_iter(running_props.drain(..)),
                        ));
                    }
                    let is_static =
                        source_from_span(spread.span, self.source).contains(&self.static_marker);
                    let dynamic = self.classify().is_dynamic(None, &spread.argument, false);
                    self.semantic_trace.value(
                        spread.argument.span(),
                        crate::semantic_trace::ExecutionSiteKind::NativeSpread,
                        if dynamic {
                            crate::semantic_trace::ValueDecision::CallerContext
                        } else {
                            crate::semantic_trace::ValueDecision::EagerOnce
                        },
                    );
                    let value = spread.argument.clone_in(self.allocator);
                    let value = if dynamic {
                        dynamic_spread = true;
                        // Babel's `inlineCallExpression`: `{...results()}`
                        // passes `results` straight through to mergeProps.
                        match zero_arg_call_thunk(&value, self.allocator) {
                            Some(callee) => callee,
                            None => arrow_return_expression(self.allocator, spread.span, value),
                        }
                    } else {
                        value
                    };
                    let value = if is_static {
                        let mut properties = self.ast().vec();
                        properties.push(ObjectPropertyKind::SpreadProperty(
                            self.ast().alloc_spread_element(spread.span, value),
                        ));
                        self.ast().expression_object(spread.span, properties)
                    } else {
                        value
                    };
                    prop_objects.push(value);
                }
                JSXAttributeItem::Attribute(attr) => {
                    // Refs, compiler namespaces, and the pre-spread carve-out
                    // are all decided by `spread_routes`; only a `Merged`
                    // attribute belongs in the props object.
                    if route != SpreadRoute::Merged {
                        continue;
                    }
                    running_props.push(self.spread_attribute_property(
                        attr,
                        skip_children,
                        children_from_attribute,
                    )?);
                }
            }
        }
        if !running_props.is_empty() {
            prop_objects.push(self.ast().expression_object(
                Span::default(),
                self.ast().vec_from_iter(running_props.drain(..)),
            ));
        }

        let props = if prop_objects.len() == 1 && !dynamic_spread {
            prop_objects
                .pop()
                .expect("single spread props object exists")
        } else {
            self.template_state.uses_merge_props = true;
            self.call_identifier(Span::default(), "_$mergeProps", prop_objects)
        };

        Ok(self.ast().statement_expression(
            Span::default(),
            self.call_identifier(
                Span::default(),
                "_$spread",
                vec![
                    self.identifier_expression(Span::default(), element_id),
                    props,
                    self.ast()
                        .expression_boolean_literal(Span::default(), svg_elements(tag_name)),
                    self.ast()
                        .expression_boolean_literal(Span::default(), skip_children),
                ],
            ),
        ))
    }

    fn spread_attribute_property(
        &mut self,
        attr: &oxc_ast::ast::JSXAttribute<'a>,
        skip_children: bool,
        children_from_attribute: bool,
    ) -> Result<ObjectPropertyKind<'a>> {
        let name = match &attr.name {
            oxc_ast::ast::JSXAttributeName::Identifier(name) => name.name.to_string(),
            oxc_ast::ast::JSXAttributeName::NamespacedName(name) => {
                format!("{}:{}", name.namespace.name, name.name.name)
            }
        };

        // Babel's no-`inlineStyles` preprocess wraps style values in IIFEs at
        // the JSX level, before spreads are processed — the wrap makes the
        // value a call expression, so it always lands as a getter (and any
        // `/*@static*/` marker is lost with the original node's comments).
        if name == "style" && !self.inline_styles {
            match &attr.value {
                Some(JSXAttributeValue::StringLiteral(value)) => {
                    let planner = self.attr_planner();
                    let text = decode_html_entities(&value.value);
                    let template = planner.style_string_template_literal(attr.span, &text);
                    let wrapped = planner.style_no_inline_iife(attr.span, template);
                    return Ok(self.object_getter_property(attr.span, &name, wrapped));
                }
                Some(JSXAttributeValue::ExpressionContainer(container))
                    if container.expression.as_expression().is_some() =>
                {
                    // The IIFE wrap makes every value a getter, so the spread
                    // consumer decides when it is read.
                    self.semantic_trace.value(
                        container.expression.span(),
                        crate::semantic_trace::ExecutionSiteKind::NativeAttribute,
                        crate::semantic_trace::ValueDecision::CallerContext,
                    );
                    let value = self.attribute_value_expression(container);
                    let wrapped = self.attr_planner().style_no_inline_iife(attr.span, value);
                    return Ok(self.object_getter_property(attr.span, &name, wrapped));
                }
                _ => {}
            }
        }

        match &attr.value {
            None => Ok(self.object_property(
                attr.span,
                &name,
                if dom_properties(&name) {
                    self.ast().expression_boolean_literal(attr.span, true)
                } else {
                    self.ast()
                        .expression_string_literal(attr.span, self.ast().atom(""), None)
                },
            )),
            Some(JSXAttributeValue::StringLiteral(value)) => {
                let value = decode_html_entities(&value.value);
                Ok(self.object_property(
                    attr.span,
                    &name,
                    self.ast()
                        .expression_string_literal(attr.span, self.ast().atom(&value), None),
                ))
            }
            Some(JSXAttributeValue::ExpressionContainer(container)) => {
                let dynamic = container.expression.as_expression().is_some_and(|expression| {
                    self.classify()
                        .is_dynamic(Some(container.span.start), expression, false)
                });
                // `ref`/`use:` never reach the spread object (`can_native_spread`).
                let semantic_kind = if name.starts_with("on") {
                    self.semantic_trace.callback(
                        container.expression.span(),
                        crate::semantic_trace::ExecutionSiteKind::EventHandler,
                        crate::semantic_trace::CallbackDecision::LaterEvent,
                    );
                    None
                } else if name == "children" {
                    // A promoted `children` attribute is reported by child
                    // insertion instead.
                    if children_from_attribute {
                        None
                    } else {
                        self.semantic_trace.value(
                            container.expression.span(),
                            crate::semantic_trace::ExecutionSiteKind::JsxChild,
                            if dynamic {
                                if skip_children {
                                    crate::semantic_trace::ValueDecision::Elided
                                } else {
                                    crate::semantic_trace::ValueDecision::ReactiveRerun
                                }
                            } else {
                                crate::semantic_trace::ValueDecision::EagerOnce
                            },
                        );
                        None
                    }
                } else {
                    Some(crate::semantic_trace::ExecutionSiteKind::NativeAttribute)
                };
                if let Some(kind) = semantic_kind {
                    self.semantic_trace.value(
                        container.expression.span(),
                        kind,
                        if dynamic {
                            crate::semantic_trace::ValueDecision::CallerContext
                        } else {
                            crate::semantic_trace::ValueDecision::EagerOnce
                        },
                    );
                }
                let mut value = self.attribute_value_expression(container);
                self.attr_planner().fold_confident(&mut value);
                if dynamic {
                    // Babel: logical/conditional getter bodies flow through
                    // `transformCondition(..., inline)`.
                    let value = if self.wrap_conditionals && is_condition_shape(&value) {
                        transform_condition_inline(self, container.span, value)
                    } else {
                        value
                    };
                    Ok(self.object_getter_property(attr.span, &name, value))
                } else {
                    Ok(self.object_property(attr.span, &name, value))
                }
            }
            Some(JSXAttributeValue::Element(_) | JSXAttributeValue::Fragment(_)) => {
                Err(Error::from_reason(
                    "JSX spread attribute object values are not implemented in the AST-native milestone yet",
                ))
            }
        }
    }
}
