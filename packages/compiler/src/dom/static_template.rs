use crate::error::{Error, Result};
use oxc_ast::ast::{JSXChild, JSXElement, JSXExpression};
use oxc_span::GetSpan;

use crate::dom::attrs::CloseTagContext;
use crate::dom::element::AstDomTransform;
use crate::shared::utils::{
    element_name, escape_html_text, escape_html_text_expression, is_component_name,
    static_jsx_expression_value, trim_jsx_text,
};

pub(crate) fn lower_static_native_template<'a>(
    ctx: &mut AstDomTransform<'a, '_>,
    element: &JSXElement<'a>,
    close_context: CloseTagContext,
) -> Result<Option<crate::dom::template::TemplateHtml>> {
    if is_component_name(&element.opening_element.name) {
        return Ok(None);
    }

    let tag_name = element_name(&element.opening_element.name)?;

    let mut template = crate::dom::template::TemplateHtml::open_tag(&tag_name);

    // Attributes only land in the emitted markup, not the validation variant.
    let Some(children_replacement) = ctx.try_append_planned_static_attributes(
        &element.opening_element.attributes,
        &tag_name,
        true,
        &mut template.html,
    )?
    else {
        return Ok(None);
    };

    template.push_both(">");

    if tag_name == "noscript" {
        // `<noscript>` markup is inert, so this path emits the tag and returns
        // without visiting the children at all. Babel drops them too — its
        // `transformElement` guards the recursion with
        // `if (tagName !== "noscript") transformChildren(...)`, ungated on the
        // element's position — so retracting their censused sites is
        // parity-clean: nothing is emitted for them, so there is nothing to
        // decide. (Measured against the oracle in all four dom modes for
        // `<div><noscript>{b()}</noscript></div>` and for a `ref` nested in the
        // discarded subtree; both are exact parity.)
        //
        // The two `<noscript>` positions this path does *not* cover keep their
        // sites, because there this compiler really does lower the children:
        // a `<noscript>` template root, and a nested `<noscript>` whose
        // attributes push it onto the dynamic path. Both are divergences from
        // Babel, recorded in docs/execution-contract.md rather than repaired
        // by a trace that under-reports what this compiler emits.
        ctx.retract_children_sites(&element.children);
        if ctx.should_close_tag(&tag_name, close_context.clone()) {
            template.html.push_str(&format!("</{tag_name}>"));
        }
        if !crate::shared::utils::is_void_element(&tag_name) {
            template.closed.push_str(&format!("</{tag_name}>"));
        }
        return Ok(Some(template));
    }

    let child_to_be_closed = ctx.child_close_context(&tag_name, close_context.clone());
    // The textarea `value` fold replaces the element's children with a
    // single synthesized child.
    let children: &[JSXChild<'a>] = match &children_replacement {
        Some(child) => std::slice::from_ref(child),
        None => &element.children,
    };
    let last_element = ctx.find_last_element(children);
    for (index, child) in children.iter().enumerate() {
        match child {
            JSXChild::Text(text) => {
                let text = trim_jsx_text(&text.value);
                if !text.is_empty() {
                    template.push_both(&escape_html_text(&text));
                }
            }
            JSXChild::ExpressionContainer(container) => {
                if matches!(container.expression, JSXExpression::EmptyExpression(_)) {
                    continue;
                }
                let value = ctx
                    .static_jsx_expression_value(&container.expression)
                    .or_else(|| static_jsx_expression_value(&container.expression));
                let Some(value) = value else {
                    return Ok(None);
                };
                ctx.semantic_trace.value(
                    container.expression.span(),
                    crate::semantic_trace::ExecutionSiteKind::JsxChild,
                    crate::semantic_trace::ValueDecision::Elided,
                );
                template.push_both(&escape_html_text_expression(&value));
            }
            JSXChild::Element(child) => {
                // Cross-renderer nesting errors even in fully static subtrees
                // (Babel routes the child through the other renderer's
                // transform first, then throws in `transformChildren`).
                if ctx.is_foreign_element(child) {
                    let child_tag = element_name(&child.opening_element.name)?;
                    return Err(Error::from_reason(format!(
                        "<{child_tag}> is not supported in <{tag_name}>.\n      Wrap the usage with a component that would render this element, eg. Canvas"
                    )));
                }
                let Some(child_template) = lower_static_native_template(
                    ctx,
                    child,
                    CloseTagContext {
                        last_element: Some(index) == last_element,
                        to_be_closed: child_to_be_closed.clone(),
                    },
                )?
                else {
                    return Ok(None);
                };
                template.append(child_template);
            }
            _ => return Ok(None),
        }
    }

    if ctx.should_close_tag(&tag_name, close_context) {
        template.html.push_str(&format!("</{tag_name}>"));
    }
    if !crate::shared::utils::is_void_element(&tag_name) {
        template.closed.push_str(&format!("</{tag_name}>"));
    }

    Ok(Some(template))
}
