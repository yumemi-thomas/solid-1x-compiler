use oxc_allocator::CloneIn;
use oxc_ast::ast::{BinaryOperator, Expression, LogicalOperator, Statement};
use oxc_span::Span;

use crate::dom::element::AstDomTransform;
use crate::dom::set_attr::SetAttrOptions;
use crate::shared::utils::get_numbered_id;

/// One deferred dynamic attribute binding, mirroring Babel's
/// `results.dynamics` entries. Collected across a whole template root and
/// wrapped into a single effect by `wrap_dynamics_statement`.
pub(crate) struct DynamicSlot<'a> {
    pub(crate) span: Span,
    pub(crate) elem: String,
    pub(crate) key: String,
    pub(crate) value: Expression<'a>,
    pub(crate) tag_name: String,
    pub(crate) style_property: bool,
    pub(crate) class_property: bool,
}

impl<'a> AstDomTransform<'a, '_> {
    /// Port of Babel's `wrapDynamics` (dom/template.ts): one dynamic binding
    /// gets its own effect; multiple bindings share a single keyed effect
    /// with a previous-values object.
    pub(crate) fn wrap_dynamics_statement(
        &mut self,
        mut dynamics: std::vec::Vec<DynamicSlot<'a>>,
    ) -> Option<Statement<'a>> {
        if dynamics.is_empty() {
            return None;
        }
        self.template_state.uses_effect = true;

        if dynamics.len() == 1 {
            let slot = dynamics.pop().expect("single dynamic slot exists");
            let span = slot.span;
            let dynamic_style = slot.key.starts_with("style:");
            let use_prev = slot.key == "classList" || slot.key == "style" || dynamic_style;

            let mut value = if slot.class_property
                && !matches!(
                    slot.value,
                    Expression::BooleanLiteral(_) | Expression::UnaryExpression(_)
                ) {
                self.double_negation(span, slot.value)
            } else {
                slot.value
            };

            if dynamic_style {
                value = self.ast().expression_assignment(
                    span,
                    oxc_ast::ast::AssignmentOperator::Assign,
                    oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(
                        self.ast()
                            .alloc_identifier_reference(span, self.ast().ident("_$p")),
                    ),
                    value,
                );
            }
            let elem = self.identifier_expression(span, &slot.elem);
            let prev_id = use_prev.then(|| self.identifier_expression(span, "_$p"));
            let set_attr = self.set_attr_expression(
                span,
                elem,
                &slot.key,
                value,
                SetAttrOptions {
                    dynamic: true,
                    prev_id,
                    tag_name: slot.tag_name,
                    style_property: slot.style_property,
                    class_property: slot.class_property,
                },
            );
            let statement = self.ast().statement_return(span, Some(set_attr));
            let params = if use_prev { vec!["_$p"] } else { vec![] };
            let effect = self.arrow_with_statements(span, params, self.ast().vec1(statement));
            let effect_local = self.effect_wrapper_local();
            return Some(self.ast().statement_expression(
                span,
                self.call_identifier(span, &effect_local, vec![effect]),
            ));
        }

        let span = dynamics
            .first()
            .map_or_else(Span::default, |slot| slot.span);
        let mut initial_props = self.ast().vec();
        let mut value_declarators = self.ast().vec();
        let mut updates = self.ast().vec();

        for (index, slot) in dynamics.into_iter().enumerate() {
            let prop_name = get_numbered_id(index);
            let slot_span = slot.span;
            let value_name = self.next_value_id();

            let value = if slot.class_property
                && !matches!(
                    slot.value,
                    Expression::BooleanLiteral(_) | Expression::UnaryExpression(_)
                ) {
                self.double_negation(slot_span, slot.value)
            } else {
                slot.value
            };
            value_declarators.push(
                self.ast().variable_declarator(
                    slot_span,
                    oxc_ast::ast::VariableDeclarationKind::Var,
                    self.ast().binding_pattern_binding_identifier(
                        slot_span,
                        self.ast().ident(&value_name),
                    ),
                    oxc_ast::NONE,
                    Some(value),
                    false,
                ),
            );
            initial_props.push(self.object_property(
                slot_span,
                &prop_name,
                self.identifier_expression(slot_span, "undefined"),
            ));

            let elem = self.identifier_expression(slot_span, &slot.elem);
            let value_ident = self.identifier_expression(slot_span, &value_name);
            let prev_member = self.static_member_expression(slot_span, "_p$", &prop_name);

            if slot.key == "classList" || slot.key == "style" {
                let set_attr = self.set_attr_expression(
                    slot_span,
                    elem,
                    &slot.key,
                    value_ident,
                    SetAttrOptions {
                        dynamic: true,
                        prev_id: Some(prev_member.clone_in(self.allocator)),
                        tag_name: slot.tag_name,
                        style_property: slot.style_property,
                        class_property: slot.class_property,
                    },
                );
                let assignment = self.assign_prev_member(slot_span, &prop_name, set_attr);
                updates.push(self.ast().statement_expression(slot_span, assignment));
            } else {
                let changed = self.ast().expression_binary(
                    slot_span,
                    value_ident.clone_in(self.allocator),
                    BinaryOperator::StrictInequality,
                    prev_member,
                );
                let assigned = self.assign_prev_member(
                    slot_span,
                    &prop_name,
                    value_ident.clone_in(self.allocator),
                );
                let set_attr = self.set_attr_expression(
                    slot_span,
                    elem,
                    &slot.key,
                    assigned,
                    SetAttrOptions {
                        dynamic: true,
                        prev_id: slot
                            .style_property
                            .then(|| value_ident.clone_in(self.allocator)),
                        tag_name: slot.tag_name,
                        style_property: slot.style_property,
                        class_property: slot.class_property,
                    },
                );
                updates.push(self.ast().statement_expression(
                    slot_span,
                    self.ast().expression_logical(
                        slot_span,
                        changed,
                        LogicalOperator::And,
                        set_attr,
                    ),
                ));
            }
        }

        let mut declarations = self.ast().vec1(Statement::VariableDeclaration(
            self.ast().alloc_variable_declaration(
                span,
                oxc_ast::ast::VariableDeclarationKind::Var,
                value_declarators,
                false,
            ),
        ));
        declarations.extend(updates);
        declarations.push(
            self.ast()
                .statement_return(span, Some(self.identifier_expression(span, "_p$"))),
        );
        let effect = self.arrow_with_statements(span, vec!["_p$"], declarations);
        let initial = self.ast().expression_object(span, initial_props);
        let effect_local = self.effect_wrapper_local();
        Some(self.ast().statement_expression(
            span,
            self.call_identifier(span, &effect_local, vec![effect, initial]),
        ))
    }

    fn assign_prev_member(&self, span: Span, name: &str, value: Expression<'a>) -> Expression<'a> {
        let target = oxc_ast::ast::AssignmentTarget::StaticMemberExpression(
            self.ast().alloc_static_member_expression(
                span,
                self.identifier_expression(span, "_p$"),
                self.ast().identifier_name(span, self.ast().ident(name)),
                false,
            ),
        );
        self.ast().expression_assignment(
            span,
            oxc_ast::ast::AssignmentOperator::Assign,
            target,
            value,
        )
    }
}
