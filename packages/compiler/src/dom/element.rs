use crate::error::Result;
use oxc_allocator::{Allocator, CloneIn};
use oxc_ast::ast::{
    AssignmentOperator, AssignmentTarget, Expression, JSXElement, JSXExpression, Statement,
};
use oxc_span::GetSpan;

use crate::dom::attrs::CloseTagContext;
use crate::dom::template::DomTemplateState;
use crate::shared::bindings::BindingTable;
use crate::shared::component::lower_component_with_setup;
use crate::shared::utils::{element_name, is_component_name, static_jsx_expression, StaticValue};

pub(crate) struct AstDomTransform<'a, 'source> {
    pub(crate) allocator: &'a Allocator,
    pub(crate) source: &'source str,
    pub(crate) module_name: &'source str,
    pub(crate) hydratable: bool,
    pub(crate) dev: bool,
    pub(crate) context_to_custom_elements: bool,
    pub(crate) omit_server_only_templates: bool,
    pub(crate) delegate_events: bool,
    pub(crate) delegated_events: std::vec::Vec<String>,
    pub(crate) omit_quotes: bool,
    pub(crate) omit_attribute_spacing: bool,
    pub(crate) inline_styles: bool,
    /// The reactive wrapper import name (`effect` by default); `None`
    /// disables effect wrapping (Babel's falsy `effectWrapper`).
    pub(crate) effect_wrapper: Option<String>,
    pub(crate) wrap_conditionals: bool,
    /// The memo wrapper import name (`memo` by default); `None` disables
    /// memo wrapping (Babel's falsy `memoWrapper`).
    pub(crate) memo_wrapper: Option<String>,
    pub(crate) static_marker: String,
    pub(crate) omit_nested_closing_tags: bool,
    pub(crate) omit_last_closing_tag: bool,
    /// Babel's `validate` (default on): warn when a template's markup would
    /// be restructured by the browser's HTML parser.
    pub(crate) validate: bool,
    pub(crate) built_ins: std::vec::Vec<String>,
    /// Where the reactive wrapper helpers (`memo`, `effect`) import from.
    /// Babel resolves them against the top-level module — in dynamic
    /// (universal + dom renderer) mode that's the base universal module, not
    /// the dom renderer's module.
    pub(crate) wrapper_module_name: Option<String>,
    /// Dynamic mode: the tags this renderer owns. Native elements outside the
    /// list are left as raw JSX for the driving universal transform to lower
    /// (Babel dispatches per element through `transformElement`). `None`
    /// (plain dom mode) claims every native tag.
    pub(crate) renderer_elements: Option<std::vec::Vec<String>>,
    pub(crate) template_state: DomTemplateState,
    pub(crate) error: Option<String>,
    pub(crate) bindings: BindingTable,
    pub(crate) pending_this_capture: Option<String>,
    pub(crate) current_this_capture: Option<String>,
    pub(crate) function_parent_stack: std::vec::Vec<crate::shared::transform::FunctionParentKind>,
    pub(crate) next_function_class_method: bool,
    pub(crate) lowered_function_bodies: std::collections::HashSet<usize>,
    pub(crate) statement_depth: usize,
    pub(crate) skip_xmlns_attribute: bool,
    /// After a hydration `getNextMarker` destructure, positional child walks
    /// in the same parent chain from the marker's end node — the SSR'd DOM
    /// holds arbitrary content between `<!$>` and `<!/>`, so root-relative
    /// `firstChild.nextSibling…` paths would land inside the marker region.
    /// `(end node identifier, template index of the end node)`.
    pub(crate) hydration_walk_anchor: Option<(String, usize)>,
    /// Babel's `tempPath`: the last declared positional walk variable of the
    /// current parent and its template index. Dev-mode validated walks
    /// (`getFirstChild`/`getNextSibling`) chain from it by name — the plain
    /// member walks re-derive from the root instead (equalized by traversal).
    pub(crate) last_child_walk: Option<(String, usize)>,
    /// Whether the current template root saw a delegated event handler or a
    /// spread (which may carry one); consumed at the root to emit a single
    /// `runHydrationEvents()` after setup.
    pub(crate) has_hydratable_event: bool,
    pub(crate) element_index: usize,
    pub(crate) this_index: usize,
    pub(crate) ref_index: usize,
    pub(crate) value_index: usize,
    pub(crate) condition_index: usize,
    /// Span of the JSX root currently being lowered via the visitor entry.
    /// Babel keeps a raw `this` in the tag callee of the root element of each
    /// `transformJSX` call; only descendants use the `_self$` capture.
    pub(crate) jsx_root_span: Option<oxc_span::Span>,
    /// Disabled for ordinary `transform()` runs, so tracing cannot influence
    /// generated output.
    pub(crate) semantic_trace: crate::semantic_trace::TraceRecorder,
}

pub(crate) struct DomTransformConfig {
    pub(crate) hydratable: bool,
    pub(crate) dev: bool,
    pub(crate) context_to_custom_elements: bool,
    pub(crate) omit_server_only_templates: bool,
    pub(crate) delegate_events: bool,
    pub(crate) delegated_events: std::vec::Vec<String>,
    pub(crate) omit_quotes: bool,
    pub(crate) omit_attribute_spacing: bool,
    pub(crate) inline_styles: bool,
    pub(crate) effect_wrapper: Option<String>,
    pub(crate) wrap_conditionals: bool,
    pub(crate) memo_wrapper: Option<String>,
    pub(crate) static_marker: String,
    pub(crate) omit_nested_closing_tags: bool,
    pub(crate) omit_last_closing_tag: bool,
    pub(crate) validate: bool,
    pub(crate) built_ins: std::vec::Vec<String>,
    pub(crate) wrapper_module_name: Option<String>,
    pub(crate) renderer_elements: Option<std::vec::Vec<String>>,
}

impl<'a, 'source> AstDomTransform<'a, 'source> {
    /// Local for the configured effect wrapper (Babel's `_$${name}` hint).
    pub(crate) fn effect_wrapper_local(&self) -> String {
        format!("_${}", self.effect_wrapper.as_deref().unwrap_or("effect"))
    }

    /// Local for the configured memo wrapper.
    pub(crate) fn memo_wrapper_local(&self) -> String {
        format!("_${}", self.memo_wrapper.as_deref().unwrap_or("memo"))
    }

    pub(crate) fn new(
        allocator: &'a Allocator,
        source: &'source str,
        module_name: &'source str,
        config: DomTransformConfig,
    ) -> Self {
        Self {
            allocator,
            source,
            module_name,
            hydratable: config.hydratable,
            dev: config.dev,
            context_to_custom_elements: config.context_to_custom_elements,
            omit_server_only_templates: config.omit_server_only_templates,
            delegate_events: config.delegate_events,
            delegated_events: config.delegated_events,
            omit_quotes: config.omit_quotes,
            omit_attribute_spacing: config.omit_attribute_spacing,
            inline_styles: config.inline_styles,
            effect_wrapper: config.effect_wrapper,
            wrap_conditionals: config.wrap_conditionals,
            memo_wrapper: config.memo_wrapper,
            static_marker: config.static_marker,
            omit_nested_closing_tags: config.omit_nested_closing_tags,
            omit_last_closing_tag: config.omit_last_closing_tag,
            validate: config.validate,
            built_ins: config.built_ins,
            wrapper_module_name: config.wrapper_module_name,
            renderer_elements: config.renderer_elements,
            template_state: DomTemplateState::new(),
            error: None,
            bindings: BindingTable::default(),
            pending_this_capture: None,
            current_this_capture: None,
            function_parent_stack: std::vec::Vec::new(),
            next_function_class_method: false,
            lowered_function_bodies: std::collections::HashSet::new(),
            statement_depth: 0,
            skip_xmlns_attribute: false,
            semantic_trace: crate::semantic_trace::TraceRecorder::disabled(),
            hydration_walk_anchor: None,
            last_child_walk: None,
            has_hydratable_event: false,
            element_index: 0,
            this_index: 0,
            ref_index: 0,
            value_index: 0,
            condition_index: 0,
            jsx_root_span: None,
        }
    }

    /// Whether a native element belongs to a different renderer in dynamic
    /// mode: the dom transform leaves it as raw JSX for the universal
    /// transform to lower.
    pub(crate) fn is_foreign_element(&self, element: &JSXElement<'a>) -> bool {
        let Some(elements) = &self.renderer_elements else {
            return false;
        };
        if is_component_name(&element.opening_element.name) {
            return false;
        }
        match element_name(&element.opening_element.name) {
            Ok(tag_name) => !elements.iter().any(|name| name == &tag_name),
            Err(_) => false,
        }
    }

    pub(crate) fn lower_element(&mut self, element: &JSXElement<'a>) -> Result<Expression<'a>> {
        let (result, setup) = self.lower_element_with_setup(element)?;
        if setup.is_empty() {
            return Ok(result);
        }

        let mut statements = self.ast().vec();
        statements.extend(setup);
        statements.push(self.ast().statement_return(element.span, Some(result)));
        let arrow = self.arrow_iife(element.span, statements);
        Ok(self.call_expression(element.span, arrow, std::vec::Vec::new()))
    }

    pub(crate) fn lower_element_with_setup(
        &mut self,
        element: &JSXElement<'a>,
    ) -> Result<(Expression<'a>, std::vec::Vec<Statement<'a>>)> {
        // Dynamic mode: another renderer's element stays raw JSX; the driving
        // universal transform lowers it after this subtree returns.
        if self.is_foreign_element(element) {
            return Ok((
                Expression::JSXElement(oxc_allocator::Box::new_in(
                    oxc_allocator::CloneIn::clone_in(element, self.allocator),
                    self.allocator,
                )),
                std::vec::Vec::new(),
            ));
        }
        if is_component_name(&element.opening_element.name) {
            return lower_component_with_setup(self, element);
        }

        let tag_name = element_name(&element.opening_element.name)?;
        if self.hydratable && tag_name == "head" {
            // Babel's `transformElement` returns here too, before
            // `transformAttributes` and the child recursion, so neither
            // compiler lowers anything inside the element (exact parity in all
            // four dom modes for a root `<head>{b()}</head>`). Withdraw the
            // censused sites inside it: the element becomes a bare
            // `NoHydration` call, so nothing there executes to decide about.
            // The element's own span is left alone — where a parent lowering
            // decided the `<head>` as a child of its own, that decision stands.
            self.retract_discarded_element_sites(element);
            self.template_state.uses_create_component = true;
            if !self
                .template_state
                .built_in_imports
                .iter()
                .any(|name| name == "NoHydration")
            {
                self.template_state
                    .built_in_imports
                    .push("NoHydration".to_string());
            }
            return Ok((
                self.call_expression(
                    element.span,
                    self.ast()
                        .expression_identifier(element.span, self.ast().ident("_$createComponent")),
                    vec![
                        self.ast()
                            .expression_identifier(element.span, self.ast().ident("_$NoHydration")),
                        self.ast().expression_object(element.span, self.ast().vec()),
                    ],
                ),
                std::vec::Vec::new(),
            ));
        }

        // Each native template root replays hydratable events independently
        // (Babel transforms component children with `topLevel: true`).
        let saved_hydratable_event = self.has_hydratable_event;
        self.has_hydratable_event = false;

        // A non-literal `children` attribute participates in child insertion
        // rather than attribute handling (Babel parity): when the element has
        // no real children, the value becomes its child expression; when it
        // does, the attribute is dropped.
        //
        // `promoted_children_attribute` owns which `children` attribute the
        // slot ends up holding and why. What is decided here is only whether
        // that value is ever *visited*: Babel pushes it onto the child list in
        // `transformAttributes` and then guards the recursion with
        // `if (!voidTag) { … if (tagName !== "noscript") transformChildren(…) }`,
        // so a void element's or a `<noscript>`'s promoted value emits nothing
        // at all. Without those two gates `<br children={x()}/>` emitted an
        // `_$insert` Babel does not.
        //
        // The gates cover the promotion only. A void or `<noscript>` template
        // root still lowers its *source* children here, which Babel also
        // discards — divergence 2 in docs/execution-contract.md.
        let attribute_child =
            (element.children.is_empty()
                && !crate::shared::utils::is_void_element(&tag_name)
                && tag_name != "noscript"
                && !element.opening_element.attributes.iter().any(|attr| {
                    matches!(attr, oxc_ast::ast::JSXAttributeItem::SpreadAttribute(_))
                }))
            .then(|| self.promoted_children_attribute(element))
            .flatten()
            .map(|(_, container)| container);
        // Which `children` value was promoted, so the attribute loop can tell
        // it apart from sibling `children` attributes the promotion dropped.
        let promoted_children_span = attribute_child.map(|container| container.expression.span());
        let element: &JSXElement<'a> = if let Some(container) = attribute_child {
            let mut clone = element.clone_in(self.allocator);
            clone
                .children
                .push(oxc_ast::ast::JSXChild::ExpressionContainer(
                    oxc_allocator::Box::new_in(container.clone_in(self.allocator), self.allocator),
                ));
            self.allocator.alloc(clone)
        } else {
            element
        };

        // XML partial handling (Babel parity): template-root SVG/MathML
        // elements other than <svg>/<math> themselves get wrapped in their
        // owner tag and flagged, and the `xmlns` attribute (only needed to
        // detect the namespace) is dropped from the template.
        let wrapper_tag = self.xml_wrapper_tag(element, &tag_name);
        let skip_xmlns = wrapper_tag.is_some();

        let mut template = crate::dom::template::TemplateHtml::open_tag(&tag_name);
        let mut declarations = std::vec::Vec::new();
        let mut operations = std::vec::Vec::new();
        let mut dynamics = std::vec::Vec::new();
        let element_id = self.next_element_id();

        let saved_skip_xmlns = self.skip_xmlns_attribute;
        self.skip_xmlns_attribute = skip_xmlns;
        // Attributes only land in the emitted markup — Babel leaves
        // `templateWithClosingTags` attribute-free (solidjs/solid#2338).
        let attribute_result = self.lower_template_attributes(
            &element.opening_element.attributes,
            &tag_name,
            false,
            &element_id,
            !element.children.is_empty(),
            promoted_children_span,
            &mut template.html,
            &mut declarations,
            &mut operations,
            &mut dynamics,
        );
        self.skip_xmlns_attribute = saved_skip_xmlns;
        let attrs_lowering = attribute_result?;
        let needs_text_placeholder = attrs_lowering.text_placeholder.is_some();

        // Babel's textarea `value` fold replaces the element's children
        // (`path.node.children = [child]`).
        let element: &JSXElement<'a> = match attrs_lowering.children_replacement {
            Some(child) => {
                let mut clone = element.clone_in(self.allocator);
                clone.children.clear();
                clone.children.push(child);
                self.allocator.alloc(clone)
            }
            None => element,
        };

        // Babel pushes the custom-element owner-context assignment right
        // after the attribute expressions, before child inserts.
        let needs_custom_element_context =
            self.should_capture_custom_element_context(element, &tag_name);
        if needs_custom_element_context {
            let statement = self.custom_element_context_statement(element.span, &element_id);
            operations.push(statement);
        }

        template.push_both(">");
        if needs_text_placeholder && element.children.is_empty() {
            // Dynamic `textContent` adds a single space text node the effect
            // writes into — but only when the element has no children of its
            // own (Babel's `!hasChildren` gate; with children the `firstChild`
            // declaration still emits and the children compile normally).
            // Attribute-driven, so like attributes it stays out of `closed`.
            template.html.push(' ');
        } else {
            self.lower_dom_children(
                element,
                &tag_name,
                &element_id,
                CloseTagContext::root(),
                &mut template,
                &mut declarations,
                &mut operations,
                &mut dynamics,
            )?;
        }
        // All dynamic attribute bindings collected across this template root
        // batch into one effect, appended after the other expressions.
        let mut post_dynamics = std::vec::Vec::new();
        let mut regular_dynamics = std::vec::Vec::new();
        for slot in dynamics {
            let state_key = slot.key.strip_prefix("prop:").unwrap_or(&slot.key);
            let stateful = matches!(
                crate::shared::constants::dom_with_state(&slot.tag_name, state_key),
                Some(crate::shared::constants::DomPropertyState::Stateful)
            );
            if (!matches!(
                slot.tag_name.to_ascii_uppercase().as_str(),
                "VIDEO" | "AUDIO"
            ) && stateful)
                || (slot.tag_name.contains('-') && slot.key == "value")
            {
                post_dynamics.push(slot);
            } else {
                regular_dynamics.push(slot);
            }
        }
        if let Some(statement) = self.wrap_dynamics_statement(regular_dynamics) {
            operations.push(statement);
        }
        post_dynamics.sort_by_key(|slot| slot.elem == element_id);
        for slot in post_dynamics {
            if let Some(statement) = self.wrap_dynamics_statement(vec![slot]) {
                operations.push(statement);
            }
        }
        if self.should_close_tag(&tag_name, CloseTagContext::root()) {
            template.html.push_str(&format!("</{tag_name}>"));
        }
        if !crate::shared::utils::is_void_element(&tag_name) {
            template.closed.push_str(&format!("</{tag_name}>"));
        }
        if let Some(wrapper) = wrapper_tag {
            template.html = format!("<{wrapper}>{}</{wrapper}>", template.html);
            template.closed = format!("<{wrapper}>{}</{wrapper}>", template.closed);
        }

        let template_flag = if wrapper_tag == Some("svg") {
            Some(2)
        } else if wrapper_tag == Some("math")
            || crate::shared::constants::mathml_elements(&tag_name)
        {
            Some(3)
        } else if self.template_subtree_is_import_node(element) {
            Some(1)
        } else {
            None
        };
        // Babel's `skipTemplate`: `$ServerOnly` elements and document shells
        // (`html`/`head`/`body`) never render client-side markup — the element
        // is only recovered from the hydration walk. `omitServerOnlyTemplates`
        // opts out of the `$ServerOnly` half only; the document shells keep
        // their own unconditional `skipTemplate` in `dom/element.js`.
        let skip_template = self.hydratable
            && ((self.omit_server_only_templates && has_attribute_named(element, "$ServerOnly"))
                || matches!(tag_name.as_str(), "html" | "head" | "body"));
        let template_id = if skip_template {
            None
        } else {
            Some(self.template_id_with_options(template, template_flag))
        };
        let has_hydratable_event = self.has_hydratable_event;
        self.has_hydratable_event = saved_hydratable_event;

        if declarations.is_empty() && operations.is_empty() && !has_hydratable_event {
            Ok((
                self.template_call(element.span, template_id.as_deref()),
                std::vec::Vec::new(),
            ))
        } else {
            let init = self.template_call(element.span, template_id.as_deref());
            let mut setup = std::vec::Vec::new();
            setup.push(self.variable_statement(element.span, &element_id, init));
            // Babel hoists all positional walk declarations ahead of the
            // effectful statements (attribute setters, inserts), so walks are
            // resolved before inserts mutate sibling positions.
            setup.extend(declarations);
            setup.extend(operations);
            if has_hydratable_event {
                self.template_state.uses_run_hydration_events = true;
                setup.push(self.ast().statement_expression(
                    element.span,
                    self.call_identifier(
                        element.span,
                        "_$runHydrationEvents",
                        std::vec::Vec::new(),
                    ),
                ));
            }
            Ok((self.identifier_expression(element.span, &element_id), setup))
        }
    }

    fn template_call(&mut self, span: oxc_span::Span, template_id: Option<&str>) -> Expression<'a> {
        if self.hydratable {
            self.template_state.uses_get_next_element = true;
            let args = match template_id {
                Some(template_id) => vec![self.identifier_expression(span, template_id)],
                None => std::vec::Vec::new(),
            };
            self.call_identifier(span, "_$getNextElement", args)
        } else {
            let template_id = template_id.expect("non-hydratable templates are always registered");
            self.call_identifier(span, template_id, std::vec::Vec::new())
        }
    }

    fn should_capture_custom_element_context(
        &self,
        element: &JSXElement<'a>,
        tag_name: &str,
    ) -> bool {
        self.context_to_custom_elements
            && (tag_name == "slot" || self.has_custom_element_marker(element, tag_name))
    }

    fn has_custom_element_marker(&self, element: &JSXElement<'a>, tag_name: &str) -> bool {
        tag_name.contains('-') || has_attribute_named(element, "is")
    }

    /// Owner tag (`svg` / `math`) for a template-root XML partial, detected
    /// by element name or an explicit `xmlns` attribute, mirroring the Babel
    /// plugin's top-level XML handling.
    fn xml_wrapper_tag(&self, element: &JSXElement<'a>, tag_name: &str) -> Option<&'static str> {
        if tag_name == "svg" || tag_name == "math" {
            return None;
        }
        let xmlns = xmlns_attribute_value(element);
        if crate::shared::constants::svg_elements(tag_name)
            || xmlns.as_deref() == Some("http://www.w3.org/2000/svg")
        {
            return Some("svg");
        }
        // Solid 1.x's template helper accepts the MathML flag directly; it
        // does not require the temporary `<math>` wrapper used by SVG.
        if crate::shared::constants::mathml_elements(tag_name)
            || xmlns.as_deref() == Some("http://www.w3.org/1998/Math/MathML")
        {
            return None;
        }
        None
    }

    /// Whether any native element in the template's subtree requires
    /// `importNode` cloning (custom elements, `is` attributes, or lazy-loading
    /// img/iframe). Component subtrees produce their own templates and are
    /// not descended into.
    fn template_subtree_is_import_node(&self, element: &JSXElement<'a>) -> bool {
        if is_component_name(&element.opening_element.name) {
            return false;
        }
        let Ok(tag_name) = element_name(&element.opening_element.name) else {
            return false;
        };
        if self.has_custom_element_marker(element, &tag_name)
            || ((tag_name == "img" || tag_name == "iframe")
                && has_attribute_named(element, "loading"))
        {
            return true;
        }
        element.children.iter().any(|child| {
            matches!(
                child,
                oxc_ast::ast::JSXChild::Element(child)
                    if self.template_subtree_is_import_node(child)
            )
        })
    }

    fn custom_element_context_statement(
        &mut self,
        span: oxc_span::Span,
        element_id: &str,
    ) -> Statement<'a> {
        self.template_state.uses_get_owner = true;
        let target = AssignmentTarget::StaticMemberExpression(
            self.ast().alloc_static_member_expression(
                span,
                self.identifier_expression(span, element_id),
                self.ast()
                    .identifier_name(span, self.ast().ident("_$owner")),
                false,
            ),
        );
        let value = self.call_identifier(span, "_$getOwner", std::vec::Vec::new());
        self.ast().statement_expression(
            span,
            self.ast()
                .expression_assignment(span, AssignmentOperator::Assign, target, value),
        )
    }
}

fn has_attribute_named(element: &JSXElement<'_>, attribute_name: &str) -> bool {
    element.opening_element.attributes.iter().any(|attr| {
        matches!(
            attr,
            oxc_ast::ast::JSXAttributeItem::Attribute(attribute)
                if matches!(
                    &attribute.name,
                    oxc_ast::ast::JSXAttributeName::Identifier(name)
                        if name.name == attribute_name
                )
        )
    })
}

/// Static string value of an element's `xmlns` attribute, if present.
fn xmlns_attribute_value(element: &JSXElement<'_>) -> Option<String> {
    element.opening_element.attributes.iter().find_map(|attr| {
        let oxc_ast::ast::JSXAttributeItem::Attribute(attr) = attr else {
            return None;
        };
        let oxc_ast::ast::JSXAttributeName::Identifier(name) = &attr.name else {
            return None;
        };
        if name.name != "xmlns" {
            return None;
        }
        match &attr.value {
            Some(oxc_ast::ast::JSXAttributeValue::StringLiteral(value)) => {
                Some(value.value.to_string())
            }
            Some(oxc_ast::ast::JSXAttributeValue::ExpressionContainer(container)) => {
                match &container.expression {
                    JSXExpression::StringLiteral(value) => Some(value.value.to_string()),
                    _ => None,
                }
            }
            _ => None,
        }
    })
}

/// Whether this attribute is a `children` attribute whose value reaches Babel's
/// capture — the attribute loop's own gate, which is
/// `t.isJSXExpressionContainer(value) && !(t.isStringLiteral(value.expression)
/// || t.isNumericLiteral(value.expression))`. A string- or number-valued
/// `children` is an ordinary `ChildProperties` write and never touches the
/// slot; every other container value does.
fn children_attribute_container_from_item<'e, 'a>(
    attr: &'e oxc_ast::ast::JSXAttributeItem<'a>,
) -> Option<&'e oxc_ast::ast::JSXExpressionContainer<'a>> {
    let oxc_ast::ast::JSXAttributeItem::Attribute(attr) = attr else {
        return None;
    };
    let oxc_ast::ast::JSXAttributeName::Identifier(name) = &attr.name else {
        return None;
    };
    if name.name != "children" {
        return None;
    }
    let Some(oxc_ast::ast::JSXAttributeValue::ExpressionContainer(container)) = &attr.value else {
        return None;
    };
    if matches!(
        container.expression,
        JSXExpression::StringLiteral(_)
            | JSXExpression::NumericLiteral(_)
            | JSXExpression::EmptyExpression(_)
    ) {
        return None;
    }
    Some(container)
}

pub(crate) fn jsx_expression_to_expression<'a>(
    expression: &JSXExpression<'a>,
    allocator: &'a Allocator,
) -> Expression<'a> {
    expression.clone_in(allocator).into_expression()
}

impl<'a> AstDomTransform<'a, '_> {
    /// The `children` attribute that holds Babel's single `children` slot when
    /// the attribute loop finishes, with its index in the attribute list.
    ///
    /// Solid 1.x's plugin does not deduplicate attributes by name (the
    /// `dedupe_attributes` shim is deliberately the identity here): every
    /// attribute is visited in source order, and only the ones that reach
    /// `else if (key === "children") children = value` write the slot. That
    /// branch sits behind
    /// `t.isJSXExpressionContainer(value) && !(isStringLiteral || isNumericLiteral)`,
    /// so a literal-valued `children` never touches the slot at all — it falls
    /// through to the `ChildProperties` write (`_el$.children = "s"`) and
    /// leaves an *earlier* non-literal `children` as the promoted value.
    /// `<div children={x()} children={"s"}/>` therefore emits both, which is
    /// why the scan must keep looking past a literal rather than stop at the
    /// last attribute named `children`.
    ///
    /// Literal-ness is judged after constant folding, because `transformElement`
    /// runs `evaluateAndInline` over every attribute value before the loop:
    /// `children={"a" + "b"}` is already a string literal by the time the
    /// capture could see it. Applying that filter to the scan's *result*
    /// instead of to each candidate is what made
    /// `<div children={x()} children={"a" + "b"}/>` drop the promotion
    /// entirely.
    ///
    /// The index is returned because the slot has a second writer — a dynamic
    /// `textContent` — and which one survives depends on their relative source
    /// position; see [`AttrsLowering::text_placeholder`].
    ///
    /// Selection and lowerability are two separate steps here, and the order
    /// matters. The scan stops at the attribute *Babel's* capture keeps, then
    /// asks whether this compiler lowers that value; it never falls back to an
    /// earlier `children` attribute, because Babel's own capture already
    /// overwrote the slot with the later one. Falling back turned
    /// `<div children={x()} children={null}/>` — where Babel inserts `null` —
    /// into an insert of `x()`, which is worse than the missing insert it
    /// replaced. A confidently-folded non-text value (`null`, `undefined`, a
    /// boolean, a confident object literal) is such a capture: it is selected
    /// and then dropped, which is divergence 3 in docs/execution-contract.md,
    /// not a reason to promote a different value.
    pub(crate) fn promoted_children_attribute<'e>(
        &self,
        element: &'e JSXElement<'a>,
    ) -> Option<(usize, &'e oxc_ast::ast::JSXExpressionContainer<'a>)> {
        let (index, container) = element
            .opening_element
            .attributes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, attr)| {
                let container = children_attribute_container_from_item(attr)?;
                // `evaluateAndInline` has already rewritten a foldable value to
                // a literal by the time Babel's gate above sees it, so a value
                // that folds to a string or number is one of those literals.
                let folds_to_text =
                    container
                        .expression
                        .as_expression()
                        .is_some_and(|expression| {
                            matches!(
                                self.evaluate_confident(expression),
                                Some(crate::shared::attr_plan::ConfidentValue::Str(_))
                                    | Some(crate::shared::attr_plan::ConfidentValue::Num(_))
                            )
                        });
                (!folds_to_text).then_some((index, container))
            })?;
        container
            .expression
            .as_expression()
            .is_none_or(|expression| self.evaluate_confident(expression).is_none())
            .then_some((index, container))
    }
}

impl<'a> AstDomTransform<'a, '_> {
    /// Clones an attribute's expression value and lowers any JSX nested
    /// inside it (`innerHTML={cond ? <Comp/> : <Other/>}`) — Babel's generic
    /// traversal transforms nested JSX everywhere, so ours must too.
    pub(crate) fn attribute_value_expression(
        &mut self,
        container: &oxc_ast::ast::JSXExpressionContainer<'a>,
    ) -> Expression<'a> {
        // JSX inside stays raw for the deferred pass (Babel's outer
        // traversal lowers it after the root completes).
        jsx_expression_to_expression(&container.expression, self.allocator)
    }
}

impl AstDomTransform<'_, '_> {
    pub(crate) fn static_jsx_expression_value(
        &self,
        expression: &JSXExpression<'_>,
    ) -> Option<String> {
        static_jsx_expression(expression, Some(&self.bindings))
            .map(StaticValue::into_template_value)
    }

    /// The shared classification authority over this transform's bindings,
    /// source, and configured static marker.
    pub(crate) fn classify(&self) -> crate::shared::classify::Classify<'_> {
        crate::shared::classify::Classify::new(&self.bindings, self.source, &self.static_marker)
    }
}
