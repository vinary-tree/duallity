# duallity — JavaScript / TypeScript / ClojureScript bindings

`@vinary-tree/duallity` is the JavaScript, TypeScript, and ClojureScript facade for duallity:
**dictionary-backed edit and phonetic WFSTs** (**W**eighted **F**inite-**S**tate **T**ransducers).
Given a dictionary resource and a query, it captures the dictionary once and returns a lazy, composable
WFST resource that hands off in $`\mathcal{O}(1)`$ to `@vinary-tree/lling-llang` composition — no
serialization, no full state-space materialization.

It is a facade over the `duallity_*` C ABI documented in
[docs/architecture/06](../../docs/architecture/06-resource-abi-and-bindings.md) and the
[versioned configuration contract](../../docs/architecture/07-versioned-configurable-wfst-abi.md);
this README is the JavaScript-specific guide. The task-oriented cross-language walkthrough is
[docs/guides/07 · Language bindings](../../docs/guides/07-language-bindings.md).

The native C library reports ABI version 1, API revision 4. Its configured WFST constructor
and cache controls were added at API revision 3. The shared JavaScript runtime now bridges
them for native N-API, browser WebAssembly, and Node WASI; this RC.6 development candidate
exposes that bridge but has **not been published**. The positional constructor remains
unchanged. Passing a configuration object or sixth argument to `wfst` throws a `TypeError`;
use `configuredWfst` instead. An older runtime missing the bridge also fails explicitly.

## Surface

```ts
import { wfst, runtimeIdentity } from "@vinary-tree/duallity";

wfst(
  dictionary: DictionaryResource,   // a vt.dictionary.v1 resource (UnicodeScalar units)
  query: string,
  maximumDistance: number,
  algorithm?: Algorithm,            // default "standard"
  kind?: WfstKind,                  // default "levenshtein"
): Wfst;                            // a vt.scalar-wfst.1 resource

configuredWfst(
  dictionary: DictionaryResource,
  query: string,
  options: DuallityWfstOptions,
): ConfiguredDuallityWfst;         // Wfst plus native-effective options/cache controls
```

- **`algorithm`** — `"standard"` | `"transposition"` | `"merge-and-split"` | `"damerau-levenshtein"`
  (consumed by the Levenshtein kind only).
- **`kind`** — one of the nine kinds: `"levenshtein"`, `"universal-standard"`,
  `"universal-transposition"`, `"universal-merge-and-split"`, `"generalized-standard"`,
  `"generalized-transposition"`, `"generalized-merge-and-split"`, `"generalized-phonetic"`, `"fzf"`.
  See [architecture/06 §4](../../docs/architecture/06-resource-abi-and-bindings.md#4-the-nine-automaton-kinds-and-their-algorithms).
- **`runtimeIdentity`** — the guard that keeps a resource on one runtime (native / WASM / WASI); it is
  what makes the same-runtime handoff copy-free.
- **`configuredWfst`** — creates a WFST with an explicit kind, distance, cache policy,
  generalized limits, or custom operation grammar. The shared runtime validates every
  option before entering the native ABI; unknown fields and unsupported combinations fail.
  The result exposes `options`, `cacheStatistics`, `clearCache()`, and
  `setCachePolicy(policy, capacity?)`.

TypeScript declarations ship in [`index.d.ts`](index.d.ts), importing the canonical option,
statistics, and configured-resource types from the shared runtime rather than duplicating
them. The ClojureScript namespace `vinary-tree.duallity` exposes `wfst`,
`configured-wfst`, `wfst-options`, `cache-statistics`, `clear-cache!`,
`set-cache-policy!`, `start`, `state`, and `close!`. Its positional `wfst` map
still accepts only `:algorithm` and `:kind`; the distinct configured constructor
accepts kebab-case keys such as `:cache-policy` and `:maximum-distance` and
converts them to the runtime's camel-case transport without changing selector values.

## Install

```sh
npm install @vinary-tree/duallity @vinary-tree/vinary-tree-interop
```

Requires **Node 22.14 or newer**. The package depends on `@vinary-tree/javascript-runtime` (the shared
runtime) and `@vinary-tree/vinary-tree-interop`. Entry points: native N-API (the Node default), `./wasm`,
and `./wasi`; a `./typescript` and a `./clojurescript` facade are also exported.

## Quickstart

```js
import { wfst } from "@vinary-tree/duallity";

// Pass a same-runtime DictionaryResource from a @vinary-tree dictionary package.
export function withEditWfst(dictionary, consume) {
  const edit = wfst(dictionary, "helo", 2, "standard", "levenshtein");
  try {
    return consume(edit); // e.g. hand off to a same-runtime lling-llang pipeline
  } finally {
    edit.close();
  }
}
```

Configured construction, native-effective read-back, and one provider-owned cache:

```js
import { configuredWfst } from "@vinary-tree/duallity";

export function withConfiguredEdit(dictionary, consume) {
  const edit = configuredWfst(dictionary, "café", {
    kind: "generalized-standard",
    maximumDistance: 1,
    cachePolicy: "lru",
    cacheCapacity: 512,
    operations: [
      { name: "equal", consumeX: 1, consumeY: 1, weight: 0,
        applicability: "equal" },
      { name: "accent", consumeX: 1, consumeY: 1, weight: 1,
        applicability: "listed", restrictions: [{ source: "é", target: "e" }] },
    ],
  });
  try {
    const applied = edit.options; // copied, effective native configuration
    const before = edit.cacheStatistics; // lossless bigint counters
    edit.clearCache().setCachePolicy("none");
    return consume(edit, applied, before);
  } finally {
    edit.close();
  }
}
```

`DuallityWfstOptions` accepts `kind`, `algorithm`, `maximumDistance`,
`cachePolicy`, `cacheCapacity`, `limits`, and `operations`. Policies are
`"all"`, `"none"`, and `"lru"`; only LRU accepts a nonzero capacity. A zero
LRU capacity selects the native default, while FZF normalizes it to no cache.
Only generalized kinds accept limits and operations. When supplied, `limits`
must contain all eight ceilings defined in `DuallityGeneralizedLimits`.
Custom operations require a nonempty name, finite nonnegative weight, and
positive total scalar consumption; listed applicability requires explicit
source/target restrictions with matching Unicode-scalar widths. FZF requires
distance zero; universal and generalized distances fit one byte. The
[shared runtime API reference](https://github.com/vinary-tree/javascript-runtime/blob/master/docs/api-reference.md#duallity)
lists the exact fields and bounds.

`edit.options` reports the effective native policy, capacity, limits, and
owned operation grammar. `edit.cacheStatistics` reports ten `bigint` counters,
including `hits`, `misses`, `evictions`, `clears`, and `residentStates`.
`clearCache()` evicts residency without changing the captured dictionary
snapshot or WFST semantics. `setCachePolicy()` publishes the new policy and
clears the prior generation. Controls are available only on a configured
duallity WFST, not on a positional or composed WFST, and throw after close.

ClojureScript:

```clojure
(require '[vinary-tree.duallity :as d])

(let [edit (d/configured-wfst dictionary "café"
              {:kind :generalized-standard
               :maximum-distance 1
               :cache-policy :lru
               :cache-capacity 128
               :operations [{:name "equal" :consume-x 1 :consume-y 1
                             :weight 0 :applicability :equal}]})]
  (try
    (let [options (d/wfst-options edit)
          counters (d/cache-statistics edit)]
      (d/clear-cache! edit)
      (d/set-cache-policy! edit :none)
      ;; (d/start edit) / (d/state edit s) walk the lazy WFST.
      [options counters])
    (finally (d/close! edit))))
```

The ClojureScript accessors return nested maps with kebab-case keywords;
statistics remain JavaScript `BigInt` values so no unsigned 64-bit precision
is lost. Explicit `close!` remains required.

## Ownership and memory model

`wfst(...)` and `configuredWfst(...)` return `Wfst` resources that own **one retain** of the
underlying `vt.scalar-wfst.1` resource. Release either with `close()` (ClojureScript: `close!`).
Garbage-collector finalization is a
backstop, not a guarantee — **call `close()`** when you are done, ideally in a `try/finally`.

The `dictionary` argument is **borrowed for the call only**. duallity captures its snapshot exactly
once ([the capture-once rule](../../docs/architecture/06-resource-abi-and-bindings.md#5-the-capture-once-rule)),
so the returned WFST keeps matching against that immutable revision even after you close or mutate the
source dictionary.

## Errors

A failed native construction throws; the thrown error carries the boundary message
(`duallity_last_error_message()`) and corresponds to one `DuallityStatus`. The mapping is **total** —
see the [error-mapping totality table](../../docs/guides/07-language-bindings.md#5-error-mapping-totality).
Common cases: a non-`UnicodeScalar` dictionary or stale interop ABI throws `INCOMPATIBLE_RESOURCE`; a
misbehaving dictionary provider throws `PROVIDER_ERROR`; an out-of-range `kind`/`algorithm` or a
$`k > 255`$ distance for a universal/generalized kind throws `INVALID_ARGUMENT`.
Malformed configuration is rejected by the shared runtime with `TypeError` or
`RangeError` before any native call. Incompatible grammar or resource metadata
is rejected by the native ABI; those errors preserve their `DuallityStatus`.

## Concurrency and zero-copy

- **Same-runtime handoff is copy-free.** A `runtimeIdentity` guard ensures a resource composes with
  lling-llang in-process as a handle, not a serialized graph. Mixing native and WASM resources is
  refused rather than silently copied.
- **Capture and handoff are $`\mathcal{O}(1)`$.** No dictionary terms are copied at construction, and the
  resource is a two-word handle; product states expand lazily during search.
- **The resource is reentrant.** Independent expansions share the registries and the captured snapshot
  behind reference-counted structural sharing ([architecture/06 §6](../../docs/architecture/06-resource-abi-and-bindings.md#6-the-double-adapter-bridge)).

## Version compatibility

| Component | Version |
|-----------|---------|
| `@vinary-tree/duallity` | `4.0.0-rc.6` |
| `@vinary-tree/vinary-tree-interop` (dependency) | `4.0.0-rc.6` |
| `@vinary-tree/javascript-runtime` (runtime) | `4.0.0-rc.6` |
| Node | `>= 22.14` |
| duallity C ABI | version `1`, API revision `4` (configured controls start at revision `3`) |

> **Release policy.** Evaluate the candidate through the exact
> `4.0.0-rc.6` version or npm's `next` tag. The package and both shared
> dependencies are exact pins so a mixed runtime family fails during
> resolution rather than at resource handoff.

## See also

- [docs/guides/07 · Language bindings](../../docs/guides/07-language-bindings.md) — the nine-section cross-language guide.
- [docs/architecture/06 · The resource ABI and language bindings](../../docs/architecture/06-resource-abi-and-bindings.md) — the ABI reference.
- [bindings/cpp/README](../cpp/README.md) — the C++ RAII facade.
- [docs/security/threat-model](../../docs/security/threat-model.md) — why a foreign dictionary is untrusted input.

## Executable conformance evidence

[`test/facades.test.mjs`](test/facades.test.mjs) checks package exports,
the ClojureScript option guard, and native/CommonJS, browser-WebAssembly,
and WASI JavaScript facade behavior against an isolated runtime double:

```sh
npm test --prefix bindings/javascript
```

These tests verify positional compatibility, configured forwarding, absent-bridge
failures, and same-runtime guards for all entry points. The shared runtime's
native, browser-WebAssembly, and WASI conformance suites test effective
options, cache statistics and controls, and native state expansion. The isolated
facade tests alone do **not** qualify an installed RC.6 package; a fresh
installed-consumer run against the final artifacts remains required.

## Security and provider trust

Treat resource-like JavaScript objects as untrusted. The facade rejects a
different runtime identity or missing dictionary interface before crossing into
native code. Native construction then validates version/domain metadata, UTF-8,
selectors, provider node/page output, and resource limits. Do not bypass the
guard with private handle fields or move a resource between workers/runtimes.

## Troubleshooting

| Symptom | Likely cause and response |
|---|---|
| different-runtime `TypeError` | Deduplicate the shared runtime and use only one of native, WebAssembly, or WASI in a resource domain. |
| incompatible-resource error | Supply a Unicode-scalar `vt.dictionary.v1` object from the same runtime. |
| invalid selector/distance | Check the nine kind strings and each kind's represented maximum distance. |
| positional options `TypeError` | Pass options to `configuredWfst` (ClojureScript: `configured-wfst`), not to positional `wfst`. |
| missing bridge `TypeError` | Install one matching shared-runtime build containing configured duallity support; mixed RC.6 development artifacts are not interchangeable. |
| invalid configuration `TypeError` or `RangeError` | Check exact option names, selector combinations, operation grammar, and cache-capacity rules before retrying. |
| native module load failure | Verify Node version, OS/CPU artifact, exact family pins, and reinstall the package. |
| rising native memory | Close every returned WFST in `finally`; GC finalizers are fallback containment only. |

## Maintainer workflow

1. Update [`bindings/api.json`](../api.json), `package.json`, declarations, and every entry point together.
2. Keep configured controls backed by the shared JavaScript runtime in native-addon, browser-WebAssembly, and WASI hosts; never duplicate its parser or cache in this facade.
3. Keep JavaScript, TypeScript, and ClojureScript exports and selector semantics identical. Add positive, negative, cross-runtime, and close-after-error cases to `facades.test.mjs` and installed-consumer tests.
4. Run both binding gates, npm tests, and the family pipeline.
5. Validate native, WebAssembly, and WASI packages without weakening runtime identity.
