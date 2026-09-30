import { duallity } from "@vinary-tree/javascript-runtime";
import { assertDictionaryResource, assertSameRuntime } from "@vinary-tree/vinary-tree-interop";
import { assertLegacyWfstArguments } from "./legacy-wfst-options.mjs";

export const runtimeIdentity = duallity.runtimeIdentity;
export function wfst(dictionary, query, maximumDistance, algorithm, kind) {
  assertLegacyWfstArguments(arguments);
  assertDictionaryResource(dictionary);
  assertSameRuntime(dictionary, runtimeIdentity);
  return duallity.wfst(dictionary, query, maximumDistance, algorithm, kind);
}
export default { ...duallity, runtimeIdentity, wfst };
