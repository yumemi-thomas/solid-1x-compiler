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

Three paths in the DOM lowering discard a subtree, and each calls it:

- the `<noscript>` static-template fast path, which emits the tag and returns
  without visiting the children (`dom/static_template.rs`);
- a hydratable `<head>` that is the direct child of a native element, replaced
  by a bare `createComponent(NoHydration, {})` (`dom/children.rs`);
- a hydratable `<head>` reaching `lower_element` in any other position — a
  template root, a component child, a conditional branch — same replacement
  (`dom/element.rs`).

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

Retraction is a claim about a *path*, not about a tag name. Every `<noscript>`
position this compiler really does lower — a template root, and a nested
`<noscript>` whose attributes push it off the static fast path — keeps its
sites, even though Babel emits nothing for either (divergences 2 and 8 below).
Under-reporting to match Babel would make the trace lie about the code this
compiler actually generated.

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

## Where this compiler and Babel 1.x still differ

These are measured against the vendored `babel-plugin-jsx-dom-expressions`
oracle. Divergences 1–7 each have a probe under
`__tests__/parity/expected-probes/`, so each is ratcheted rather than merely
written down; 8 and 9 were measured against the same oracle but are not in the
probe corpus yet, and each says why. The trace reports what *this* compiler does
at every one of them, faithfully; a consumer reasoning about Babel-compiled
output should read them as the list of places where the two answers are not the
same.

1. **A template root's `children` attribute ignores a later dynamic
   `textContent`.** Both attributes write Babel's single `children` slot in
   source order, so `<div children={x()} textContent={t()}/>` keeps the
   synthesized space text node and emits no insert. The root lowering promotes
   the captured value before its attribute loop runs, so it has no position to
   compare against and inserts anyway. The nested lowering does compare, and
   agrees with Babel in both orders. Probe: `1x children attribute before
   dynamic textContent`.
2. **Void and `<noscript>` template roots still lower their source children.**
   Babel guards the whole recursion — `if (!voidTag) { … if (tagName !==
   "noscript") transformChildren(…) }` — so `<br>{c()}</br>`,
   `<noscript>{c()}</noscript>` and `<noscript><span>s</span></noscript>` emit
   nothing (and contribute no markup) there. This compiler inserts, and keeps
   static children in the template. Only the `children`-attribute *promotion*
   carries those two gates, in both positions; the general case is untouched.
   Probes: `1x void root children`, `1x noscript root children`.

   Nested positions agree with Babel only where the static-template fast path
   owns them — a nested `<noscript>` whose attributes are all inlinable really
   does discard its children (measured parity in all four dom modes for
   `<div><noscript>{c()}</noscript></div>`, `<div><noscript><span>s</span>
   </noscript></div>` and a `ref` nested in the discarded subtree). The two
   nested cases that do *not* reach it are divergences of their own; see
   divergence 8.
3. **A `children` value this compiler folds confidently is dropped where Babel
   inserts it.** Babel's capture takes any container value that is not a string
   or number after `evaluateAndInline`, so `children={null}`,
   `children={undefined}`, `children={true}` and `children={{ a: 1 }}` become
   `_$insert(el, null)` and friends. Here the capture is filtered by
   `evaluate_confident`, which is confident about all four, so nothing is
   emitted. Position-independent. The selection is still Babel's — the last
   attribute Babel's capture keeps — and a capture dropped this way does *not*
   fall back to an earlier `children` attribute, because Babel's own capture
   already overwrote it: falling back would insert a value Babel never inserts.
   Probes: `1x children attribute nested undefined`, `… nested boolean
   literal`, `… nested confident object`, `1x duplicate children attributes
   trailing null`, `… nested trailing boolean`.
4. **A constant-foldable non-text value reaches the runtime unfolded.**
   `children={1 === 1}` inserts `true` in Babel, which rewrote the node before
   lowering, and `1 === 1` here. Independent of position and of generate — the
   ssr and universal modes diverge on the same probe. Probe: `1x children
   attribute nested constant folded boolean`.
5. **A JSX-valued hole loses Babel's inner IIFE.** Babel emits
   `() => (() => {…})()` where this compiler emits `() => {…}`, for
   `children={<b>{x()}</b>}` as for any other JSX-valued hole, in every
   position and in the universal modes too. Probe: `1x children attribute
   nested jsx value`.
6. **A nested custom element emits no `_$owner` context assignment.**
   `should_capture_custom_element_context` runs only at the template root, so
   `<div><my-el children={x()}/></div>` gets the insert but not the owner
   write. Pre-existing; promoting the nested `children` value is what made it
   visible. Probe: `1x children attribute nested custom element`.
7. **A dynamic `textContent` placeholder is pushed without a void/`<noscript>`
   gate.** The placeholder push in `children.rs` and its companion call in
   `element.rs` carry no void-element or `<noscript>` check at all, unlike
   every gate above, so a dynamic `textContent` on one of those elements gets
   an extra placeholder space text node Babel never emits:
   `<div><br textContent={t()}/></div>` is Babel's `` `<div><br>` `` against
   this compiler's `` `<div><br> ` `` (trailing space), and the same one-space
   difference shows for a root `<br textContent={t()}/>`,
   `<div><noscript textContent={t()}/></div>`, `<div><input
   textContent={t()}/></div>`, and with a `children={x()}` sibling on the same
   element. Pre-existing and byte-identical on `main` before this branch — the
   `children`-attribute promotion never touches this path, so it is scoped out
   of this change rather than fixed here. Probes: `1x br textContent
   placeholder`, `1x nested br textContent placeholder`.
8. **A nested `<noscript>` pushed off the static fast path lowers its
   children.** Babel's `if (tagName !== "noscript") transformChildren(…)` gate
   is on `transformElement`, so it holds in *every* position and whatever the
   attributes are. Here the gate is a property of the static-template fast path
   (`static_template.rs`), which any attribute that cannot inline into the
   markup rejects — a `class={c()}`, a `style` object, a `ref`, an `on*`
   handler. The element then takes the dynamic child path, which lowers the
   children like any other element's, so `<div><noscript
   class={c()}>{d()}</noscript></div>` emits an `_$insert` Babel does not, in
   all four dom modes; measured the same for `ref`, `onClick` and a `style`
   object. A nested void element diverges the same way and always has, since no
   fast path claims it: `<div><br>{c()}</br></div>` inserts here and emits
   nothing in Babel. Pre-existing in both cases. The trace reports the insert
   rather than hiding it, so a consumer sees the code this compiler generated —
   see "Discarded subtrees". No probes yet: adding them means ratcheting four
   dom-mode diffs per shape, which is a separate change.
9. **A hydratable nested `<head>` is discarded where Babel keeps it.** Babel
   only returns early from `transformElement` for a `head` at `info.topLevel`;
   a nested one is transformed normally, and it is the *parent's*
   `transformChildren` that drops the child's expressions (`if (child.tagName
   === "head")`) — after `results.template += child.template` has already taken
   the markup. This compiler drops the element outright in both positions, so
   `<div><head>{b()}</head></div>` is Babel's `` `<div><head>` `` plus an
   insert against this compiler's `` `<div>` `` plus nothing, and
   `<div><head><title>t</title></head></div>` loses the `<title>t` markup too.
   Measured in `dom-hydratable` only; the other three dom modes are parity
   (without `hydratable` the element is an ordinary native root). A `<head>`
   directly under `<html>` is parity in every dom mode, and so is a `<head>`
   template root, where Babel's own early return matches this one exactly.
   Pre-existing. The trace retracts the discarded interior because *this*
   compiler emits nothing for it — see "Discarded subtrees". No probes yet, for
   the same reason as divergence 8; `<Comp><head>{b()}</head></Comp>` would
   additionally need the suite's `referenceRejected` set, since Babel 1.x
   throws on it (`Property body of ArrowFunctionExpression expected node to be
   of a type ["BlockStatement","Expression"] but instead got
   "ExpressionStatement"`).

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
