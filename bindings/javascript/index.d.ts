import type { DictionaryResource, RuntimeIdentity } from "@vinary-tree/vinary-tree-interop";
import type {
  Algorithm as RuntimeAlgorithm,
  ConfiguredDuallityWfst,
  DuallityWfstKind,
  DuallityWfstOptions,
  Wfst,
} from "@vinary-tree/javascript-runtime";

export type {
  ConfiguredDuallityWfst,
  DuallityCachePolicy,
  DuallityCacheStatistics,
  DuallityGeneralizedLimits,
  DuallityOperation,
  DuallityOperationApplicability,
  DuallityWfstOptions,
} from "@vinary-tree/javascript-runtime";

export type Algorithm = RuntimeAlgorithm;
export type WfstKind = DuallityWfstKind;
export interface DuallityNamespace {
  readonly runtimeIdentity: RuntimeIdentity;
  /** Native-effective options, cache statistics, and controls are on the returned resource. */
  configuredWfst(dictionary: DictionaryResource, query: string,
                 options: DuallityWfstOptions): ConfiguredDuallityWfst;
  wfst(dictionary: DictionaryResource, query: string, maximumDistance: number,
       algorithm?: Algorithm, kind?: WfstKind): Wfst;
}
export const runtimeIdentity: RuntimeIdentity;
export function configuredWfst(dictionary: DictionaryResource, query: string,
                               options: DuallityWfstOptions): ConfiguredDuallityWfst;
export function wfst(dictionary: DictionaryResource, query: string, maximumDistance: number,
                     algorithm?: Algorithm, kind?: WfstKind): Wfst;
declare const duallity: DuallityNamespace;
export default duallity;
