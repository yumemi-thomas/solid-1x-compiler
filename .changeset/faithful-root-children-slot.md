---
"@dom-expressions/compiler": patch
---

Match the Solid 1.x Babel compiler across native child lowering: preserve the
source-order `children`/`textContent` slot, insert confidently folded non-text
`children` values, discard void and `<noscript>` child lists and placeholders,
capture nested custom-element owner context, and retain Babel's hydratable
nested `<head>` markup and `NoHydration` gate.
