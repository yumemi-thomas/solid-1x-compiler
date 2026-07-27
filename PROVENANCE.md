# Provenance

This repository is a selective, maintained import of
[DOM Expressions](https://github.com/ryansolid/dom-expressions) (MIT). It exists
so that one Solid 1.x JSX compiler can be developed, differentially tested, and
consumed as a library, independently of any one consumer.

## Pinned upstream inputs

- **Oxc compiler** — `packages/compiler`, imported from
  `ryansolid/dom-expressions@55df930e42bc396c50adda9720fcc4b0b8a587b0`
  (`packages/compiler`, from `next`).
- **Solid 1.x behavioral oracle** — `packages/babel-plugin-jsx-dom-expressions`,
  imported from
  `ryansolid/dom-expressions@062d23cc29731e8c2281ddfa36188d438a90e21f`
  (from `main`).
- **Constants** — `packages/dom-expressions/src/constants.js`, imported from the
  same revision as the Babel package.
- **Host-independent compiler core** — `src/compiler.rs`, `src/error.rs` and
  `src/node_adapter.rs` are ported from
  `ryansolid/dom-expressions@feat/host-independent-compiler-core`, adapted to
  Solid 1.x: `dev` stays inert, and the adapter keeps this fork's established
  parse/skip/module/generate validation ordering.
- **Parity/trace design** —
  `ryansolid/dom-expressions@44f8d2668fff93c9d5ed5fdbef1cdadc1817a5a1`
  (the former `eat/total-semantic-trace` work, merged into `next`).
  `packages/compiler/src/semantic_trace.rs` is a port of that module's census
  and recorder, adapted to Solid 1.x lowering: the census models 1.x's
  `classList` splitting, `class`/`className` combining, `use:`/`on:` namespaces
  and `children` promotion, none of which exist upstream.

The upstream MIT license is retained: `LICENSE` at the root carries the upstream
copyright, and `packages/babel-plugin-jsx-dom-expressions/LICENSE` is the
upstream file unmodified.

## Dependencies that are not forked

- **Oxc** — <https://github.com/oxc-project/oxc>, version `0.118`, resolved
  exactly by `packages/compiler/Cargo.lock`. MIT. Consumed as published crates.
- **Babel** — consumed as published npm packages by the oracle and the parity
  harness only. It is not part of the shipped compiler.

## Extraction history

These packages lived in
[`yumemi-thomas/solid-checker`](https://github.com/yumemi-thomas/solid-checker)
under `packages/` until 2026-07-27, alongside the checker that consumes them.
They were extracted here so the compiler's execution-contract producer has one
home with its own conformance gate, and so consumers depend on a pinned crate
rather than a sibling directory.
