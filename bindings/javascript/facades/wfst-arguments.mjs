// The positional and configured constructors are distinct APIs. Never let an
// object or extra argument be silently ignored by the positional constructor.
export function assertLegacyWfstArguments(args) {
  if (args.length > 5 ||
      (args[3] !== undefined && typeof args[3] !== "string") ||
      (args[4] !== undefined && typeof args[4] !== "string")) {
    throw new TypeError(
      "wfst accepts only five positional arguments; use configuredWfst for options and cache controls"
    );
  }
}

export function assertConfiguredWfstArguments(args) {
  if (args.length !== 3 || args[2] === null || typeof args[2] !== "object" ||
      Array.isArray(args[2])) {
    throw new TypeError("configuredWfst requires exactly three arguments and an options object");
  }
}
