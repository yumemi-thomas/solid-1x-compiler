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
oracle, one probe per shape under `__tests__/parity/expected-probes/`, so each
is a recorded divergence rather than an unnoticed one. The trace reports what
*this* compiler does at each of them, faithfully; a consumer reasoning about
Babel-compiled output should read them as the list of places where the two
answers are not the same.

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
   Nested positions already agree with Babel.
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

Two more shapes were measured while confirming the list and are recorded
without probes of their own, so they are not mistaken for parity: a nested
element whose only runtime work is a literal `children` property write
(`<div><span children="s"/></div>`) makes the 1.x plugin *throw* — a one-sided
reference failure, not a divergence — while this compiler emits the write; and
a numeric `children={5}` becomes `.children = "5"` in Babel (`children` is not
in `Properties`, so the fold stringifies it) against `.children = 5` here.
Neither is in the probe corpus: pinning a reference failure also means
enumerating it in the suite's `referenceRejected` set, which is a separate
change from this one.
