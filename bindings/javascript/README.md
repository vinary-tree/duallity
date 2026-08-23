# duallity — JavaScript / TypeScript / ClojureScript bindings

`@vinary-tree/duallity` is the JavaScript, TypeScript, and ClojureScript facade for duallity:
**dictionary-backed edit and phonetic WFSTs** (**W**eighted **F**inite-**S**tate **T**ransducers).
Given a dictionary resource and a query, it captures the dictionary once and returns a lazy, composable
WFST resource that hands off in `` $`O(1)`$ `` to `@vinary-tree/lling-llang` composition — no
serialization, no full state-space materialization.

It is a thin skin over the seven-function `duallity_*` C ABI documented in
[docs/architecture/06](../../docs/architecture/06-resource-abi-and-bindings.md); this README is the
JavaScript-specific guide. The task-oriented cross-language walkthrough is
[docs/guides/07 · Language bindings](../../docs/guides/07-language-bindings.md).

## Surface

```ts
import { wfst, runtimeIdentity } from "@vinary-tree/duallity";

wfst(
  dictionary: DictionaryResource,   // a vt.dictionary.v1 resource (UnicodeScalar units)
  query: string,
  maximumDistance: number,
  algorithm?: Algorithm,            // default "standard"
  kind?: WfstKind,                  // default "levenshtein"
): WfstResource;                    // a vt.scalar-wfst.1 resource
```

- **`algorithm`** — `"standard"` | `"transposition"` | `"merge-and-split"` | `"damerau-levenshtein"`
  (consumed by the Levenshtein kind only).
- **`kind`** — one of the nine kinds: `"levenshtein"`, `"universal-standard"`,
  `"universal-transposition"`, `"universal-merge-and-split"`, `"generalized-standard"`,
  `"generalized-transposition"`, `"generalized-merge-and-split"`, `"generalized-phonetic"`, `"fzf"`.
  See [architecture/06 §4](../../docs/architecture/06-resource-abi-and-bindings.md#4-the-nine-automaton-kinds-and-their-algorithms).
- **`runtimeIdentity`** — the guard that keeps a resource on one runtime (native / WASM / WASI); it is
  what makes the same-runtime handoff copy-free.

TypeScript declarations ship in [`index.d.ts`](index.d.ts). A ClojureScript namespace,
`vinary-tree.duallity`, exposes `wfst`, `start`, `state`, and `close!`.

## Install

```sh
npm install @vinary-tree/duallity @vinary-tree/interop
```

Requires **Node 22.14 or newer**. The package depends on `@vinary-tree/vinary-tree` (the umbrella
runtime) and peers on `@vinary-tree/interop`. Entry points: native N-API (the Node default), `./wasm`,
and `./wasi`; a `./typescript` and a `./clojurescript` facade are also exported.

## Quickstart

```js
import { wfst } from "@vinary-tree/duallity";
import { compose } from "@vinary-tree/lling-llang";

// `dictionary` is a DictionaryResource from a @vinary-tree dictionary package.
const edit = wfst(dictionary, "helo", 2, "standard", "levenshtein");

// Compose with any downstream WFST on the same runtime; the handoff is O(1).
const pipeline = compose(edit, languageModel);
// … run a shortest-path search over `pipeline` …

edit.close();   // release the retained resource
```

ClojureScript:

```clojure
(require '[vinary-tree.duallity :as d])

(let [edit (d/wfst dictionary "helo" 2 "standard" "levenshtein")]
  (try
    ;; (d/start edit) / (d/state edit s) walk the lazy WFST
    (finally (d/close! edit))))
```

## Ownership and memory model

`wfst(...)` returns a `WfstResource` that owns **one retain** of the underlying `vt.scalar-wfst.1`
resource. Release it with `close()` (ClojureScript: `close!`). Garbage-collector finalization is a
backstop, not a guarantee — **call `close()`** when you are done, ideally in a `try/finally`.

The `dictionary` argument is **borrowed for the call only**. duallity captures its snapshot exactly
once ([the capture-once rule](../../docs/architecture/06-resource-abi-and-bindings.md#5-the-capture-once-rule)),
so the returned WFST keeps matching against that immutable revision even after you close or mutate the
source dictionary.

## Errors

A failed construction throws; the thrown error carries the boundary message
(`duallity_last_error_message()`) and corresponds to one `DuallityStatus`. The mapping is **total** —
see the [error-mapping totality table](../../docs/guides/07-language-bindings.md#5-error-mapping-totality).
Common cases: a non-`UnicodeScalar` dictionary or stale interop ABI throws `INCOMPATIBLE_RESOURCE`; a
misbehaving dictionary provider throws `PROVIDER_ERROR`; an out-of-range `kind`/`algorithm` or a
`` $`k > 255`$ `` distance for a universal/generalized kind throws `INVALID_ARGUMENT`.

## Concurrency and zero-copy

- **Same-runtime handoff is copy-free.** A `runtimeIdentity` guard ensures a resource composes with
  lling-llang in-process as a handle, not a serialized graph. Mixing native and WASM resources is
  refused rather than silently copied.
- **Capture and handoff are `` $`O(1)`$ ``.** No dictionary terms are copied at construction, and the
  resource is a two-word handle; product states expand lazily during search.
- **The resource is reentrant.** Independent expansions share the registries and the captured snapshot
  behind reference-counted structural sharing ([architecture/06 §6](../../docs/architecture/06-resource-abi-and-bindings.md#6-the-double-adapter-bridge)).

## Version compatibility

| Component | Version |
|-----------|---------|
| `@vinary-tree/duallity` | `4.0.0-rc.1` |
| `@vinary-tree/interop` (peer) | `0.1.0` |
| `@vinary-tree/vinary-tree` (runtime) | `4.0.0-rc.1` |
| Node | `>= 22.14` |
| duallity C ABI | version `1`, revision `1` |

> **Release note.** The family pins `4.0.0-rc.1` ahead of the tag-and-publish event
> ([DUAL-B2](../../docs/scientific-ledger/bindings-findings-ledger.md)); until that release lands, the
> published registry artifact may lag the pin.

## See also

- [docs/guides/07 · Language bindings](../../docs/guides/07-language-bindings.md) — the nine-section cross-language guide.
- [docs/architecture/06 · The resource ABI and language bindings](../../docs/architecture/06-resource-abi-and-bindings.md) — the ABI reference.
- [bindings/cpp/README](../cpp/README.md) — the C++ RAII facade.
- [docs/security/threat-model](../../docs/security/threat-model.md) — why a foreign dictionary is untrusted input.

## Executable conformance evidence

[`test/facades.test.mjs`](test/facades.test.mjs) exercises the public
JavaScript, TypeScript, ClojureScript, native, WebAssembly, and WASI entry points
against an instrumented runtime contract:

```sh
npm test --prefix bindings/javascript
```

It verifies export parity, selector forwarding, runtime-identity/interface
guards, state expansion, and deterministic release without importing
repository-private implementation modules.

## Security and provider trust

Treat resource-like JavaScript objects as untrusted. The facade rejects a
different runtime identity or missing dictionary interface before crossing into
native code. Native construction then validates version/domain metadata, UTF-8,
selectors, provider node/page output, and resource limits. Do not bypass the
guard with private handle fields or move a resource between workers/runtimes.

## Troubleshooting

| Symptom | Likely cause and response |
|---|---|
| different-runtime `TypeError` | Deduplicate the umbrella runtime and use only one of native, WebAssembly, or WASI in a resource domain. |
| incompatible-resource error | Supply a Unicode-scalar `vt.dictionary.v1` object from the same runtime. |
| invalid selector/distance | Check the nine kind strings and each kind's represented maximum distance. |
| native module load failure | Verify Node version, OS/CPU artifact, exact family pins, and reinstall the package. |
| rising native memory | Close every returned WFST in `finally`; GC finalizers are fallback containment only. |

## Maintainer workflow

1. Update [`bindings/api.json`](../api.json), `package.json`, declarations, and every entry point together.
2. Keep JavaScript, TypeScript, and ClojureScript exports and selector semantics identical.
3. Add positive, negative, cross-runtime, and close-after-error cases to `facades.test.mjs`.
4. Run both binding gates, npm tests, and the family pipeline.
5. Validate native, WebAssembly, and WASI packages without weakening runtime identity.
