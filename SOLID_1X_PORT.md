# Solid 1.x compiler port

`packages/compiler` is the working Oxc compiler for Solid 1.x. It starts from
DOM Expressions' Oxc compiler on `next` and treats the Babel compiler on `main`
as the behavioral reference.

**The parity target is `babel-plugin-jsx-dom-expressions@0.40.10`** — the
version vendored under `packages/babel-plugin-jsx-dom-expressions`. "1.x
parity" means matching that exact version's output, not merely the 1.x line.

Upstream revisions, the 0.40.7 → 0.40.10 move, and intentional divergences
(including the still-excluded hydration-id codegen change) are recorded in
[PROVENANCE.md](PROVENANCE.md).

## Workflow

Install the workspace once:

```sh
pnpm install
```

Refresh the parity artifacts after intentionally changing the corpus:

```sh
pnpm parity:baseline
```

The `expected/`, `expected-probes/`, and former `expected-cross/` ratchet
directories are empty: every output the Babel reference can compile and print
as valid JavaScript now matches. Reference failures are enumerated explicitly
in the tests, so a fixed or newly broken Babel case cannot silently change the
comparison set.

After changing the Rust compiler, run:

```sh
pnpm parity
```

For raw and normalized output artifacts:

```sh
pnpm parity:diff
```

The diff direction is Babel (`-`) versus Oxc (`+`). Fix the Rust compiler and
regenerate the baseline whenever a fixture reaches parity.

## Parity corpus

The current corpus contains:

- **73/73** whole-file fixture cases at exact normalized parity.
- **3,937/3,937** focused probe comparisons at parity across all nine output
  modes (444 probe cases, 3,996 entries). The remaining 59 entries make no
  comparison and are counted separately rather than as passes: 34 where both
  compilers reject the input (TypeScript syntax, tags outside the `dynamic`
  renderer's element list), 14 where both emit syntactically invalid
  JavaScript so there is nothing to compare — Babel 1.x hoists an `await` out
  of its async function, and the Oxc compiler reproduces that faithfully —
  and 11 explicit one-sided Babel 1.x compiler failures.

  `PARITY_REPORT=1 pnpm parity:1x:probes` prints this breakdown, overall and
  per mode.
- **332/332** valid cross-mode fixture-union comparisons at parity. Six inputs
  are rejected by both compilers, and eight universal-mode entries are
  explicit cases where Babel 1.x prints syntactically invalid JavaScript.
- **144/144** option-matrix tests passing, covering each mode with one option
  changed at a time (including `omitServerOnlyTemplates: false`). Sixteen
  fixture/option entries explicitly record Babel 1.x's `memoWrapper: false`
  assertion failure while Oxc remains usable; four of those arrived with
  0.40.10, which routes ssr component props through `transformCondition` and
  so makes the reference's assertion reachable in the ssr modes too.

The full compiler package suite also covers validation warnings, server
directives, refresh, lazy transforms, binding loading, and Rust classification
invariants.
