import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
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
});
