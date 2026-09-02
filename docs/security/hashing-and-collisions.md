# Hashing and collisions

> **Defines:** the exact `DictionaryNodeKey`, its bit layout, the distinction between *logical node
> identity* and *hash-table lookup*, and why ordinary hash collisions cannot alias two dictionary
> nodes. **Symbols** are from the [master notation](../theory/README.md#master-notation).

duallity gives dictionary nodes stable integer ids **without** relying on a probabilistic path hash.
This page derives the current exact key from the source, proves that it is injective (so distinct
nodes never share an id), separates the two things the word "collision" can mean, and lays out the
remaining hardening path.

## 1. Why nodes need ids, and the mechanism

A product `StateId` packs a `u32` **dictionary-node component** with a `u32` automaton component
([architecture/03](../architecture/03-state-encoding-and-product-space.md)). But a dictionary node
arrives from libdictenstein as an opaque handle with **no inherent integer identity**. Lazy state
sources therefore *intern* each node the first time they reach it, keying it by the **exact path step**
$`(\text{parent id},\ \text{edge label})`$ that discovered it. The key is compact and exact, not
a digest:

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DictionaryNodeKey(u64);

const CHAR_BITS: u64 = 21;
const CHAR_MASK: u64 = (1 << CHAR_BITS) - 1;   // 2^21 - 1

impl DictionaryNodeKey {
    pub(crate) const ROOT: Self = Self(u64::MAX);

    pub(crate) fn child(parent_id: u32, edge_label: char) -> Self {
        let codepoint = edge_label as u64;          // Unicode scalar value ≤ 0x10FFFF < 2^21
        debug_assert!(codepoint <= CHAR_MASK);
        Self((u64::from(parent_id) << CHAR_BITS) | codepoint)
    }
}
```

The registry stores a forward map from this key to a dense id and a reverse vector back to the node
([`node_registry.rs`](../architecture/05-registries-and-interning.md)):

```rust,ignore
node_to_id: FxHashMap<DictionaryNodeKey, u32>,   // forward: exact key → dense id
id_to_node: Vec<N>,                              // reverse: id → dictionary node
```

The root is registered under `ROOT` and receives id `0`; every other node is registered under a
`child(parent_id, edge_label)` key on first sight.

<img src="../diagrams/noderegistry-interning.svg" alt="A dictionary node is keyed by its exact parent-and-edge path step into a u32 id; the reverse vector recovers the node, and ordinary hash-table collisions do not alias nodes" width="820"/>

## 2. The packing formula and bit layout

Writing $`p`$ for the parent id (a previously-interned node's `u32`), $`\ell`$ for the edge
label (a Rust `char`), and $`\operatorname{cp}(\ell) = \ell \text{ as } \mathtt{u64}`$ for its
Unicode scalar value, the constructor computes

```math
\operatorname{child}(p,\,\ell) \;=\; \bigl(p \ll 21\bigr)\ \mathbin{\vert}\ \operatorname{cp}(\ell)
\;=\; p \cdot 2^{21} + \operatorname{cp}(\ell),
\qquad \operatorname{ROOT} \;=\; 2^{64} - 1 .
```

The bitwise-or coincides with addition here because the two fields never overlap. A Rust `char` is a
Unicode scalar value, so

```math
0 \;\le\; \operatorname{cp}(\ell) \;\le\; \mathtt{0x10FFFF} \;=\; 1{,}114{,}111 \;<\; 2^{21} \;=\; 2{,}097{,}152 .
```

Because $`\operatorname{cp}(\ell)`$ occupies strictly fewer than $`21`$ bits, the
`| codepoint` writes only into the low 21-bit field and never carries into the parent field. The
`CHAR_BITS = 21` shift width and the `debug_assert!(codepoint <= CHAR_MASK)` (with
$`\texttt{CHAR\_MASK} = 2^{21}-1`$) document and, in debug builds, check that invariant.

| Bit range | Width | Field | Value domain |
|-----------|-------|-------|--------------|
| $`63 \dots 53`$ | 11 | unused (always `0` for child keys) | $`0`$ |
| $`52 \dots 21`$ | 32 | `parent_id` $`= p`$ | $`[0,\ 2^{32})`$ — any `u32` |
| $`20 \dots 0`$ | 21 | edge-label codepoint $`\operatorname{cp}(\ell)`$ | $`[0,\ \mathtt{0x10FFFF}] \subset [0,\ 2^{21})`$ |
| all 64 | 64 | `ROOT` sentinel (all ones) | $`2^{64}-1`$ |

A packed child key therefore uses at most $`32 + 21 = 53`$ bits.

## 3. Why the key is exact

**Injectivity.** Because $`0 \le \operatorname{cp}(\ell) < 2^{21}`$, the two fields are recovered
from a packed key by exact integer division and masking:

```math
p \;=\; \left\lfloor \frac{\text{key}}{2^{21}} \right\rfloor \;=\; \text{key} \gg 21,
\qquad
\operatorname{cp}(\ell) \;=\; \text{key} \bmod 2^{21} \;=\; \text{key} \mathbin{\&} (2^{21}-1) .
```

These are a left inverse of $`\operatorname{child}`$, so $`\operatorname{child}`$ is
**injective**: two child keys are equal **iff** both their $`p`$ and their $`\ell`$ are
equal. There is no rounding, truncation, or digest step at which information about $`(p, \ell)`$
could be lost.

**The root sentinel cannot collide with a child key.** The largest representable child key is

```math
\max_{p,\,\ell}\ \operatorname{child}(p,\ell)
\;=\; (2^{32}-1)\cdot 2^{21} + (2^{21}-1)
\;=\; 2^{53} - 1
\;<\; 2^{64} - 1 \;=\; \operatorname{ROOT},
```

so child keys occupy only the low $`53`$ bits while $`\operatorname{ROOT}`$ sets all $`64`$.
They can never coincide (the real maximum is smaller still, since
$`\operatorname{cp}(\ell) \le \mathtt{0x10FFFF} < 2^{21}-1`$). This is exactly what the unit test
`child_keys_are_distinct_from_root` asserts for both $`\operatorname{child}(0, \texttt{'\textbackslash 0'})`$
and $`\operatorname{child}(\mathtt{u32::MAX}, \texttt{char::MAX})`$.

| Property | Status |
|----------|--------|
| Logical node key | exact packed $`(\text{parent id},\ \text{edge label})`$ plus root sentinel |
| Key width | 8 bytes (`u64`) |
| Encoding | injective mixed-radix numeral, radix $`2^{21}`$ |
| Hash-table implementation | `FxHashMap<DictionaryNodeKey, u32>` |
| Consequence of an ordinary hash-table collision | probe-and-`Eq` comparison; **no node aliasing** |
| Memory safety affected? | **No** — the crate remains safe Rust |

The radix-$`2^{21}`$ numeral is the same positional-encoding trick the product-state `StateId`
uses with radix $`M`$ ([architecture/03](../architecture/03-state-encoding-and-product-space.md));
here the "digits" are the parent id (high) and the codepoint (low).

## 4. Logical identity vs hash-table lookup

The word "collision" hides two very different events, and the packed key makes one **impossible** and
the other **harmless**:

- a **logical-identity collision** — two *different* dictionary nodes are assigned the *same* registry
  id. This would be aliasing: a correctness failure that silently merges two nodes.
- a **hash-table collision** — two *different* `DictionaryNodeKey`s hash (via `FxHash`) into the same
  bucket/probe slot of the `FxHashMap`. This is a lookup-cost detail, nothing more.

Interning keys on the **exact** $`(p, \ell)`$ (section 3) makes the first impossible. The second
is resolved by equality, as the following worked example shows.

**Worked non-aliasing example.** Take the three keys from the crate's own
`child_keys_encode_parent_and_label` test, with $`\texttt{'a'} = \mathtt{U{+}0061} = 97`$ and
$`\texttt{'b'} = \mathtt{U{+}0062} = 98`$:

| Call | `parent_id` | $`\operatorname{cp}(\ell)`$ | packed `u64` |
|------|-------------|-----------------------------------|--------------|
| `child(1, 'a')` | $`1`$ | $`97`$ | $`1 \cdot 2^{21} + 97 = 2{,}097{,}249`$ |
| `child(2, 'a')` | $`2`$ | $`97`$ | $`2 \cdot 2^{21} + 97 = 4{,}194{,}401`$ |
| `child(1, 'b')` | $`1`$ | $`98`$ | $`1 \cdot 2^{21} + 98 = 2{,}097{,}250`$ |

All three `u64`s differ, so they are three distinct keys mapping to three distinct ids —
`child(1,'a')` and `child(2,'a')` differ in the parent field, `child(1,'a')` and `child(1,'b')` differ
by $`1`$ in the label field. Now suppose — purely hypothetically — that `FxHash` sent
$`2{,}097{,}249`$ and $`4{,}194{,}401`$ into the same bucket. hashbrown (the SwissTable
backing `FxHashMap`) continues its probe sequence and compares the **full key** with the derived `Eq`
(a `u64` comparison); since $`2{,}097{,}249 \ne 4{,}194{,}401`$, the two entries stay separate,
each mapped to its own id. The collision costs one extra probe-and-compare — it **never** merges the
nodes.

**Contrast with a probabilistic path hash.** The pre-key design digested a traversal path into a
`u64` with some $`h : \Sigma^{\ast} \to [0,\ 2^{64})`$. Distinct paths
$`\pi_A \ne \pi_B`$ to distinct nodes could satisfy $`h(\pi_A) = h(\pi_B)`$ — a genuine
collision **on the logical key itself** — and `get_id` would then return $`A`$'s id for
$`B`$, aliasing two distinct nodes into one product state and corrupting corrections. The packed
key removes this failure mode because it is **not a lossy digest of** $`(p, \ell)`$: it **is**
$`(p, \ell)`$, injectively encoded.

## 5. Path-step keys vs true node identity

There is one honest limitation, and it fails in the **safe** direction. The key identifies a node by
the *path step* $`(p, \ell)`$ that reached it, not by the dictionary's intrinsic node identity.
The dictionary's transition function is deterministic — from a given node, a given label leads to
exactly one child — so $`(p, \ell) \mapsto \text{child node}`$ is a **well-defined function**,
and the same key always denotes the same physical node. But it is **not injective the other way**: in
a minimized DAWG, two different path steps $`(p_1, \ell)`$ and $`(p_2, \ell)`$ can reach
the *same* physical suffix node, yet they carry different keys and so receive **two different ids**.

| Direction | Possible? | Effect |
|-----------|-----------|--------|
| **Over-split** — one physical node reachable under several ids | yes, on shared suffixes | extra states / memory; each id still resolves to the correct node via `id_to_node` — **no correctness impact** |
| **Over-merge** — two distinct nodes under one id | **no** | would be aliasing; ruled out because $`(p, \ell) \mapsto`$ node is a function |

The unsafe direction (over-merge) is structurally impossible; only the harmless direction (over-split)
can occur, and it costs memory, not correctness. Recovering the lost suffix sharing is the sole
motivation for the "true node identities" hardening option below — it is an efficiency improvement,
not a correctness repair.

## 6. Residual considerations

- `FxHashMap` is a **non-cryptographic** hash table. Its internal hash collisions affect lookup
  **cost**, not correctness, because keys are compared for equality after hashing (section 4).
- **Hash-flooding** is a *performance*-DoS, not a correctness hazard, and the adversary does not freely
  choose keys: a key is $`(p, \ell)`$ derived from the dictionary structure and the traversal,
  and the dictionary is trusted, host-built input ([threat-model](threat-model.md)). Were a future
  embedding to expose dictionary construction to an adversary, flooding would degrade registry ops from
  $`\mathcal{O}(1)`$ toward $`\mathcal{O}(\text{bucket length})`$ — still bounded by the same query-length,
  `max_distance`, and cache-policy controls as the rest of the crate.
- The dictionary is treated as **trusted** throughout. Untrusted callers are bounded by query length,
  `max_distance`, and cache policy ([threat-model §4](threat-model.md#4-per-vector-mitigations)) rather
  than by any hash-table assumption.

## 7. Hardening

The implementation already uses exact path-step keys, so the packed `DictionaryNodeKey` has removed
the logical-aliasing failure mode entirely. Further hardening is therefore about **reducing overhead**
or **improving sharing**, not repairing a correctness hazard:

1. **True node identities (preferred).** Have the dictionary backend expose a stable, collision-free
   node id (e.g. a DAWG state index) and key the registry on that. This removes path-step over-splitting
   (section 5) and preserves the suffix sharing a minimized DAWG already encodes.
2. **Concurrent / lock-free registries.** Replace `Arc<RwLock<FxHashMap<…>>>` with a concurrent or
   persistent map if profiling shows write-lock contention during parallel lazy expansion
   ([engineering/concurrency-and-locking](../engineering/concurrency-and-locking.md)).
3. **Keyed / DoS-resistant hasher.** Keep `FxHash` for speed in trusted dictionaries, or switch to a
   keyed hasher (e.g. SipHash) if a future embedding exposes dictionary construction to adversaries.
   This is defense-in-depth against hash-flooding, **not** a correctness fix — correctness already
   rests on `Eq`, not on hash uniqueness.

These are tracked alongside the lock-free registry work in
[engineering/concurrency-and-locking](../engineering/concurrency-and-locking.md).

## See also

- [architecture/05 · Registries and interning](../architecture/05-registries-and-interning.md) — the registry family that owns this key.
- [architecture/03 · State encoding and the product space](../architecture/03-state-encoding-and-product-space.md) — the `StateId` packing that needs a `u32` node id.
- [engineering/concurrency-and-locking](../engineering/concurrency-and-locking.md) — the `Arc<RwLock>` model and lock-free alternatives.
- [security/threat-model](threat-model.md) — the resource bounds that govern untrusted callers.
