"use strict";
const { duallity } = require("@vinary-tree/javascript-runtime");
const { assertDictionaryResource, assertSameRuntime } = require("@vinary-tree/vinary-tree-interop");
const runtimeIdentity = duallity.runtimeIdentity;
function wfst(dictionary, query, maximumDistance, algorithm, kind) {
  assertDictionaryResource(dictionary);
  assertSameRuntime(dictionary, runtimeIdentity);
  return duallity.wfst(dictionary, query, maximumDistance, algorithm, kind);
}
module.exports = { ...duallity, runtimeIdentity, wfst, default: duallity };
