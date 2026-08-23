# The execution contract

Setting `CompileOptions::semantic_trace` makes `compile` report how the
compiler will execute the original-source JSX it was handed, alongside the code
it generated for it. The report is typed Rust data, not a wire format: a
consumer that needs to move it across a process boundary defines its own
envelope and version, because that is a transport concern rather than a
compiler one.

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

Only DOM generation produces a trace; every other generate returns a
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
