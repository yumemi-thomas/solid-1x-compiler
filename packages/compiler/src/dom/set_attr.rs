use oxc_allocator::CloneIn;
use oxc_ast::ast::{AssignmentOperator, AssignmentTarget, Expression, UnaryOperator};
use oxc_span::Span;

use crate::dom::element::AstDomTransform;
use crate::shared::constants::{
    child_properties, dom_properties, dom_with_state, namespaces, prop_alias, reserved_namespace,
    svg_elements,
};

/// Options mirroring Babel's `setAttr(path, elem, name, value, options)`.
#[derive(Default)]
pub(crate) struct SetAttrOptions<'a> {
    pub(crate) dynamic: bool,
    pub(crate) prev_id: Option<Expression<'a>>,
    pub(crate) tag_name: String,
    pub(crate) style_property: bool,
    pub(crate) class_property: bool,
}

impl<'a> AstDomTransform<'a, '_> {
    /// Faithful port of Babel's `setAttr` (dom/element.ts): the single point
    /// that decides how one attribute write compiles (helper call, property
    /// assignment, classList toggle, ...), shared between static expressions
    /// and effect-wrapped dynamics.
    pub(crate) fn set_attr_expression(
        &mut self,
        span: Span,
        elem: Expression<'a>,
        name: &str,
        value: Expression<'a>,
        options: SetAttrOptions<'a>,
    ) -> Expression<'a> {
        let mut name = name.to_string();
        let mut namespace = None;
        let split = name
            .split_once(':')
            .map(|(prefix, rest)| (prefix.to_string(), rest.to_string()));
        if let Some((prefix, rest)) = split {
            if reserved_namespace(&prefix) && !rest.is_empty() {
                namespace = Some(prefix);
                name = rest;
            }
        }

        if namespace.as_deref() == Some("style") || options.style_property {
            self.template_state.uses_set_style_property = true;
            // Babel unwraps `ident = value` assignments to the assigned value.
            let value = match value {
                Expression::AssignmentExpression(assignment)
                    if matches!(
                        assignment.left,
                        AssignmentTarget::AssignmentTargetIdentifier(_)
                    ) =>
                {
                    assignment.right.clone_in(self.allocator)
                }
                value => value,
            };
            return self.call_identifier(
                span,
                "_$setStyleProperty",
                vec![
                    elem,
                    self.ast()
                        .expression_string_literal(span, self.ast().atom(&name), None),
                    value,
                ],
            );
        }

        if namespace.as_deref() == Some("class") || options.class_property {
            let value = if options.dynamic {
                value
            } else {
                self.double_negation(span, value)
            };
            let toggle = self.static_member_expression_from_expression(
                span,
                self.static_member_expression_from_expression(span, elem, "classList"),
                "toggle",
            );
            return self.call_expression(
                span,
                toggle,
                vec![
                    self.ast()
                        .expression_string_literal(span, self.ast().atom(&name), None),
                    value,
                ],
            );
        }

        if name == "style" {
            self.template_state.uses_style = true;
            let mut args = vec![elem, value];
            if let Some(prev) = options.prev_id {
                args.push(prev);
            }
            return self.call_identifier(span, "_$style", args);
        }

        if name == "class" && !svg_elements(&options.tag_name) {
            self.template_state.uses_class_name = true;
            return self.call_identifier(span, "_$className", vec![elem, value]);
        }

        if name == "className" && svg_elements(&options.tag_name) {
            name = "class".to_string();
        }

        if name == "classList" {
            self.template_state.uses_class_list = true;
            let mut args = vec![elem, value];
            if let Some(prev) = options.prev_id {
                args.push(prev);
            }
            return self.call_identifier(span, "_$classList", args);
        }

        if options.dynamic && name == "textContent" {
            if self.hydratable {
                self.template_state.uses_set_property = true;
                return self.call_identifier(
                    span,
                    "_$setProperty",
                    vec![
                        elem,
                        self.ast()
                            .expression_string_literal(span, self.ast().atom("data"), None),
                        value,
                    ],
                );
            }
            return self.member_assignment(span, elem, "data", value);
        }

        if namespace.as_deref() == Some("bool") {
            self.template_state.uses_set_bool_attribute = true;
            return self.call_identifier(
                span,
                "_$setBoolAttribute",
                vec![
                    elem,
                    self.ast()
                        .expression_string_literal(span, self.ast().atom(&name), None),
                    value,
                ],
            );
        }

        let is_child_prop = child_properties(&name);
        let property_state = dom_with_state(&options.tag_name, &name);
        let is_locked = matches!(
            property_state,
            Some(crate::shared::constants::DomPropertyState::Locked)
        );
        let is_custom_element = options.tag_name.contains('-');
        if is_custom_element
            && !is_child_prop
            && namespace.as_deref() != Some("prop")
            && namespace.as_deref() != Some("attr")
        {
            name = custom_element_property_name(&name);
        }
        if let Some(alias) = prop_alias(&name, &options.tag_name) {
            name = alias.to_string();
        }

        if namespace.as_deref() != Some("attr")
            && (is_child_prop
                || (!svg_elements(&options.tag_name) && dom_properties(&name))
                || is_custom_element
                || namespace.as_deref() == Some("prop")
                || property_state.is_some())
        {
            if self.hydratable && namespace.as_deref() != Some("prop") && !is_locked {
                self.template_state.uses_set_property = true;
                return self.call_identifier(
                    span,
                    "_$setProperty",
                    vec![
                        elem,
                        self.ast()
                            .expression_string_literal(span, self.ast().atom(&name), None),
                        value,
                    ],
                );
            }

            return self.member_assignment_expression(span, elem, &name, value);
        }

        if let Some(ns) = name
            .split_once(':')
            .and_then(|(prefix, _)| namespaces(prefix))
        {
            self.template_state.uses_set_attribute_ns = true;
            return self.call_identifier(
                span,
                "_$setAttributeNS",
                vec![
                    elem,
                    self.ast()
                        .expression_string_literal(span, self.ast().atom(ns), None),
                    self.ast()
                        .expression_string_literal(span, self.ast().atom(&name), None),
                    value,
                ],
            );
        }

        name = match name.as_str() {
            "className" => "class".to_string(),
            "htmlFor" => "for".to_string(),
            _ if !svg_elements(&options.tag_name) => name.to_ascii_lowercase(),
            _ => name,
        };

        self.template_state.uses_set_attribute = true;
        self.call_identifier(
            span,
            "_$setAttribute",
            vec![
                elem,
                self.ast()
                    .expression_string_literal(span, self.ast().atom(&name), None),
                value,
            ],
        )
    }

    pub(crate) fn double_negation(&self, span: Span, value: Expression<'a>) -> Expression<'a> {
        self.ast().expression_unary(
            span,
            UnaryOperator::LogicalNot,
            self.ast()
                .expression_unary(span, UnaryOperator::LogicalNot, value),
        )
    }

    fn member_assignment(
        &self,
        span: Span,
        object: Expression<'a>,
        property: &str,
        value: Expression<'a>,
    ) -> Expression<'a> {
        self.member_assignment_expression(span, object, property, value)
    }

    /// `<object>.<property> = <value>`
    pub(crate) fn member_assignment_expression(
        &self,
        span: Span,
        object: Expression<'a>,
        property: &str,
        value: Expression<'a>,
    ) -> Expression<'a> {
        let target =
            AssignmentTarget::StaticMemberExpression(self.ast().alloc_static_member_expression(
                span,
                object,
                self.ast().identifier_name(span, self.ast().ident(property)),
                false,
            ));
        self.ast()
            .expression_assignment(span, AssignmentOperator::Assign, target, value)
    }
}

fn custom_element_property_name(name: &str) -> String {
    let mut result = String::new();
    let mut uppercase_next = false;
    for character in name.to_ascii_lowercase().chars() {
        if character == '-' {
            uppercase_next = true;
        } else if uppercase_next {
            result.extend(character.to_uppercase());
            uppercase_next = false;
        } else {
            result.push(character);
        }
    }
    result
}
