"use strict";
const { duallity } = require("@vinary-tree/javascript-runtime");
const { assertDictionaryResource, assertSameRuntime } = require("@vinary-tree/vinary-tree-interop");
const { assertLegacyWfstArguments } = require("./legacy-wfst-options.cjs");
const runtimeIdentity = duallity.runtimeIdentity;
function wfst(dictionary, query, maximumDistance, algorithm, kind) {
  assertLegacyWfstArguments(arguments);
  assertDictionaryResource(dictionary);
  assertSameRuntime(dictionary, runtimeIdentity);
  return duallity.wfst(dictionary, query, maximumDistance, algorithm, kind);
}
const facade = { ...duallity, runtimeIdentity, wfst };
module.exports = { ...facade, default: facade };
