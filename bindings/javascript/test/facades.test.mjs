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

test("ClojureScript exposes positional and configured WFST facades", async () => {
  const source = await readFile(new URL("cljs/vinary_tree/duallity.cljs", root), "utf8");
  for (const name of [
    "wfst", "configured-wfst", "wfst-options", "cache-statistics",
    "clear-cache!", "set-cache-policy!", "start", "state", "close!",
  ]) {
    assert.ok(source.includes(`(defn ${name}`), `missing ${name}`);
  }
  assert.match(source, /remove supported-wfst-options \(keys options\)/);
  assert.match(source, /native\/configuredWfst/);
  assert.match(source, /options->js/);
  assert.match(source, /native->clj/);
});

const dataUrl = (source) => `data:text/javascript,${encodeURIComponent(source)}`;
const identity = "duallity-test-runtime";
const configuredRuntimeSource = `
  export const duallity = {
    runtimeIdentity: ${JSON.stringify(identity)},
    wfst(...args) { return args; },
    configuredWfst(...args) {
      const options = args[2];
      return {
        args,
        options,
        cacheStatistics: { hits: 0n, clears: 0n },
        clearCache() { this.cacheStatistics.clears += 1n; return this; },
        setCachePolicy(policy, capacity = 0) {
          this.options = { ...this.options, cachePolicy: policy, cacheCapacity: capacity };
          return this;
        },
      };
    }
  };`;
const interopSource = `
  export function assertDictionaryResource(dictionary) {
    if (!dictionary || dictionary.type !== "vt.dictionary.v1") throw new TypeError("dictionary resource required");
  }
  export function assertSameRuntime(dictionary, identity) {
    if (dictionary.runtimeIdentity !== identity) throw new TypeError("different runtime");
  }`;

async function loadIsolatedEsmFacade(host, runtimeSource = configuredRuntimeSource) {
  const helper = await readFile(new URL("facades/wfst-arguments.mjs", root), "utf8");
  const source = await readFile(new URL(`facades/${host}.mjs`, root), "utf8");
  const runtimeSpecifier = host === "native" ?
    "@vinary-tree/javascript-runtime" : `@vinary-tree/javascript-runtime/${host}`;
  assert.ok(source.includes(`"${runtimeSpecifier}"`), `missing ${host} runtime import`);
  return import(dataUrl(source
    .replace(`"${runtimeSpecifier}"`, `"${dataUrl(runtimeSource)}"`)
    .replace('"@vinary-tree/vinary-tree-interop"', `"${dataUrl(interopSource)}"`)
    .replace('"./wfst-arguments.mjs"', `"${dataUrl(helper)}"`)));
}

for (const host of ["native", "wasm", "wasi"]) {
  test(`${host} ESM facade preserves positional calls and forwards configured controls`, async () => {
    const facade = await loadIsolatedEsmFacade(host);
    const dictionary = { type: "vt.dictionary.v1", runtimeIdentity: identity };
    assert.deepEqual(facade.wfst(dictionary, "helo", 2, "standard", "levenshtein"),
      [dictionary, "helo", 2, "standard", "levenshtein"]);
    assert.equal(facade.default.wfst, facade.wfst);
    const options = {
      kind: "generalized-standard", maximumDistance: 1,
      cachePolicy: "lru", cacheCapacity: 3,
    };
    const configured = facade.configuredWfst(dictionary, "helo", options);
    assert.deepEqual(configured.args, [dictionary, "helo", options]);
    assert.equal(facade.default.configuredWfst, facade.configuredWfst);
    assert.equal(configured.options.cachePolicy, "lru");
    assert.equal(configured.clearCache(), configured);
    assert.equal(configured.cacheStatistics.clears, 1n);
    assert.equal(configured.setCachePolicy("none"), configured);
    assert.equal(configured.options.cachePolicy, "none");
    assert.throws(() => facade.wfst(dictionary, "helo", 2, { cachePolicy: "lru" }),
      /use configuredWfst/);
    assert.throws(() => facade.wfst(dictionary, "helo", 2, "standard", { limits: {} }),
      /use configuredWfst/);
    assert.throws(() => facade.wfst(dictionary, "helo", 2, "standard", "levenshtein", {}),
      /use configuredWfst/);
    assert.throws(() => facade.configuredWfst(dictionary, "helo"), /exactly three arguments/);
    assert.throws(() => facade.configuredWfst(dictionary, "helo", null), /options object/);
    assert.throws(() => facade.configuredWfst(dictionary, "helo", []), /options object/);
    assert.throws(() => facade.configuredWfst(dictionary, "helo", options, "ignored"),
      /exactly three arguments/);
    assert.throws(() => facade.configuredWfst({ ...dictionary, runtimeIdentity: "other" }, "helo", options),
      /different runtime/);
    assert.throws(() => facade.configuredWfst({ runtimeIdentity: identity }, "helo", options),
      /dictionary resource required/);
    assert.throws(() => facade.wfst({ ...dictionary, runtimeIdentity: "other" }, "helo", 2),
      /different runtime/);
  });

  test(`${host} ESM facade fails closed against a runtime missing the config bridge`, async () => {
    const facade = await loadIsolatedEsmFacade(host,
      `export const duallity = { runtimeIdentity: ${JSON.stringify(identity)}, wfst() {} };`);
    const dictionary = { type: "vt.dictionary.v1", runtimeIdentity: identity };
    assert.throws(() => facade.configuredWfst(dictionary, "helo", {}), /matching JavaScript runtime/);
  });
}

test("CommonJS and its default facade enforce the same config and runtime guards", () => {
  const require = createRequire(import.meta.url);
  const originalLoad = Module._load;
  Module._load = function (request, parent, isMain) {
    if (request === "@vinary-tree/javascript-runtime") {
      return { duallity: {
        runtimeIdentity: identity,
        wfst: (...args) => args,
        configuredWfst: (...args) => ({ args, options: args[2],
          cacheStatistics: { hits: 0n },
          clearCache() { return this; },
          setCachePolicy() { return this; } }),
      } };
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
      /use configuredWfst/);
    const options = { kind: "fzf", maximumDistance: 0, cachePolicy: "none" };
    const configured = entry.configuredWfst(dictionary, "cat", options);
    assert.deepEqual(configured.args, [dictionary, "cat", options]);
    assert.equal(configured.clearCache(), configured);
    assert.equal(configured.setCachePolicy("none"), configured);
    assert.throws(() => entry.configuredWfst(dictionary, "cat", null), /options object/);
    assert.throws(() => entry.configuredWfst({ ...dictionary, runtimeIdentity: "other" }, "cat", options),
      /different runtime/);
    assert.throws(() => entry.wfst({ ...dictionary, runtimeIdentity: "other" }, "helo", 2),
      /different runtime/);
  }
});
