import { duallity } from "@vinary-tree/javascript-runtime";
import { assertDictionaryResource, assertSameRuntime } from "@vinary-tree/vinary-tree-interop";
import { assertLegacyWfstArguments, assertConfiguredWfstArguments } from "./wfst-arguments.mjs";

export const runtimeIdentity = duallity.runtimeIdentity;
export function wfst(dictionary, query, maximumDistance, algorithm, kind) {
  assertLegacyWfstArguments(arguments);
  assertDictionaryResource(dictionary);
  assertSameRuntime(dictionary, runtimeIdentity);
  return duallity.wfst(dictionary, query, maximumDistance, algorithm, kind);
}
export function configuredWfst(dictionary, query, options) {
  assertConfiguredWfstArguments(arguments);
  assertDictionaryResource(dictionary);
  assertSameRuntime(dictionary, runtimeIdentity);
  if (typeof duallity.configuredWfst !== "function") {
    throw new TypeError("configuredWfst requires a matching JavaScript runtime with the duallity configuration bridge");
  }
  return duallity.configuredWfst(dictionary, query, options);
}
export default { ...duallity, runtimeIdentity, wfst, configuredWfst };
