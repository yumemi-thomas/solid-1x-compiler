# Solid 1.x compiler port

`packages/compiler` is the working Oxc compiler for Solid 1.x. It starts from
DOM Expressions' Oxc compiler on `next` and treats the Babel compiler on `main`
as the behavioral reference.

Upstream revisions and license provenance are recorded in
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
- **3,967/3,967** valid focused probe comparisons at parity across all nine
  output modes (442 probe cases). Eleven additional entries are explicit
  Babel 1.x compiler failures.
- **332/332** valid cross-mode fixture-union comparisons at parity. Six inputs
  are rejected by both compilers, and eight universal-mode entries are
  explicit cases where Babel 1.x prints syntactically invalid JavaScript.
- **135/135** option-matrix tests passing, covering each mode with one option
  changed at a time. Twelve fixture/option entries explicitly record Babel
  1.x's `memoWrapper: false` assertion failure while Oxc remains usable.

The full compiler package suite also covers validation warnings, server
directives, refresh, lazy transforms, binding loading, and Rust classification
invariants.
