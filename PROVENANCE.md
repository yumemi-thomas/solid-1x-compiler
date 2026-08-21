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
  **`babel-plugin-jsx-dom-expressions@0.40.10`**. Differential parity is
  defined against that exact version: whatever it emits, `packages/compiler`
  must emit.

  Originally imported from
  `ryansolid/dom-expressions@062d23cc29731e8c2281ddfa36188d438a90e21f`
  (from `main`) at 0.40.7, then moved to 0.40.10 by applying upstream's four
  behavioral changes to the vendored `src/`
  (`ryansolid/dom-expressions@727298ff79a54ef5299d3649ab7f4105a326e9b3`, the
  `gitHead` npm records for 0.40.10; 0.40.8 and 0.40.9 were never published,
  so this is one hop):

  1. `$ServerOnly` template skipping is gated on the new
     `omitServerOnlyTemplates` config;
  2. that option is added with default `true`, so 1 + 2 are behavior-preserving
     under default config;
  3. **security** — an attribute-position template literal now HTML-escapes its
     static quasis, so `` style={`url("${x}")`} `` can no longer close the
     quoted attribute value (attribute injection);
  4. `wrapConditionals` no longer excludes the `ssr` generate in
     `shared/component.js`, so ssr component props collapse a conditional
     through `transformCondition`. `shared/transform.js` keeps its ssr
     exclusion, so child and fragment holes are unaffected.

  The vendored bundle's 0.40.7 → 0.40.10 delta was verified line-for-line
  against the published npm tarballs of both versions: 18 added and 3 removed
  lines, identical on both sides.
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

## Intentional divergences from upstream

- **Hydration-id codegen** — upstream's later hydration-id codegen change is
  **still absent** here, deliberately. The 0.40.10 bump did not pull it in:
  none of that version's four changes touches hydration-id allocation, and the
  fixture-corpus diff for the bump is confined to ssr component-prop
  conditionals. Consumers that exclude it (see `solid-checker`'s dependency
  pin comment) can keep doing so.
- **`dev`** — accepted as an inert option. Hydration-walk validation
  (`getFirstChild`/`getNextSibling`) postdates Solid 1.x and must not be
  enabled when matching the 1.x Babel compiler.

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
