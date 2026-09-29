# Authoritative WFST cache-control qualification — 29 September 2026

## Question and source boundary

Can duallity expose policy, residency statistics, clear, and policy changes
through the **one cache owned by the exported lling-llang provider resource**?
The tested source is the `codex/binding-integration` completion tree containing
this note. This is an internal revision-3 implementation step: the public
`duallity_api_revision()` still returns `2`, and no new callable C symbol is
advertised until the final foreign-mirror qualification step.

The [ABI contract](../architecture/07-versioned-configurable-wfst-abi.md)
defines the intended cache semantics and the [ownership diagram](../diagrams/duallity-config-ownership.svg)
shows where residency belongs. Duallity's adapter supplies direct immutable
state expansions; `OwnedWfstResource::from_provider_with_cache` installs the
single shared cache. The control helpers in `src/ffi/cache.rs` obtain that
resource's `ProviderCacheControl`; they never allocate a duallity-side state
payload cache. Legacy construction still selects `CacheAll` explicitly.

## Method and expected behavior

The parser represents the caller's policy before construction. Its conversion
to the lling-llang policy resolves the legacy zero-capacity convention once:

| Requested policy | Native parameterized/universal/generalized | FZF |
| --- | --- | --- |
| CacheAll | CacheAll | CacheAll |
| NoCache | NoCache | NoCache |
| LRU with capacity 0 | LRU with effective capacity 100000 | NoCache |
| LRU with capacity 1 or 2 | Exact stated capacity | Exact stated capacity |

The effective selector and capacity are read from the resource's current
cache-control authority. Changing policy publishes an empty generation;
clearing also publishes an empty generation without changing dictionary
snapshot or state identity. Cumulative counters are copied from
`SharedCacheStatistics` into the staged C record; residency and recency counts
come from one published root. This is not a transactional snapshot of all
concurrent counters, as specified in the [cache contract](../architecture/07-versioned-configurable-wfst-abi.md#one-cache-owner-and-observable-controls).

The tests use both a real captured duallity dictionary adapter and a small
concurrent provider. The provider calls `clear()` from within its own state
callback, so a cache lock held across callbacks would deadlock that test.
Separate tests force admission rejection and provider failure to check that
neither result is retained or mislabeled as a hit. A retained resource clone
must see the same policy, counters, and clear generation as the original.

## Reproduction and observations

The primary `../lling-llang` checkout was owned by another agent and reported
version `0.2.0`; this duallity worktree requires `4.0.0-rc.6` and its shared
cache-control API. For the test run only, the relative path dependency used
the clean `../lling-llang-vco-integration` worktree. The ordinary relative
manifest path was restored before committing. No primary lling-llang edits
were made, and no path to a feature worktree is published in the manifest.

The Rust commands ran under a user systemd scope with `MemoryMax=4G`,
`MemorySwapMax=0`, `CPUQuota=100%`, `TasksMax=64`, and `IOWeight=30`. Their
target directory was disk-backed under the liblevenshtein-rust workspace,
not `/tmp`.

```sh
cargo test --locked --features ffi --lib --quiet
cargo clippy --locked --features ffi --lib --tests -- -D warnings
python3 scripts/check-bindings.py
python3 scripts/check-binding-docs.py
git diff --check
```

The final FFI-enabled library suite passed **184/184** tests; six focused
cache tests passed. Strict Clippy, the binding model's **74/74** checks, and
the documentation checker for **six** facades passed. For LRU capacity one,
two distinct state requests left one resident record and caused an eviction;
for capacity two, three requests left two resident records and caused an
additional eviction. CacheAll retained all three requested states. NoCache
retained zero states and recomputed repeated requests. Invalid and faulting
states incremented separate `uncacheable_results` and `faults` counters and
left residency at zero. Eight concurrent clones made 128 ABI state requests
while a provider callback reentered `clear()`, completing without deadlock.

## Limits of the evidence

The synthetic reentrancy test exercises exactly the exported provider-cache
control used by duallity; it does not prove every foreign dictionary callback
is safe or every interleaving is covered. The C API's sized output writes,
inspection-buffer ownership, public symbol/version negotiation, foreign
mirrors, and old/new C consumers remain to be qualified atomically. No
performance claim or RC6 package publication follows from these tests.
