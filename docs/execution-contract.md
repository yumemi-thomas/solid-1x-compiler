# The execution contract

`analyze_execution_contract` reports how the compiler will execute the
original-source JSX it was handed. It is the producer side of the contract; a
consumer validates it before trusting it, and the two halves are deliberately
independent.

## Totality, not sampling

Before lowering, the compiler independently enumerates every relevant
original-source JSX site — the census. Lowering then assigns exactly one
terminal value or callback decision to each censused site, and finish-time
validation rejects a contract with a missing or conflicting decision.

That completeness invariant is the whole point. A consumer that receives a
partial contract cannot tell "this expression runs untracked" from "the
compiler forgot to mention it", so a partial contract must be rejected rather
than read optimistically. Absence is never untracked rendering.

Reporting is side-effect free: enabling it does not change generated
JavaScript, and tests assert the emitted code is byte-identical either way.

## Scope

The contract carries the exact source, the normalized options, the output mode,
and the producer identity it was computed under, plus `solidSemantics: "1"`.
A consumer is expected to check all of them against the bytes it analyzed.

Only DOM generation claims total facets. Other renderer modes, malformed
options, unknown fact kinds, invalid UTF-8 boundaries, and stale source hashes
fail closed rather than degrade.

## What the DOM contract decides

- Dynamic native JSX children are tracked `jsx-child` regions.
- Dynamic native JSX attributes are tracked `jsx-attribute` regions.
- Expressions the compiler renders exactly once are explicit untracked regions:
  template-inlined and unwrapped-insert children (including `staticMarker`
  holes) as `jsx-child`, one-shot `setAttr` attribute values as
  `jsx-attribute`, and by-value component properties and children as
  `component-getter`.
- `on*` JSX values are deferred `event-handler` callbacks, not tracked reads at
  element creation.
- Component invocations and dynamic component properties are identified;
  property getters are deferred callbacks.
- Function children of configured control-flow built-ins are render callbacks.
- `hydratable`, `dev`, `effectWrapper`, `wrapConditionals`, `staticMarker`, and
  sorted, unique `builtIns` are forwarded exactly to the compiler.
- Fact arrays are sorted deterministically by original UTF-8 byte spans.

`packages/compiler/src/shared/classify.rs` is the single classification
authority; lowering and the census both go through it, which is what keeps the
two from disagreeing.

## Changing a classification

A classification change is a contract change. It must keep the census total,
update the affected fixtures, and pass `make parity` against the Babel 1.x
oracle. If no fixture covers the changed behavior, the corpus has a gap — add
the fixture rather than landing the change uncovered.
