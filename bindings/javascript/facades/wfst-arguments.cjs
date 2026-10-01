"use strict";
// Keep CommonJS and ESM positional calls fail-closed on configuration objects.
function assertLegacyWfstArguments(args) {
  if (args.length > 5 ||
      (args[3] !== undefined && typeof args[3] !== "string") ||
      (args[4] !== undefined && typeof args[4] !== "string")) {
    throw new TypeError(
      "wfst accepts only five positional arguments; use configuredWfst for options and cache controls"
    );
  }
}
function assertConfiguredWfstArguments(args) {
  if (args.length !== 3 || args[2] === null || typeof args[2] !== "object" ||
      Array.isArray(args[2])) {
    throw new TypeError("configuredWfst requires exactly three arguments and an options object");
  }
}
module.exports = { assertLegacyWfstArguments, assertConfiguredWfstArguments };
