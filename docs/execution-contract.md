# The execution contract

Setting `CompileOptions::semantic_trace` makes `compile` report how the
compiler will execute the original-source JSX it was handed, alongside the code
it generated for it. The report is typed Rust data, not a wire format: a
consumer that needs to move it across a process boundary defines its own
envelope, while checking the producer's `SEMANTIC_TRACE_VERSION` before
accepting the strict schema.

## Totality, not sampling

Two independent producers have to agree before a contract exists.

Before lowering, `ExecutionCensus` walks the source and enumerates every
relevant original-source JSX site. That is the denominator: "this expression is
here, and the compiler owes an answer about it."

Then the real DOM lowering pass runs with a `TraceRecorder` attached, and calls
`trace_value` / `trace_callback` at the exact point it decides what to do with
each site. That is the numerator, and it is an *observation* — the decision is
recorded by the code that emitted (or discarded) the value, not re-derived
afterwards from the rule that produced it.

`TraceRecorder::finish` reconciles the two and fails closed on any of:

- an **unresolved** censused site — lowering never said what it did with it;
- **conflicting** decisions for one site — two paths disagree;
- a decision aimed at an **uncensused** site — lowering reported something the
  census does not recognize.

`compile` returns the reconciled decisions in `CompileOutput::semantic_trace`,
from the same pass that produced `CompileOutput::code`.

That completeness invariant is the whole point. A consumer that receives a
partial contract cannot tell "this expression runs untracked" from "the
compiler forgot to mention it", so a partial contract must be rejected rather
than read optimistically. Absence is never untracked rendering.

The reconciliation is also what keeps the two halves from drifting. A lowering
rule that changes without its reporting changing with it stops being a stale
contract nobody notices and becomes a build error: the corpus reconciliation
tests run every Babel fixture and every parity probe through `finish`, so a
transform path that forgets to report fails there.

Reporting is side-effect free: with `semantic_trace` off the recorder is
disabled and every call is a no-op, and a test asserts the emitted code is
byte-identical either way.

## Scope

Spans are byte offsets into the exact source that was compiled, valid only for
those bytes and the options they were compiled under. A consumer that caches or
transports a trace is responsible for carrying whatever it needs to re-establish
that — a source hash, an options hash, a protocol version — and for rejecting a
mismatch.

Only DOM generation produces a trace and these lowering facts; every other generate returns a
configuration error rather than a partial answer, as does a source skipped by
`requireImportSource` (there is no lowering to report on).

## What a site says

Each `ExecutionSite` carries a span, a closed `ExecutionSiteKind` naming the
JSX position, and exactly one terminal decision. Sites are ordered
deterministically by span. The kind is an observation of the lowering branch,
not a runtime claim; in particular, `control-flow-render` is emitted by the
lowering that creates the deferred callback. Consumers should use that fact
instead of recomputing the callback from their own JSX AST.

Value positions — `jsx-child`, `native-attribute`, `native-spread`,
`component-property`, `component-spread`, `component-child` — decide between:

| decision | meaning |
| --- | --- |
| `reactive-rerun` | read inside an effect; re-runs when its sources change |
| `eager-once` | read exactly once, at creation |
| `caller-context` | handed to a caller (a getter, a spread merge, a directive accessor) that decides when to read it |
| `elided` | never emitted — folded into the template string, or dropped |

Callback positions decide between `later-event` (`event-handler`),
`later-render` (`control-flow-render`, a built-in's function child), and
`ref-apply` (`ref`).

Expressions with no observable execution are not sites at all: literal-only
leaves have nothing to report, and neither does anything nested inside a value
the compiler discards wholesale.

## Discarded subtrees

Some lowering paths do not decide a child list value by value; they skip the
whole subtree without visiting it. Nothing there is emitted, so nothing there
executes, so nothing there is a site — but the census walks source and cannot
know that, and an unresolved site fails the *file*, not the shape. A source
`tsc` accepts would become unanalysable for a consumer.

`TraceRecorder::retract_within` is the answer: the discarding path withdraws
the censused sites inside the range it skipped, at the point it decides to skip
them. Like every other reporting call it is an observation by the code that
made the decision, not a rule the census re-derives — which matters here more
than elsewhere, because "is this subtree reachable" is exactly the kind of
question two independent derivations get differently. It is recorder-internal:
no serialized field carries it, and `SemanticTrace` looks the same as if the
census had never enumerated those sites.

The parity-clean discard paths are:

- every void-element and `<noscript>` child list, at a template root, in the
  static-template fast path, and in the dynamic nested path;
- a hydratable `<head>` template root (or another expression-position head)
  replaced by `createComponent(NoHydration, {})` before attribute or child
  lowering;
- the setup arrays of a dynamic hydratable `<head>` nested under a native
  element. That path deliberately lowers far enough to retain Babel's head
  markup, then discards the declarations, operations and dynamics and emits
  the same bare `NoHydration` call Babel does.

A static nested hydratable `<head>` is not a discard path: it stays in the
template and produces no `NoHydration` call, exactly like Babel. The focused
probes cover dynamic markup, nested static markup, and the direct-static
`child.id` gate.

The `<head>` paths retract over the whole *element*, not its child list: the
replacement runs before attribute lowering, so a `ref` or handler written on the
`<head>` itself never executes either.

Two boundaries keep retraction from becoming a way to lose information.
**A site something already decided is kept**, so a discarding path can only
remove sites nothing has spoken for. And **containment is strict** — the same
rule the census's own `dropped` predicate applies. A site spanned at the
discarded node *itself* belongs to the parent lowering that decided it: a
`<head>` handed to a component as a child really is a `component-child` whose
getter the caller holds, and a JSX-valued hole or a conditional branch around
one really is inserted. Only the interior stops existing. Strictness also makes
the call order-independent, where an inclusive range would quietly depend on
whether the parent recorded its decision before or after the discarding path
ran.

Retraction is a claim about a *path*, not merely a tag name. The compiler now
applies Babel's void/`<noscript>` recursion gates in every native position;
attributes on the element may still lower, but no source child site survives.

Non-hydratable nested `<head>` is the one discard-shaped case handled the other
way round, and it is genuinely a different shape: its static children *do* reach
the template, only the setup expressions and walk slot are dropped. The census
excludes that subtree up front (`dropped_ranges`) and lowering suppresses
reporting while it runs; see `semantic_trace.rs`.

## Lowering wrapper facts

`owner_establishments` records the wrapper identity and source span at the
lowering site that emitted it, one fact per wrapper call the lowering emits.
The span rule is uniform: it is the exact source span of the expression or JSX
node whose lowering is being wrapped — never the JSX expression container
including braces, the whole attribute, or the parent element. When the
construct has an `ExecutionSite`, the spans are equality-joinable; JSX-node
facts join to `ComponentRenderSite` or `DeferredCallbackSite` spans. It is
additive evidence about compiler output, not a runtime ownership, ancestry,
timing, or render-occurrence claim.

A conditional's memo is the one case where the wrapped expression is smaller
than the site: `{cond() ? left() : right()}` lowers to
`memo(() => !!cond())`, with the branches evaluated in the insert's or
getter's scope, so the memo fact is spanned at `cond()` — the test it actually
memoizes — and is *contained by* the enclosing site's span rather than equal
to it. Each memo the lowering emits gets its own fact, so a nested conditional
reports one fact per memoized test, and a fragment or component child whose
thunk is also memo-wrapped reports that memo separately at the child
expression's span.

Two consequences for a consumer building a map from these facts. **A span is
not a unique key**: one span can carry more than one wrapper identity — a
component child is both rendered and inserted at the same span
(`createComponent` + `insert`) — so key on `(span, identity)`, never on the
span alone. And **a fact need not join to any site**: a literal-only hole such
as `<div>{true}{undefined}{null}</div>` really does emit an `insert` per hole,
and those inserts are reported, but literal-only leaves are deliberately not
`ExecutionSite`s, so those facts join to nothing.

Facts are additive within schema version 2, but the schema is intentionally
strict: `SemanticTrace` rejects unknown fields and consumers must reject an
unsupported `version`. The identity is deliberately preserved as a string so
a custom or unaudited wrapper remains representable; the consumer maps it
through its audited dialect and treats an unknown identity as unknown. An
optional `group_id` links the bindings emitted into one keyed effect wrapper
invocation.

The current DOM lowering records these facts for effect, memo,
`createComponent`, insert, delegated, direct, capture, and ref-apply wrappers.
One carve-out: the synthesized `createComponent(NoHydration, {})` call the two
hydratable `<head>` discard paths emit (see "Discarded subtrees") records no
`owner_establishments` fact at all — neither call site calls
`owner_establishment`. That is a facts-completeness gap in what this producer
reports about its own output, not a trace-contract violation; a consumer that
builds an exhaustive map of `createComponent` calls from this fact alone will
miss these two.
Event identities describe auditable semantics: `delegated`, `direct`, and
`capture`; they do not expose which helper or builder happened to emit the
listener. It also records `component_render_sites`, which are spans where JSX
component renders are emitted. This fact is deliberately kept as an explicit
consumer seam even though it is currently one-to-one and span-identical with
`createComponent` establishments. Neither fact replaces the consumer's
dialect or runtime model; the consumer should use the spans and identities
directly rather than reconstructing them from JSX AST or inferred ownership
rules.

`deferred_callback_sites` pairs a deferred component prop, ref, spread, or
control-flow child callback span with the enclosing JSX component span that
receives it. It is a source relationship only; the consumer should use it to
attach the callback to the receiver span, without inferring callback timing or
runtime receiver semantics.

This is an intentional trace-contract replacement for the former
`ownership_sites` / `OwnershipDecision` vocabulary. A consumer upgrading to
this producer must migrate its mapping and version check before moving the
compiler pin; old serialized traces are rejected rather than silently treated
as having no wrapper facts.

`packages/compiler/src/shared/classify.rs` is the single classification
authority. Only lowering consults it for decisions; the census uses it solely
to enumerate sites, so the two cannot quietly re-derive the same rule
differently.

## Changing a classification

A classification change is a contract change. It must keep the census total,
update the affected fixtures, and pass `make parity` against the Babel 1.x
oracle. If no fixture covers the changed behavior, the corpus has a gap — add
the fixture rather than landing the change uncovered.

If a change makes lowering drop a value, say so where it is dropped
(`ValueDecision::Elided`); if it makes lowering emit a value the census does
not enumerate, teach the census about it. Reconciliation will not let a change
land with only one of the two updated.

## Changing what a lowering emits

A change to generated code is a contract change twice over: the trace has to
follow it, and the Babel 1.x oracle has to agree with it. Two artifacts make
that reviewable rather than discovered later.

`tests/transform-output-baseline.txt` holds every corpus source's emitted bytes
— each Babel fixture and each parity probe, hex-encoded, or `reject` where the
compiler refuses the input — generated from the branch point.
`transform_output_matches_checked_in_baseline` compares the current build
against it and names every entry that moved.
`tracing_does_not_change_generated_output` is *not* that invariant: both halves
of one build can share the same codegen regression.

The sequence for a deliberate transform change is: run the comparison first and
account for every mover, then regenerate with

```sh
UPDATE_TRANSFORM_BASELINE=1 cargo test --no-default-features \
  --test execution_contract_census regenerate_transform_output_baseline \
  -- --ignored --nocapture
```

which is `#[ignore]`d and environment-gated precisely so no ordinary run — not
even `--include-ignored` — can rewrite the witness as a side effect. A change
that no fixture and no probe covers is a corpus gap: add the probes, and the
regenerated baseline then grows by exactly those entries and nothing else.

## Resolved compiler differences from Babel 1.x

These are measured against the vendored `babel-plugin-jsx-dom-expressions`
oracle. The divergences below are resolved and their differential artifacts
were deleted only after the affected modes reached byte parity.

1. **Resolved — template-root `children`/`textContent` slot order.** Both
   attributes write Babel's single `children` slot in source order, so
   `<div children={x()} textContent={t()}/>` keeps the synthesized space text
   node and emits no insert. Template-root lowering now applies the same
   dynamic-last-writer check as the nested path. The focused trace regression
   pins both positions, and the probe `1x children attribute before dynamic
   textContent` is byte-identical to Babel in DOM, hydratable DOM,
   no-inline-styles DOM, and dynamic DOM output.
2. **Resolved — void and `<noscript>` child recursion.** Root and nested
   lowering now apply Babel's gates before visiting source children, including
   attribute-driven nested paths. The exact Babel 0.40.10 void vocabulary is
   used, including its legacy `<keygen>` and `<menuitem>` entries rather than
   the modern runtime's shorter list. The trace retracts the discarded sites.
   Probes: `1x void root children`, `1x noscript root children`, `1x nested
   void children with dynamic attribute`, `1x nested noscript children with
   dynamic attribute`, and the four `1x legacy void …` position/value forms.
3. **Resolved — confidently folded non-text `children`.** `null`, `undefined`,
   booleans and confident object values now write Babel's child slot and are
   inserted after folding; string and numeric folds remain literal attributes.
   Duplicate selection still follows Babel's source-order slot semantics.
4. **Resolved — folded child value emission.** A value such as
   `children={1 === 1}` now reaches insertion as `true`, matching Babel's
   `evaluateAndInline` preprocessing.
5. **Resolved — JSX-valued hole IIFE.** Reactive child thunks retain Babel's
   expression-position setup boundary, emitting `() => (() => {…})()` for
   `children={<b>{x()}</b>}` and other JSX-valued holes. Ordinary
   statement-position `return <JSX>` continues to lift setup. Probe: `1x
   children attribute nested jsx value`.
6. **Resolved — nested custom-element owner context.** Native custom elements,
   customized built-ins, and slots now receive the same owner assignment in
   nested dynamic lowering as at the template root.
7. **Resolved — void/`<noscript>` text placeholders.** Dynamic `textContent`
   no longer synthesizes a space child where Babel skips child recursion.
8. **Resolved with item 2 — nested void and `<noscript>` paths.** The gate is
   now independent of whether the static-template fast path accepted the
   element.
9. **Resolved — hydratable nested `<head>`.** Static head markup is retained
   without a `NoHydration` call. Dynamic head markup is retained while its
   setup arrays are discarded and replaced by the bare call Babel emits.
   Probes cover dynamic markup, nested static markup, and the direct-static
   `child.id` gate; dynamic-mode reference rejections are recorded explicitly.

Shadowed JSX-valued native `children` attributes are reconciled under the same
discarded-subtree contract: lowering keeps the outer attribute-value site as
`elided` and withdraws nested sites it never visits. Transform output is
unchanged and remains byte-identical to Babel.

Two more shapes were measured while confirming the list and are recorded
without probes of their own, so they are not mistaken for parity: nested is
where the 1.x plugin *throws* on a literal `children` property write — a
one-sided reference failure, not a divergence — and the throw is not specific
to strings: `<div><span children="s"/></div>` and `<div><span
children={5}/></div>` both throw the same `"Property object of
MemberExpression expected node to be of a type [\"Expression\",\"Super\"] but
instead got undefined"`, because the failure is in Babel's nested attribute
lowering itself, before it looks at the value. The `.children = "5"` versus
`.children = 5` value difference is a *different*, non-throwing shape: a
**root**-position `<span children={5}/>`, where Babel's literal-attribute path
folds the number to a string (`children` is not in `Properties`) while this
compiler keeps it numeric. Neither is in the probe corpus: pinning a reference
failure also means enumerating it in the suite's `referenceRejected` set,
which is a separate change from this one.
