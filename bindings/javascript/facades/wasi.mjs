import { duallity } from "@vinary-tree/vinary-tree/wasi";
import { assertDictionaryResource, assertSameRuntime } from "@vinary-tree/interop";

export const runtimeIdentity = duallity.runtimeIdentity;
export function wfst(dictionary, query, maximumDistance, algorithm, kind) {
  assertDictionaryResource(dictionary);
  assertSameRuntime(dictionary, runtimeIdentity);
  return duallity.wfst(dictionary, query, maximumDistance, algorithm, kind);
}
export default { ...duallity, runtimeIdentity, wfst };
