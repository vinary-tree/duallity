// The shared JavaScript runtime currently exports only the positional WFST API.
// Reject configuration objects/extra arguments until its versioned bridge exists.
export function assertLegacyWfstArguments(args) {
  if (args.length > 5 ||
      (args[3] !== undefined && typeof args[3] !== "string") ||
      (args[4] !== undefined && typeof args[4] !== "string")) {
    throw new TypeError(
      "Configured WFST options and cache controls are unavailable in this JavaScript runtime; use the five-argument positional wfst API"
    );
  }
}
