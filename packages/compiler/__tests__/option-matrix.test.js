// Option-matrix sweep: recompiles the whole Babel fixture corpus under
// non-default option combinations (one flag flipped at a time per mode) and
// requires normalized output parity, including error parity — a flag that
// only one compiler honors shows up here as a diff.

const {
  modes,
  fixtureNames,
  readFixtureSource,
  compileBabel,
  compileOxc,
  normalize,
  unifiedDiff
} = require("./parity/harness");

// Applied one at a time on top of each mode's base options so a failure
// points at a single flag. Flags without meaning for a generate target are
// still passed to both compilers — parity includes ignoring them identically.
const variants = {
  "omitQuotes:false": { omitQuotes: false },
  "omitAttributeSpacing:false": { omitAttributeSpacing: false },
  "delegateEvents:false": { delegateEvents: false },
  "omitNestedClosingTags:true": { omitNestedClosingTags: true },
  "omitLastClosingTag:false": { omitLastClosingTag: false },
  "wrapConditionals:false": { wrapConditionals: false },
  "effectWrapper:false": { effectWrapper: false },
  "memoWrapper:false": { memoWrapper: false },
  customWrappers: { effectWrapper: "createRenderEffect", memoWrapper: "createMemo" },
  "staticMarker:@once": { staticMarker: "@once" },
  "delegatedEvents:custom": { delegatedEvents: ["custom", "keyup"] },
  "contextToCustomElements:flip": mode => ({
    contextToCustomElements: !mode.options.contextToCustomElements
  }),
  "inlineStyles:false": { inlineStyles: false },
  "dev:true": { dev: true },
  "validate:false": { validate: false }
};

// These are bugs in the Solid 1.x Babel reference: disabling memo wrapping
// trips an internal assertion for these fixtures. Oxc intentionally remains
// usable, so parity is defined only for the cases the reference can compile.
// Keep the list exact so a newly fixed or newly broken reference case cannot
// silently disappear from the matrix.
const referenceRejected = new Set([
  "dom/memoWrapper:false/attributeExpressions",
  "dom/memoWrapper:false/components",
  "dom/memoWrapper:false/conditionalExpressions",
  "dom-hydratable/memoWrapper:false/attributeExpressions",
  "dom-hydratable/memoWrapper:false/components",
  "dom-hydratable/memoWrapper:false/conditionalExpressions",
  "universal/memoWrapper:false/components",
  "universal/memoWrapper:false/conditionalExpressions",
  "dynamic-universal/memoWrapper:false/components",
  "dynamic-universal/memoWrapper:false/conditionalExpressions",
  "dynamic/memoWrapper:false/conditionalExpressions",
  "dynamic/memoWrapper:false/hybrid"
]);

describe("option-matrix parity", () => {
  for (const [modeName, mode] of Object.entries(modes)) {
    describe(modeName, () => {
      for (const [variantName, patch] of Object.entries(variants)) {
        const extra = typeof patch === "function" ? patch(mode) : patch;
        const options = { ...mode.options, ...extra };

        test(variantName, () => {
          const failures = [];
          const expectedReferenceRejections = new Set(
            [...referenceRejected].filter(key => key.startsWith(`${modeName}/${variantName}/`))
          );
          for (const fixture of fixtureNames(modeName)) {
            const caseKey = `${modeName}/${variantName}/${fixture}`;
            const source = readFixtureSource(modeName, fixture);
            let babelRaw, oxcRaw, babelErr, oxcErr;
            try {
              babelRaw = compileBabel(source, options);
            } catch (err) {
              babelErr = err.message.split("\n")[0];
            }
            try {
              oxcRaw = compileOxc(source, fixture, options);
            } catch (err) {
              oxcErr = err.message.split("\n")[0];
            }
            if (babelErr || oxcErr) {
              if (babelErr && !oxcErr && referenceRejected.has(caseKey)) {
                expectedReferenceRejections.delete(caseKey);
                continue;
              }
              if (babelErr && oxcErr) continue; // error parity
              failures.push(
                `${fixture}: babel error: ${babelErr ?? "-"} | oxc error: ${oxcErr ?? "-"}`
              );
              continue;
            }
            if (referenceRejected.has(caseKey)) {
              failures.push(`${fixture}: expected the Babel 1.x reference to reject, but it compiled`);
              expectedReferenceRejections.delete(caseKey);
            }
            const babelNorm = normalize(babelRaw);
            const oxcNorm = normalize(oxcRaw);
            if (babelNorm !== oxcNorm) {
              failures.push(`${fixture}:\n${unifiedDiff(babelNorm, oxcNorm)}`);
            }
          }
          for (const missing of expectedReferenceRejections) {
            failures.push(`${missing}: expected reference-rejection case was not exercised`);
          }
          if (failures.length) {
            throw new Error(
              `${failures.length} fixture(s) diverged under ${modeName} + ${variantName}:\n\n` +
                failures.join("\n")
            );
          }
        });
      }
    });
  }
});
