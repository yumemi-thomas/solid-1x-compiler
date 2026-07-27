# solid-1x-compiler

A controlled Solid 1.x JSX compiler, implemented with [Oxc](https://oxc.rs) in
Rust, kept at differential parity with the Babel compiler Solid 1.x actually
ships. It compiles JSX, and it reports the **execution contract** — which
original-source expressions the compiler runs tracked, untracked, once, or as a
deferred callback — from the same transform decisions that produce the code.

Two consumers want different halves of that:

- **Bundlers and build tools** want `transform()`: JSX in, DOM Expressions
  output out.
- **Static analyzers** want `CompileOptions::semantic_trace`: the compiler's
  own answer to "how will this JSX site execute?", which no tool can derive
  from source text alone.

Both come out of one pass — literally the same `compile()` call. The trace is
recorded *by* the lowering as it runs, then reconciled against an independent
census of the source before it is returned, so the semantics an analyzer
reasons about are the semantics the code was compiled with, not a second
implementation's opinion about them.

## Packages

| package | what it is |
| --- | --- |
| [`packages/compiler`](packages/compiler) | `@dom-expressions/compiler` — the Oxc compiler, published as a napi native addon and consumable from Rust as the `dom-expressions-compiler` crate. The implementation under test. |
| [`packages/babel-plugin-jsx-dom-expressions`](packages/babel-plugin-jsx-dom-expressions) | The Solid 1.x Babel compiler, imported unmodified. The behavioral oracle — never shipped, only compared against. |
| [`packages/dom-expressions`](packages/dom-expressions) | The exact `src/constants.js` the oracle imports. |

Provenance and upstream revisions: [PROVENANCE.md](PROVENANCE.md). Port notes
and the parity corpus census: [SOLID_1X_PORT.md](SOLID_1X_PORT.md).

## Using it from JavaScript

This repository does not publish to npm — `@dom-expressions/compiler` on the
registry is upstream's package, not this fork. Build the addon locally
(`pnpm build`) and require the package directory:

```js
const { transform } = require("./packages/compiler");

const result = transform(`const view = <div>Hello</div>;`, {
  filename: "App.jsx",
  moduleName: "dom",
  generate: "dom"
});
```

See [`packages/compiler/README.md`](packages/compiler/README.md) for the full
option surface, the Solid preset, directives, refresh, and lazy transforms;
its installation and prebuilt-binary sections describe upstream's release, and
the release machinery under `packages/compiler/npm` is carried unused so the
diff against upstream stays readable.

## Using it from Rust

The same crate links in-process. With default features off it has no Node-API
dependency at all — the addon is one adapter over the core, not a layer the
core sits on:

```toml
[dependencies]
dom-expressions-compiler = { git = "https://github.com/yumemi-thomas/solid-1x-compiler", tag = "v0.1.0", default-features = false }
```

```rust
use dom_expressions_compiler::{compile, CompileOptions};

let output = compile(source, &CompileOptions {
    module_name: "solid-js/web".into(),
    semantic_trace: true,
    ..CompileOptions::default()
})?;

for site in output.semantic_trace.expect("tracing was requested").sites {
    println!("{:?} {:?} {:?}", site.span, site.kind, site.decision);
}
```

`CompileOptions` states the module name and output mode directly instead of as
nullable transport fields, and `compile` returns owned `String`s and a
`CompileError` carrying a `kind()` of `Parse`, `Configuration`, or `Transform`.
No Oxc allocator, AST node, or Node-API value crosses the interface.

`semantic_trace` is reporting-only: a test proves the emitted code is
byte-identical with tracing on and off. Compilation fails closed when the
recorded decisions and the independent source census disagree, rather than
returning a partial trace. The trace types derive `Serialize`/`Deserialize`, so
a sidecar can define whatever envelope its consumer expects — the compiler owns
no wire format of its own. See
[docs/execution-contract.md](docs/execution-contract.md) for what the contract
covers and what it refuses to answer.

The default `node` feature adds the napi dependency and the `#[napi]` exports
in `src/node_adapter.rs`; turning it off drops both. The old `native-facts`
feature remains as an inert alias so existing pins keep resolving — it selects
nothing now, because the core no longer links Node-API symbols to stub.
Consumers that want the addon should depend on the npm package, not the crate.

## Development

Requires Rust 1.97 (pinned in `rust-toolchain.toml`), Node 24, and pnpm 11.

```bash
pnpm install
```

| command | what it does |
| --- | --- |
| `make build` / `pnpm build` | Builds the Babel oracle and a debug native addon |
| `make test` | Rust unit tests, then the full Jest suite |
| `make parity` | The differential run against the Babel 1.x oracle |
| `make verify` | fmt, clippy, Rust tests, Jest, and parity — the CI gate |
| `pnpm parity:diff` | Raw and normalized output artifacts, Babel (`-`) vs Oxc (`+`) |
| `pnpm parity:baseline` | Refreshes the ratchet after an intentional corpus change |

Changes to JSX classification must keep the execution-contract census total and
pass `make parity`. A classification change that no fixture covers is a gap in
the corpus, not a free change. A change to what a lowering path emits must
change what it reports in the same edit — the reconciliation tests run every
fixture and every parity probe through the contract, so a path that stops
reporting fails the build rather than going stale.

## Versioning for consumers

Rust consumers pin a git tag. The tag is the contract: the crate's public API,
the trace's site kinds and decision vocabulary, and the Solid 1.x semantics
they describe only change on a new tag. A consumer that transports traces
should carry its own protocol version and source hash and reject a mismatch
loudly, rather than analyze stale semantics.
