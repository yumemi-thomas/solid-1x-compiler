//! Lowering-observed execution facts for Solid 1.x JSX in DOM mode.
//!
//! Two independent producers must agree before a trace exists:
//!
//! - [`ExecutionCensus`] walks the source and enumerates every relevant
//!   original-source site. It is the denominator — "this expression is here
//!   and the compiler owes an answer about it".
//! - [`TraceRecorder`] collects the numerator. Lowering calls
//!   [`TraceRecorder::value`] / [`TraceRecorder::callback`] at the exact point
//!   it decides what to do with a site, so a decision is an observation of the
//!   emitted code rather than a re-derivation of the rule that produced it.
//!
//! [`TraceRecorder::finish`] reconciles the two and fails on an unresolved
//! censused site, on conflicting decisions for one site, and on a decision
//! aimed at a site the census never enumerated. That reconciliation is the
//! point: a lowering rule that changes without its reporting changing with it
//! is a build error, not a silently stale contract.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use oxc_ast::ast::{
    JSXAttributeItem, JSXAttributeName, JSXAttributeValue, JSXChild, JSXElement, JSXExpression,
    JSXFragment, Program,
};
use oxc_ast_visit::Visit;
use oxc_span::{GetSpan, Span};

/// Version of the typed semantic-trace schema. The trace is intentionally
/// strict (`SemanticTrace` rejects unknown fields), so consumers must check
/// this value before decoding or caching the facts.
pub const SEMANTIC_TRACE_VERSION: u32 = 2;

use crate::dom::spread::{spread_routes, SpreadRoute};
use crate::shared::attr_plan::static_style_key;
use crate::shared::bindings::BindingTable;
use crate::shared::classify::Classify;
use crate::shared::utils::{dedupe_attributes, is_component_name, is_literal_only_expression};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan {
    pub start: u32,
    pub end: u32,
}

impl From<Span> for SourceSpan {
    fn from(span: Span) -> Self {
        Self {
            start: span.start,
            end: span.end,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionSiteKind {
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

impl ExecutionSiteKind {
    const fn is_value(self) -> bool {
        matches!(
            self,
            Self::JsxChild
                | Self::NativeAttribute
                | Self::NativeSpread
                | Self::ComponentProperty
                | Self::ComponentSpread
                | Self::ComponentChild
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ValueDecision {
    EagerOnce,
    ReactiveRerun,
    CallerContext,
    Elided,
}

impl ValueDecision {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::EagerOnce => "eager-once",
            Self::ReactiveRerun => "reactive-rerun",
            Self::CallerContext => "caller-context",
            Self::Elided => "elided",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallbackDecision {
    LaterEvent,
    LaterRender,
    RefApply,
}

impl CallbackDecision {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::LaterEvent => "later-event",
            Self::LaterRender => "later-render",
            Self::RefApply => "ref-apply",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalDecision {
    Value(ValueDecision),
    Callback(CallbackDecision),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSite {
    pub span: SourceSpan,
    pub kind: ExecutionSiteKind,
    pub decision: TerminalDecision,
}

/// A wrapper emitted by lowering around a source span.
///
/// This is deliberately an observation, not an ownership or timing claim.
/// `wrapper` preserves the emitted wrapper identity, including a custom name;
/// consumers decide whether that identity is audited by their dialect and map
/// unaudited identities to their own unknown state. `group_id` links source
/// spans that share one keyed wrapper invocation.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerEstablishment {
    pub span: SourceSpan,
    pub wrapper: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<u64>,
}

/// Facts about how JSX source values and callbacks were lowered and will
/// execute, as observed during DOM lowering.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticTrace {
    pub version: u32,
    pub sites: Vec<ExecutionSite>,
    #[serde(default)]
    pub owner_establishments: Vec<OwnerEstablishment>,
}

impl Default for SemanticTrace {
    fn default() -> Self {
        Self {
            version: SEMANTIC_TRACE_VERSION,
            sites: Vec::new(),
            owner_establishments: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SiteKey {
    span: SourceSpan,
    kind: ExecutionSiteKind,
}

pub(crate) struct ExecutionCensus {
    sites: BTreeSet<SiteKey>,
    ignored_literal_spans: BTreeSet<SourceSpan>,
}

impl ExecutionCensus {
    pub(crate) fn from_program(
        program: &Program<'_>,
        source: &str,
        static_marker: &str,
        built_ins: &[String],
        inline_styles: bool,
        hydratable: bool,
    ) -> Self {
        let mut bindings = BindingTable::default();
        bindings.scan_builtin_shadowing(program, built_ins);
        // `Classify` reads only namespace-import locals off the table, so the
        // module's imports are all the census needs to answer `is_dynamic`
        // exactly as lowering does.
        for statement in &program.body {
            if matches!(statement, oxc_ast::ast::Statement::ImportDeclaration(_)) {
                bindings.collect(statement);
            }
        }

        struct CensusVisitor<'a, 'bindings> {
            sites: BTreeSet<SiteKey>,
            ignored_literal_spans: BTreeSet<SourceSpan>,
            built_ins: HashSet<&'a str>,
            bindings: &'bindings BindingTable,
            /// The lowering-side dynamic classifier, so the census reads the
            /// same `spread_routes` verdict `dom::attrs` and `dom::spread` do.
            classify: Classify<'bindings>,
            /// Fragment spans that are a *direct* child of a component
            /// element (or of a fragment that is one) — the only fragments
            /// whose children lower as `ComponentChild`.
            ///
            /// A span set rather than a "nearest enclosing element is a
            /// component" flag: the generic walk descends into attribute
            /// values and nested closures, where a flag would still read
            /// `true` for a fragment that is no syntactic child of the
            /// element's child list at all.
            component_child_fragments: BTreeSet<SourceSpan>,
            inline_styles: bool,
            hydratable: bool,
            /// Value spans the compiler discards wholesale. Nothing inside one
            /// is ever lowered, so nothing inside one is a site — including
            /// JSX nested in the discarded value.
            dropped_ranges: Vec<Span>,
        }

        impl CensusVisitor<'_, '_> {
            /// Strict containment: the dropped value is still a site of its
            /// own (reported `elided`); only what is nested inside it stops
            /// existing.
            fn dropped(&self, span: Span) -> bool {
                self.dropped_ranges.iter().any(|range| {
                    range.start <= span.start
                        && span.end <= range.end
                        && (range.start, range.end) != (span.start, span.end)
                })
            }

            fn push(&mut self, span: Span, kind: ExecutionSiteKind) {
                if span.start < span.end && !self.dropped(span) {
                    self.sites.insert(SiteKey {
                        span: span.into(),
                        kind,
                    });
                }
            }

            /// Literal leaves are recorded so a lowering decision aimed at one
            /// is ignored rather than treated as targeting an uncensused site.
            fn ignore_literal(&mut self, span: Span) {
                if span.start < span.end && !self.dropped(span) {
                    self.ignored_literal_spans.insert(span.into());
                }
            }

            fn attribute_name(name: &JSXAttributeName<'_>) -> String {
                match name {
                    JSXAttributeName::Identifier(name) => name.name.to_string(),
                    JSXAttributeName::NamespacedName(name) => {
                        format!("{}:{}", name.namespace.name, name.name.name)
                    }
                }
            }

            fn native_tag_name<'node, 'ast>(
                element: &'node JSXElement<'ast>,
            ) -> Option<&'node str> {
                match &element.opening_element.name {
                    oxc_ast::ast::JSXElementName::Identifier(name) => Some(name.name.as_str()),
                    oxc_ast::ast::JSXElementName::IdentifierReference(name) => {
                        Some(name.name.as_str())
                    }
                    _ => None,
                }
            }

            /// `class={{...}}` only decomposes per property when every key is
            /// a plain, space-free, colon-free name (Babel's classList split).
            fn class_object_splits(object: &oxc_ast::ast::ObjectExpression<'_>) -> bool {
                object.properties.iter().all(|property| match property {
                    oxc_ast::ast::ObjectPropertyKind::SpreadProperty(_) => false,
                    oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property) => {
                        if property.computed {
                            return false;
                        }
                        match &property.key {
                            oxc_ast::ast::PropertyKey::StringLiteral(key) => {
                                !key.value.contains(' ') && !key.value.contains(':')
                            }
                            _ => true,
                        }
                    }
                })
            }

            /// `class={["a", "b", {...}]}` splits its trailing object only
            /// when no key collides with the leading static classes.
            fn split_class_array_object<'node, 'ast>(
                expression: &'node oxc_ast::ast::Expression<'ast>,
            ) -> Option<&'node oxc_ast::ast::ObjectExpression<'ast>> {
                let oxc_ast::ast::Expression::ArrayExpression(array) = expression else {
                    return None;
                };
                let mut static_classes = Vec::new();
                let mut cursor = 0;
                while let Some(oxc_ast::ast::ArrayExpressionElement::StringLiteral(value)) =
                    array.elements.get(cursor)
                {
                    static_classes.push(value.value.to_string());
                    cursor += 1;
                }
                if static_classes.is_empty() || cursor != array.elements.len().checked_sub(1)? {
                    return None;
                }
                let Some(oxc_ast::ast::ArrayExpressionElement::ObjectExpression(object)) =
                    array.elements.get(cursor)
                else {
                    return None;
                };
                let static_class_set: HashSet<String> = static_classes
                    .iter()
                    .flat_map(|class| class.split_whitespace().map(str::to_string))
                    .collect();
                let conflicting = object.properties.iter().any(|property| match property {
                    oxc_ast::ast::ObjectPropertyKind::SpreadProperty(_) => true,
                    oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property) => {
                        if property.computed {
                            return true;
                        }
                        static_style_key(&property.key).is_none_or(|key| {
                            key.contains(' ')
                                || key.contains(':')
                                || static_class_set.contains(&key)
                        })
                    }
                });
                (!conflicting).then_some(object)
            }

            /// Record the fragments in a component-hosted child list, whose
            /// own children lower as [`ExecutionSiteKind::ComponentChild`].
            fn mark_component_child_fragments(&mut self, children: &[JSXChild<'_>]) {
                for child in children {
                    if let JSXChild::Fragment(fragment) = child {
                        self.component_child_fragments
                            .insert(SourceSpan::from(fragment.span));
                    }
                }
            }

            fn census_child(&mut self, child: &JSXChild<'_>, kind: ExecutionSiteKind) {
                match child {
                    JSXChild::ExpressionContainer(container)
                        if !matches!(container.expression, JSXExpression::EmptyExpression(_)) =>
                    {
                        if container
                            .expression
                            .as_expression()
                            .is_some_and(is_literal_only_expression)
                        {
                            self.ignore_literal(container.expression.span());
                            return;
                        }
                        self.push(container.expression.span(), kind);
                    }
                    JSXChild::Spread(spread) => self.push(spread.expression.span(), kind),
                    _ => {}
                }
            }
        }

        impl<'b> Visit<'b> for CensusVisitor<'_, '_> {
            fn visit_jsx_element(&mut self, element: &JSXElement<'b>) {
                if self
                    .dropped_ranges
                    .iter()
                    .any(|range| range.start <= element.span.start && element.span.end <= range.end)
                {
                    return;
                }
                let component = is_component_name(&element.opening_element.name);
                let native_tag_name = (!component)
                    .then(|| Self::native_tag_name(element))
                    .flatten();
                let has_spread = element
                    .opening_element
                    .attributes
                    .iter()
                    .any(|attribute| matches!(attribute, JSXAttributeItem::SpreadAttribute(_)));
                // Which attributes lowering hands to the ordinary attribute
                // planner rather than merging into the runtime spread object.
                // Read from the same authority `dom::attrs` and `dom::spread`
                // use, so the census cannot decompose a `style`/`classList`
                // object at a granularity lowering never applies.
                let planned_attributes: HashSet<Span> = if component {
                    HashSet::new()
                } else {
                    element
                        .opening_element
                        .attributes
                        .iter()
                        .zip(spread_routes(
                            &element.opening_element.attributes,
                            &self.classify,
                        ))
                        .filter_map(|(item, route)| match (item, route) {
                            (JSXAttributeItem::Attribute(attribute), SpreadRoute::Planned) => {
                                Some(attribute.span)
                            }
                            _ => None,
                        })
                        .collect()
                };
                let control_flow = match &element.opening_element.name {
                    oxc_ast::ast::JSXElementName::IdentifierReference(name) => {
                        self.built_ins.contains(name.name.as_str())
                            && !self.bindings.is_builtin_shadowed(name.span)
                    }
                    _ => false,
                };

                // A component's JSX children shadow a `children` attribute
                // entirely: the value never lowers, so neither it nor any JSX
                // nested inside it is a site.
                let dropped_before = self.dropped_ranges.len();
                if !self.hydratable && native_tag_name.is_some() {
                    for child in &element.children {
                        let JSXChild::Element(child) = child else {
                            continue;
                        };
                        if Self::native_tag_name(child).is_some_and(|name| name == "head") {
                            self.dropped_ranges.push(child.span);
                        }
                    }
                }
                if component && !element.children.is_empty() {
                    for item in &element.opening_element.attributes {
                        let JSXAttributeItem::Attribute(attribute) = item else {
                            continue;
                        };
                        if Self::attribute_name(&attribute.name) != "children" {
                            continue;
                        }
                        if let Some(JSXAttributeValue::ExpressionContainer(container)) =
                            &attribute.value
                        {
                            self.dropped_ranges.push(container.expression.span());
                        }
                    }
                }

                // Components keep every attribute; native elements see the
                // deduped set the attribute planner works from.
                let attributes = if component {
                    element
                        .opening_element
                        .attributes
                        .iter()
                        .collect::<Vec<_>>()
                } else {
                    dedupe_attributes(&element.opening_element.attributes)
                };
                for item in attributes {
                    match item {
                        JSXAttributeItem::SpreadAttribute(spread) => self.push(
                            spread.argument.span(),
                            if component {
                                ExecutionSiteKind::ComponentSpread
                            } else {
                                ExecutionSiteKind::NativeSpread
                            },
                        ),
                        JSXAttributeItem::Attribute(attribute) => {
                            let Some(JSXAttributeValue::ExpressionContainer(container)) =
                                &attribute.value
                            else {
                                continue;
                            };
                            if matches!(container.expression, JSXExpression::EmptyExpression(_)) {
                                continue;
                            }
                            if container
                                .expression
                                .as_expression()
                                .is_some_and(is_literal_only_expression)
                            {
                                self.ignore_literal(container.expression.span());
                                continue;
                            }
                            let name = Self::attribute_name(&attribute.name);
                            if !component && (name == "_hk" || name == "data-hk") {
                                continue;
                            }
                            // Template-root SVG partials drop `xmlns`; it only
                            // signalled the namespace.
                            if !component
                                && name == "xmlns"
                                && native_tag_name.is_some_and(|tag| {
                                    tag != "svg"
                                        && tag != "math"
                                        && crate::shared::constants::svg_elements(tag)
                                })
                            {
                                continue;
                            }
                            // Solid 1.x splits `classList={{...}}` per property
                            // and `style={{...}}` per declaration; `class` is
                            // never decomposed as an object.
                            if planned_attributes.contains(&attribute.span)
                                && (name == "classList" || (name == "style" && self.inline_styles))
                            {
                                if let Some(oxc_ast::ast::Expression::ObjectExpression(object)) =
                                    container.expression.as_expression()
                                {
                                    let object_spread = object.properties.iter().any(|property| {
                                        matches!(
                                            property,
                                            oxc_ast::ast::ObjectPropertyKind::SpreadProperty(_)
                                        )
                                    });
                                    let decomposes = if name == "classList" {
                                        Self::class_object_splits(object)
                                    } else {
                                        !object_spread
                                    };
                                    if decomposes {
                                        // A computed style key keeps the whole
                                        // object as one runtime site.
                                        if name == "style"
                                            && object.properties.iter().any(|property| {
                                                matches!(
                                                    property,
                                                    oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property)
                                                        if property.computed
                                                )
                                            })
                                        {
                                            self.push(
                                                container.expression.span(),
                                                ExecutionSiteKind::NativeAttribute,
                                            );
                                        }
                                        for property in &object.properties {
                                            let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(
                                                property,
                                            ) = property
                                            else {
                                                unreachable!("fixed object checked above");
                                            };
                                            if property.computed {
                                                continue;
                                            }
                                            if is_literal_only_expression(&property.value) {
                                                continue;
                                            }
                                            self.push(
                                                property.value.span(),
                                                ExecutionSiteKind::NativeAttribute,
                                            );
                                        }
                                        continue;
                                    }
                                }
                            }
                            // `class={["a", {...}]}` keeps the static classes on
                            // the original attribute and moves the trailing
                            // object to a second `class` plan of its own.
                            if planned_attributes.contains(&attribute.span) && name == "class" {
                                if let Some(expression) = container.expression.as_expression() {
                                    if let Some(object) = Self::split_class_array_object(expression)
                                    {
                                        self.push(object.span, ExecutionSiteKind::NativeAttribute);
                                    }
                                }
                            }
                            let kind = if name == "ref" {
                                ExecutionSiteKind::Ref
                            } else if !component && name.starts_with("on") {
                                ExecutionSiteKind::EventHandler
                            } else if !component
                                && name == "children"
                                && (has_spread || element.children.is_empty())
                            {
                                // Promoted to a child by `lower_element`, or
                                // carried as `children` through the runtime
                                // spread — a child either way.
                                ExecutionSiteKind::JsxChild
                            } else if component {
                                ExecutionSiteKind::ComponentProperty
                            } else {
                                ExecutionSiteKind::NativeAttribute
                            };
                            self.push(container.expression.span(), kind);
                        }
                    }
                }

                for child in &element.children {
                    match child {
                        JSXChild::ExpressionContainer(container)
                            if !matches!(
                                container.expression,
                                JSXExpression::EmptyExpression(_)
                            ) =>
                        {
                            if container
                                .expression
                                .as_expression()
                                .is_some_and(is_literal_only_expression)
                            {
                                self.ignore_literal(container.expression.span());
                                continue;
                            }
                            let function = matches!(
                                container.expression,
                                JSXExpression::ArrowFunctionExpression(_)
                                    | JSXExpression::FunctionExpression(_)
                            );
                            self.push(
                                container.expression.span(),
                                if component && control_flow && function {
                                    ExecutionSiteKind::ControlFlowRender
                                } else if component {
                                    ExecutionSiteKind::ComponentChild
                                } else {
                                    ExecutionSiteKind::JsxChild
                                },
                            );
                        }
                        JSXChild::Spread(spread) => self.push(
                            spread.expression.span(),
                            if component {
                                ExecutionSiteKind::ComponentChild
                            } else {
                                ExecutionSiteKind::JsxChild
                            },
                        ),
                        _ => {}
                    }
                }

                // Only a fragment in this element's own child list lowers
                // through `component_children`; the recursive walk below also
                // enters attribute values and the closures inside them, whose
                // fragments are plain JSX children of nothing.
                if component {
                    self.mark_component_child_fragments(&element.children);
                }
                oxc_ast_visit::walk::walk_jsx_element(self, element);
                self.dropped_ranges.truncate(dropped_before);
            }

            fn visit_jsx_fragment(&mut self, fragment: &JSXFragment<'b>) {
                let kind = if self
                    .component_child_fragments
                    .contains(&SourceSpan::from(fragment.span))
                {
                    ExecutionSiteKind::ComponentChild
                } else {
                    ExecutionSiteKind::JsxChild
                };
                // `lower_fragment` recurses into a nested fragment with the
                // same site kind, so a fragment directly inside a component
                // child fragment is one too.
                if kind == ExecutionSiteKind::ComponentChild {
                    self.mark_component_child_fragments(&fragment.children);
                }
                for child in &fragment.children {
                    self.census_child(child, kind);
                }
                oxc_ast_visit::walk::walk_jsx_fragment(self, fragment);
            }
        }

        let mut visitor = CensusVisitor {
            sites: BTreeSet::new(),
            ignored_literal_spans: BTreeSet::new(),
            built_ins: built_ins.iter().map(String::as_str).collect(),
            bindings: &bindings,
            classify: Classify::new(&bindings, source, static_marker),
            component_child_fragments: BTreeSet::new(),
            inline_styles,
            hydratable,
            dropped_ranges: Vec::new(),
        };
        visitor.visit_program(program);
        Self {
            sites: visitor.sites,
            ignored_literal_spans: visitor.ignored_literal_spans,
        }
    }
}

#[derive(Default)]
pub(crate) struct TraceRecorder {
    census: Option<ExecutionCensus>,
    decisions: BTreeMap<SiteKey, TerminalDecision>,
    owner_establishments: Vec<OwnerEstablishment>,
    next_group_id: u64,
    suppression_depth: usize,
    error: Option<String>,
}

impl TraceRecorder {
    /// The recorder used by ordinary `transform()` runs: every call is a no-op
    /// so tracing cannot influence generated output.
    pub(crate) fn disabled() -> Self {
        Self::default()
    }

    pub(crate) fn new(census: ExecutionCensus) -> Self {
        Self {
            census: Some(census),
            ..Self::default()
        }
    }

    pub(crate) fn next_group_id(&mut self) -> u64 {
        if !self.is_recording() {
            return 0;
        }
        let group_id = self.next_group_id;
        self.next_group_id = self.next_group_id.wrapping_add(1);
        group_id
    }

    pub(crate) fn is_recording(&self) -> bool {
        self.census.is_some() && self.suppression_depth == 0
    }

    pub(crate) fn suppress(&mut self) {
        self.suppression_depth += 1;
    }

    pub(crate) fn resume(&mut self) {
        self.suppression_depth = self
            .suppression_depth
            .checked_sub(1)
            .expect("semantic trace suppression must be balanced");
    }

    pub(crate) fn owner_establishment(&mut self, span: Span, wrapper: &str, group_id: Option<u64>) {
        if self.is_recording() {
            self.owner_establishments.push(OwnerEstablishment {
                span: span.into(),
                wrapper: wrapper.to_string(),
                group_id,
            });
        }
    }

    pub(crate) fn has_site(&self, span: Span, kind: ExecutionSiteKind) -> bool {
        self.census.as_ref().is_some_and(|census| {
            census.sites.contains(&SiteKey {
                span: span.into(),
                kind,
            })
        })
    }

    /// Resolve a lowered attribute value's censused site, whatever kind the
    /// census guessed for it.
    ///
    /// The census is syntactic and runs first, so it can only name a site
    /// from the attribute's spelling: `on*` becomes an event handler, `ref` a
    /// ref, an empty element's `children` a JSX child, anything else a native
    /// attribute. Lowering knows what the value actually became, and when it
    /// resolves the value as data — folded into the template, dropped, or
    /// written once — the truthful record depends on which kind the census
    /// chose:
    ///
    /// - a censused *value* site (native attribute, JSX child) is decided
    ///   with `decision`;
    /// - a censused *callback* site (event handler, ref) is withdrawn: the
    ///   value became template text, so no callback exists at runtime to
    ///   decide about, and a callback site cannot carry a value decision.
    ///
    /// A span the census never recorded is a no-op. Recording a hardcoded
    /// [`ExecutionSiteKind::NativeAttribute`] here instead — as every caller
    /// once did — failed the whole file for `on*`/`ref`/`children` spellings,
    /// either as an unresolved site or as a category mismatch.
    pub(crate) fn resolve_lowered_attribute(&mut self, span: Span, decision: ValueDecision) {
        for kind in [
            ExecutionSiteKind::NativeAttribute,
            ExecutionSiteKind::JsxChild,
        ] {
            if self.has_site(span, kind) {
                self.value(span, kind, decision);
                return;
            }
        }
        for kind in [ExecutionSiteKind::EventHandler, ExecutionSiteKind::Ref] {
            self.retract(span, kind);
        }
    }

    /// Withdraw a censused site that lowering proved does not exist.
    ///
    /// Reached through [`Self::resolve_lowered_attribute`] when the census
    /// named a callback site (an `on*` spelling, a `ref`) whose value
    /// lowering then resolved as plain data. Retracting is the truthful
    /// outcome — the site is not reported, rather than reported with an
    /// invented decision.
    ///
    /// Retracting a site that was never censused, or one already decided, is a
    /// no-op; this only ever removes a site nothing has spoken for.
    pub(crate) fn retract(&mut self, span: Span, kind: ExecutionSiteKind) {
        if !self.is_recording() {
            return;
        }
        let key = SiteKey {
            span: span.into(),
            kind,
        };
        if self.decisions.contains_key(&key) {
            return;
        }
        if let Some(census) = self.census.as_mut() {
            census.sites.remove(&key);
        }
    }

    pub(crate) fn value(&mut self, span: Span, kind: ExecutionSiteKind, decision: ValueDecision) {
        self.resolve(span, kind, TerminalDecision::Value(decision));
    }

    pub(crate) fn callback(
        &mut self,
        span: Span,
        kind: ExecutionSiteKind,
        decision: CallbackDecision,
    ) {
        self.resolve(span, kind, TerminalDecision::Callback(decision));
    }

    fn resolve(&mut self, span: Span, kind: ExecutionSiteKind, decision: TerminalDecision) {
        if !self.is_recording() {
            return;
        }
        let Some(census) = &self.census else {
            return;
        };
        let key = SiteKey {
            span: span.into(),
            kind,
        };
        if !census.sites.contains(&key) {
            if census
                .ignored_literal_spans
                .contains(&SourceSpan::from(span))
            {
                return;
            }
            self.fail(format!(
                "semantic decision targets an uncensused {kind:?} site at {}..{}",
                span.start, span.end
            ));
            return;
        }
        if kind.is_value() != matches!(decision, TerminalDecision::Value(_)) {
            self.fail(format!(
                "semantic decision has the wrong category for {kind:?} at {}..{}",
                span.start, span.end
            ));
            return;
        }
        if let Some(previous) = self.decisions.insert(key, decision) {
            if previous != decision {
                self.fail(format!(
                    "semantic site {kind:?} at {}..{} received conflicting terminal decisions",
                    span.start, span.end
                ));
            }
        }
    }

    fn fail(&mut self, message: String) {
        if self.error.is_none() {
            self.error = Some(message);
        }
    }

    pub(crate) fn finish(self) -> Result<Option<SemanticTrace>, String> {
        let Some(census) = self.census else {
            return Ok(None);
        };
        if let Some(error) = self.error {
            return Err(error);
        }
        let unresolved = census
            .sites
            .difference(&self.decisions.keys().copied().collect())
            .map(|site| format!("{:?}@{}..{}", site.kind, site.span.start, site.span.end))
            .collect::<Vec<_>>();
        if !unresolved.is_empty() {
            return Err(format!(
                "semantic trace has unresolved execution sites: {}",
                unresolved.join(", ")
            ));
        }
        let sites = census
            .sites
            .into_iter()
            .map(|site| ExecutionSite {
                span: site.span,
                kind: site.kind,
                decision: self.decisions[&site],
            })
            .collect::<Vec<_>>();
        let mut owner_establishments = self.owner_establishments;
        owner_establishments.sort_unstable();
        owner_establishments.dedup();
        Ok(Some(SemanticTrace {
            version: SEMANTIC_TRACE_VERSION,
            sites,
            owner_establishments,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn census(kind: ExecutionSiteKind) -> ExecutionCensus {
        ExecutionCensus {
            sites: [SiteKey {
                span: SourceSpan { start: 1, end: 2 },
                kind,
            }]
            .into_iter()
            .collect(),
            ignored_literal_spans: BTreeSet::new(),
        }
    }

    #[test]
    fn finish_rejects_an_unresolved_site() {
        let recorder = TraceRecorder::new(census(ExecutionSiteKind::JsxChild));
        assert!(recorder.finish().unwrap_err().contains("unresolved"));
    }

    #[test]
    fn finish_rejects_conflicting_decisions() {
        let mut recorder = TraceRecorder::new(census(ExecutionSiteKind::JsxChild));
        let span = Span::new(1, 2);
        recorder.value(
            span,
            ExecutionSiteKind::JsxChild,
            ValueDecision::ReactiveRerun,
        );
        recorder.value(span, ExecutionSiteKind::JsxChild, ValueDecision::EagerOnce);
        assert!(recorder.finish().unwrap_err().contains("conflicting"));
    }

    #[test]
    fn finish_rejects_uncensused_decisions() {
        let mut recorder = TraceRecorder::new(census(ExecutionSiteKind::JsxChild));
        recorder.value(
            Span::new(7, 9),
            ExecutionSiteKind::JsxChild,
            ValueDecision::EagerOnce,
        );
        assert!(recorder.finish().unwrap_err().contains("uncensused"));
    }

    #[test]
    fn finish_rejects_a_miscategorized_decision() {
        let mut recorder = TraceRecorder::new(census(ExecutionSiteKind::JsxChild));
        recorder.callback(
            Span::new(1, 2),
            ExecutionSiteKind::JsxChild,
            CallbackDecision::LaterEvent,
        );
        assert!(recorder.finish().unwrap_err().contains("wrong category"));
    }

    #[test]
    fn a_disabled_recorder_produces_no_trace() {
        let mut recorder = TraceRecorder::disabled();
        recorder.value(
            Span::new(1, 2),
            ExecutionSiteKind::JsxChild,
            ValueDecision::EagerOnce,
        );
        assert_eq!(recorder.finish().unwrap(), None);
    }

    #[test]
    fn repeated_identical_decisions_are_idempotent() {
        let mut recorder = TraceRecorder::new(census(ExecutionSiteKind::JsxChild));
        let span = Span::new(1, 2);
        recorder.value(span, ExecutionSiteKind::JsxChild, ValueDecision::EagerOnce);
        recorder.value(span, ExecutionSiteKind::JsxChild, ValueDecision::EagerOnce);
        let trace = recorder.finish().unwrap().unwrap();
        assert_eq!(trace.sites.len(), 1);
    }

    #[test]
    fn owner_establishments_are_recorded_at_emission_sites() {
        let mut recorder = TraceRecorder::new(census(ExecutionSiteKind::JsxChild));
        recorder.value(
            Span::new(1, 2),
            ExecutionSiteKind::JsxChild,
            ValueDecision::ReactiveRerun,
        );
        let group_id = recorder.next_group_id();
        recorder.owner_establishment(Span::new(1, 2), "effect", Some(group_id));
        recorder.owner_establishment(Span::new(1, 2), "effect", Some(group_id));
        recorder.owner_establishment(Span::new(3, 4), "effect", Some(group_id));
        let trace = recorder.finish().unwrap().unwrap();
        assert_eq!(
            trace.owner_establishments,
            vec![
                OwnerEstablishment {
                    span: SourceSpan { start: 1, end: 2 },
                    wrapper: "effect".into(),
                    group_id: Some(0),
                },
                OwnerEstablishment {
                    span: SourceSpan { start: 3, end: 4 },
                    wrapper: "effect".into(),
                    group_id: Some(0),
                },
            ]
        );
    }

    #[test]
    fn disabled_recorders_do_not_allocate_owner_facts() {
        let mut recorder = TraceRecorder::disabled();
        recorder.owner_establishment(Span::new(1, 2), "customEffect", None);
        assert_eq!(recorder.finish().unwrap(), None);
    }
}
