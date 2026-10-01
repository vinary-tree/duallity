"use strict";
const { duallity } = require("@vinary-tree/javascript-runtime");
const { assertDictionaryResource, assertSameRuntime } = require("@vinary-tree/vinary-tree-interop");
const { assertLegacyWfstArguments, assertConfiguredWfstArguments } = require("./wfst-arguments.cjs");
const runtimeIdentity = duallity.runtimeIdentity;
function wfst(dictionary, query, maximumDistance, algorithm, kind) {
  assertLegacyWfstArguments(arguments);
  assertDictionaryResource(dictionary);
  assertSameRuntime(dictionary, runtimeIdentity);
  return duallity.wfst(dictionary, query, maximumDistance, algorithm, kind);
}
function configuredWfst(dictionary, query, options) {
  assertConfiguredWfstArguments(arguments);
  assertDictionaryResource(dictionary);
  assertSameRuntime(dictionary, runtimeIdentity);
  if (typeof duallity.configuredWfst !== "function") {
    throw new TypeError("configuredWfst requires a matching JavaScript runtime with the duallity configuration bridge");
  }
  return duallity.configuredWfst(dictionary, query, options);
}
const facade = { ...duallity, runtimeIdentity, wfst, configuredWfst };
module.exports = { ...facade, runtimeIdentity, wfst, configuredWfst, default: facade };
