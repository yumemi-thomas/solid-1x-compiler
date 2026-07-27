// Cross-mode fixture-union parity.
//
// parity.test.js checks each mode against its OWN fixture directory. This
// suite compiles the UNION of all Babel fixture sources through EVERY mode
// with both compilers, so a construct exercised only by (say) the dom
// fixtures is also locked in for ssr and universal. This is the guardrail
// the traversal/classification unification is meant to satisfy: one shared
// classification layer means a source that matches Babel in one mode cannot
// silently diverge in another.
//
// Some Babel outputs are not even parseable JS (babel-plugin-jsx prints raw
// newlines into universal-mode string props). Those reference failures are
// enumerated exactly below; every output that can be compared must match.

const {
  modes,
  fixtureNames,
  readFixtureSource,
  compileBabel,
  compileOxc,
  normalize,
  unifiedDiff
} = require("./parity/harness");

const referenceRejected = new Set([
  "universal/dom_fixtures--attributeExpressions",
  "universal/dom_hydratable_fixtures--attributeExpressions",
  "universal/ssr_fixtures--attributeExpressions",
  "universal/ssr_hydratable_fixtures--attributeExpressions",
  "dynamic-universal/dom_fixtures--attributeExpressions",
  "dynamic-universal/dom_hydratable_fixtures--attributeExpressions",
  "dynamic-universal/ssr_fixtures--attributeExpressions",
  "dynamic-universal/ssr_hydratable_fixtures--attributeExpressions"
]);
const seenReferenceRejected = new Set();

// The union of fixture sources, deduplicated by content (several mode
// directories carry identical fixtures). Ids stay stable as fixtures evolve:
// <fixtureDir>--<fixture>, keyed to the first directory that defines the
// content.
function fixtureUnion() {
  const byContent = new Map();
  const union = [];
  for (const mode of Object.keys(modes)) {
    const { fixtureDir } = modes[mode];
    for (const fixture of fixtureNames(mode)) {
      const source = readFixtureSource(mode, fixture);
      if (byContent.has(source)) continue;
      byContent.set(source, true);
      union.push({
        id: `${fixtureDir.replace(/__/g, "")}--${fixture}`,
        fixtureDir,
        source
      });
    }
  }
  return union.sort((a, b) => (a.id < b.id ? -1 : 1));
}

const union = fixtureUnion();

// Compares one union source under one mode's options. Returns "" at parity,
// otherwise a stable divergence record.
function crossDiff(mode, entry) {
  const caseKey = `${mode}/${entry.id}`;
  const { options } = modes[mode];
  let babelRaw, babelError;
  try {
    babelRaw = compileBabel(entry.source, options);
  } catch (err) {
    babelError = err;
  }
  let oxcRaw, oxcError;
  try {
    oxcRaw = compileOxc(entry.source, entry.id, options);
  } catch (err) {
    oxcError = err;
  }
  // Both compilers rejecting the input is parity (e.g. cross-renderer
  // nesting in dynamic mode).
  if (babelError && oxcError) return "";
  if (babelError) return `!! babel error: ${babelError.message.split("\n")[0]}\n`;
  if (oxcError) return `!! oxc error: ${oxcError.message.split("\n")[0]}\n`;
  let babelOut, oxcOut;
  try {
    babelOut = normalize(babelRaw);
  } catch (err) {
    if (referenceRejected.has(caseKey)) {
      try {
        normalize(oxcRaw);
      } catch (oxcNormalizeError) {
        return `!! Oxc output does not normalize: ${oxcNormalizeError.message.split("\n")[0]}\n`;
      }
      seenReferenceRejected.add(caseKey);
      return "";
    }
    return `!! babel output does not normalize: ${err.message.split("\n")[0]}\n`;
  }
  if (referenceRejected.has(caseKey)) {
    return "!! expected the Babel 1.x reference output not to normalize, but it did\n";
  }
  try {
    oxcOut = normalize(oxcRaw);
  } catch (err) {
    return `!! oxc output does not normalize: ${err.message.split("\n")[0]}\n`;
  }
  return unifiedDiff(babelOut, oxcOut);
}

describe("cross-mode fixture-union parity", () => {
  for (const mode of Object.keys(modes)) {
    // A mode's own fixture directory is already ratcheted by parity.test.js.
    const foreign = union.filter(entry => entry.fixtureDir !== modes[mode].fixtureDir);
    describe(mode, () => {
      it.each(foreign.map(entry => [entry.id, entry]))("%s", (id, entry) => {
        const diff = crossDiff(mode, entry);
        if (diff !== "") {
          throw new Error(`${mode}/${id} diverges from the Babel 1.x reference:\n\n${diff}`);
        }
      });
    });
  }

  it("exercises exactly the known invalid Babel outputs", () => {
    expect([...seenReferenceRejected].sort()).toEqual([...referenceRejected].sort());
  });
});
