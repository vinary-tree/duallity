import type { DictionaryResource, RuntimeIdentity, WfstResource } from "@vinary-tree/vinary-tree-interop";

export type Algorithm = "standard" | "transposition" | "merge-and-split" | "damerau-levenshtein";
export type WfstKind =
  | "levenshtein"
  | "universal-standard" | "universal-transposition" | "universal-merge-and-split"
  | "generalized-standard" | "generalized-transposition"
  | "generalized-merge-and-split" | "generalized-phonetic" | "fzf";
export interface DuallityNamespace {
  readonly runtimeIdentity: RuntimeIdentity;
  wfst(dictionary: DictionaryResource, query: string, maximumDistance: number,
       algorithm?: Algorithm, kind?: WfstKind): WfstResource;
}
export const runtimeIdentity: RuntimeIdentity;
export function wfst(dictionary: DictionaryResource, query: string, maximumDistance: number,
                     algorithm?: Algorithm, kind?: WfstKind): WfstResource;
declare const duallity: DuallityNamespace;
export default duallity;
