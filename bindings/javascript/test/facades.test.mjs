import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire, Module } from "node:module";
import test from "node:test";

const root = new URL("../", import.meta.url);
const packageJson = JSON.parse(await readFile(new URL("package.json", root)));

test("all duallity facades select the shared JavaScript runtime", () => {
  assert.equal(packageJson.name, "@vinary-tree/duallity");
  assert.equal(packageJson.dependencies["@vinary-tree/javascript-runtime"], "4.0.0-rc.6");
  for (const entry of [".", "./typescript", "./clojurescript", "./wasm", "./wasi"]) {
    assert.ok(packageJson.exports[entry]);
  }
});

test("ClojureScript exposes an idiomatic lazy WFST facade", async () => {
  const source = await readFile(new URL("cljs/vinary_tree/duallity.cljs", root), "utf8");
  for (const name of ["wfst", "start", "state", "close!"]) {
    assert.ok(source.includes(`(defn ${name}`), `missing ${name}`);
  }
  assert.match(source, /remove supported-wfst-options \(keys options\)/);
  assert.match(source, /Configured WFST options and cache controls are unavailable/);
});

const dataUrl = (source) => `data:text/javascript,${encodeURIComponent(source)}`;
const identity = "duallity-test-runtime";
const runtimeSource = `
  export const duallity = {
    runtimeIdentity: ${JSON.stringify(identity)},
    wfst(...args) { return args; }
  };`;
const interopSource = `
  export function assertDictionaryResource(dictionary) {
    if (!dictionary || dictionary.type !== "vt.dictionary.v1") throw new TypeError("dictionary resource required");
  }
  export function assertSameRuntime(dictionary, identity) {
    if (dictionary.runtimeIdentity !== identity) throw new TypeError("different runtime");
  }`;

async function loadIsolatedEsmFacade(host) {
  const helper = await readFile(new URL("facades/legacy-wfst-options.mjs", root), "utf8");
  const source = await readFile(new URL(`facades/${host}.mjs`, root), "utf8");
  const runtimeSpecifier = host === "native" ?
    "@vinary-tree/javascript-runtime" : `@vinary-tree/javascript-runtime/${host}`;
  assert.ok(source.includes(`"${runtimeSpecifier}"`), `missing ${host} runtime import`);
  return import(dataUrl(source
    .replace(`"${runtimeSpecifier}"`, `"${dataUrl(runtimeSource)}"`)
    .replace('"@vinary-tree/vinary-tree-interop"', `"${dataUrl(interopSource)}"`)
    .replace('"./legacy-wfst-options.mjs"', `"${dataUrl(helper)}"`)));
}

for (const host of ["native", "wasm", "wasi"]) {
  test(`${host} ESM facade preserves positional calls and rejects unsupported configuration`, async () => {
    const facade = await loadIsolatedEsmFacade(host);
    const dictionary = { type: "vt.dictionary.v1", runtimeIdentity: identity };
    assert.deepEqual(facade.wfst(dictionary, "helo", 2, "standard", "levenshtein"),
      [dictionary, "helo", 2, "standard", "levenshtein"]);
    assert.equal(facade.default.wfst, facade.wfst);
    assert.throws(() => facade.wfst(dictionary, "helo", 2, { cachePolicy: "lru" }),
      /Configured WFST options and cache controls are unavailable/);
    assert.throws(() => facade.wfst(dictionary, "helo", 2, "standard", { limits: {} }),
      /Configured WFST options and cache controls are unavailable/);
    assert.throws(() => facade.wfst(dictionary, "helo", 2, "standard", "levenshtein", {}),
      /Configured WFST options and cache controls are unavailable/);
    assert.throws(() => facade.wfst({ ...dictionary, runtimeIdentity: "other" }, "helo", 2),
      /different runtime/);
  });
}

test("CommonJS and its default facade enforce the same absence and runtime guards", () => {
  const require = createRequire(import.meta.url);
  const originalLoad = Module._load;
  Module._load = function (request, parent, isMain) {
    if (request === "@vinary-tree/javascript-runtime") {
      return { duallity: { runtimeIdentity: identity, wfst: (...args) => args } };
    }
    if (request === "@vinary-tree/vinary-tree-interop") {
      return {
        assertDictionaryResource: (dictionary) => {
          if (dictionary?.type !== "vt.dictionary.v1") throw new TypeError("dictionary resource required");
        },
        assertSameRuntime: (dictionary, expected) => {
          if (dictionary.runtimeIdentity !== expected) throw new TypeError("different runtime");
        }
      };
    }
    return originalLoad.call(this, request, parent, isMain);
  };
  let facade;
  try {
    facade = require("../facades/native.cjs");
  } finally {
    Module._load = originalLoad;
  }
  const dictionary = { type: "vt.dictionary.v1", runtimeIdentity: identity };
  assert.deepEqual(facade.wfst(dictionary, "helo", 2), [dictionary, "helo", 2, undefined, undefined]);
  for (const entry of [facade, facade.default]) {
    assert.throws(() => entry.wfst(dictionary, "helo", 2, { operations: [] }),
      /Configured WFST options and cache controls are unavailable/);
    assert.throws(() => entry.wfst({ ...dictionary, runtimeIdentity: "other" }, "helo", 2),
      /different runtime/);
  }
});
