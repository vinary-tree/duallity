use std::sync::Arc;

use liblevenshtein::phonetic::nfa::ProductStateChar;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::fx_hash_map_with_capacity;

pub(crate) type ProductStateKeyBytes = SmallVec<[u8; 96]>;
#[cfg(test)]
type CanonicalNfaStateBuffer = SmallVec<[u32; 8]>;
const PRODUCT_COST_KEY_SCALE: f64 = 1_000_000.0;

#[inline]
pub(crate) fn next_phonetic_registry_id(len: usize) -> Option<u32> {
    u32::try_from(len).ok()
}

#[inline]
fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[inline]
pub(crate) fn estimated_phonetic_product_states(max_distance: u8) -> u32 {
    u32::from(max_distance)
        .saturating_add(1)
        .saturating_mul(1000)
        .max(10_000)
}

/// Registry for assigning stable IDs to product states.
pub(crate) struct ProductStateRegistry {
    /// Map from product frontier to assigned ID.
    state_to_id: FxHashMap<ProductStateKey, u32>,
    /// Map from ID back to frontier.
    id_to_state: Vec<Arc<[ProductStateChar]>>,
}

/// Key for hashing ProductStateChar.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProductStateKey(pub(crate) ProductStateKeyBytes);

impl ProductStateRegistry {
    pub(crate) fn new(initial_frontier: Vec<ProductStateChar>) -> Self {
        let mut registry = Self {
            state_to_id: fx_hash_map_with_capacity(1),
            id_to_state: Vec::with_capacity(1),
        };
        let _ = registry.register_state(initial_frontier);
        registry
    }

    pub(crate) fn register_state(&mut self, frontier: Vec<ProductStateChar>) -> Option<u32> {
        let (frontier, key) = Self::prepare_state(frontier)?;
        self.register_prepared_state_with_key(frontier, key)
    }

    #[cfg(test)]
    pub(crate) fn register_state_with_key(
        &mut self,
        frontier: Vec<ProductStateChar>,
        key: ProductStateKey,
    ) -> Option<u32> {
        let (frontier, canonical_key) = Self::prepare_state(frontier)?;
        debug_assert_eq!(canonical_key, key);
        self.register_prepared_state_with_key(frontier, canonical_key)
    }

    pub(crate) fn register_prepared_state_with_key(
        &mut self,
        frontier: Vec<ProductStateChar>,
        key: ProductStateKey,
    ) -> Option<u32> {
        if let Some(&id) = self.state_to_id.get(&key) {
            return Some(id);
        }

        let id = next_phonetic_registry_id(self.id_to_state.len())?;
        self.state_to_id.insert(key, id);
        self.id_to_state.push(Arc::from(frontier));
        Some(id)
    }

    pub(crate) fn candidate_state_id_for_key(&self, key: &ProductStateKey) -> Option<u32> {
        self.state_to_id
            .get(key)
            .copied()
            .or_else(|| next_phonetic_registry_id(self.id_to_state.len()))
    }

    pub(crate) fn get_state(&self, id: u32) -> Option<Arc<[ProductStateChar]>> {
        self.id_to_state.get(usize_from_u32(id)).cloned()
    }

    pub(crate) fn prepare_state(
        mut frontier: Vec<ProductStateChar>,
    ) -> Option<(Vec<ProductStateChar>, ProductStateKey)> {
        canonicalize_product_frontier(&mut frontier);
        let key = canonical_product_frontier_to_key(&frontier)?;
        Some((frontier, key))
    }

    #[cfg(test)]
    pub(crate) fn state_to_key(frontier: &[ProductStateChar]) -> Option<ProductStateKey> {
        let mut states = frontier
            .iter()
            .map(ProductStateKeyEntry::from_product_state)
            .collect::<SmallVec<[ProductStateKeyEntry<'_>; 8]>>();

        if states.len() > 1 {
            states.sort_by(|a, b| {
                a.cost_key
                    .cmp(&b.cost_key)
                    .then_with(|| a.nfa_states().cmp(b.nfa_states()))
            });
            states.dedup_by(|a, b| a.cost_key == b.cost_key && a.nfa_states() == b.nfa_states());
        }

        let byte_capacity = product_state_key_byte_capacity(&states);
        let mut bytes =
            ProductStateKeyBytes::with_capacity(crate::capped_size_hint_capacity(byte_capacity));

        for state in &states {
            append_product_state_key_bytes(&mut bytes, state)?;
        }

        Some(ProductStateKey(bytes))
    }

    pub(crate) fn len(&self) -> usize {
        self.id_to_state.len()
    }
}

#[cfg(test)]
struct ProductStateKeyEntry<'a> {
    nfa_states: CanonicalNfaStates<'a>,
    cost_key: i64,
}

#[cfg(test)]
impl<'a> ProductStateKeyEntry<'a> {
    fn from_product_state(state: &'a ProductStateChar) -> Self {
        Self {
            nfa_states: CanonicalNfaStates::from_slice(&state.nfa_states),
            cost_key: product_cost_key(state.accumulated_cost),
        }
    }

    fn nfa_states(&self) -> &[u32] {
        self.nfa_states.as_slice()
    }
}

#[cfg(test)]
enum CanonicalNfaStates<'a> {
    Borrowed(&'a [u32]),
    Owned(CanonicalNfaStateBuffer),
}

#[cfg(test)]
impl<'a> CanonicalNfaStates<'a> {
    fn from_slice(states: &'a [u32]) -> Self {
        if states.windows(2).all(|pair| pair[0] < pair[1]) {
            return Self::Borrowed(states);
        }

        let mut sorted = CanonicalNfaStateBuffer::from_slice(states);
        sorted.sort_unstable();
        sorted.dedup();
        Self::Owned(sorted)
    }

    fn as_slice(&self) -> &[u32] {
        match self {
            Self::Borrowed(states) => states,
            Self::Owned(states) => states.as_slice(),
        }
    }
}

#[cfg(test)]
fn append_product_state_key_bytes(
    bytes: &mut ProductStateKeyBytes,
    state: &ProductStateKeyEntry<'_>,
) -> Option<()> {
    append_product_state_key_parts(bytes, state.nfa_states(), state.cost_key)
}

fn append_product_state_key_parts(
    bytes: &mut ProductStateKeyBytes,
    nfa_states: &[u32],
    cost_key: i64,
) -> Option<()> {
    let nfa_state_count = next_phonetic_registry_id(nfa_states.len())?;
    bytes.extend_from_slice(&nfa_state_count.to_le_bytes());
    for &s in nfa_states {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    bytes.extend_from_slice(&cost_key.to_le_bytes());
    Some(())
}

#[cfg(test)]
fn product_state_key_byte_capacity(states: &[ProductStateKeyEntry<'_>]) -> usize {
    states.iter().fold(0usize, |capacity, state| {
        capacity
            .saturating_add(std::mem::size_of::<u32>())
            .saturating_add(
                state
                    .nfa_states()
                    .len()
                    .saturating_mul(std::mem::size_of::<u32>()),
            )
            .saturating_add(std::mem::size_of::<i64>())
    })
}

fn canonical_product_frontier_to_key(frontier: &[ProductStateChar]) -> Option<ProductStateKey> {
    let byte_capacity = frontier.iter().fold(0usize, |capacity, state| {
        capacity
            .saturating_add(std::mem::size_of::<u32>())
            .saturating_add(
                state
                    .nfa_states
                    .len()
                    .saturating_mul(std::mem::size_of::<u32>()),
            )
            .saturating_add(std::mem::size_of::<i64>())
    });
    let mut bytes =
        ProductStateKeyBytes::with_capacity(crate::capped_size_hint_capacity(byte_capacity));

    for state in frontier {
        append_product_state_key_parts(
            &mut bytes,
            &state.nfa_states,
            product_cost_key(state.accumulated_cost),
        )?;
    }

    Some(ProductStateKey(bytes))
}

fn canonicalize_product_frontier(frontier: &mut Vec<ProductStateChar>) {
    for state in frontier.iter_mut() {
        canonicalize_nfa_states(&mut state.nfa_states);
    }

    if frontier.len() > 1 {
        frontier.sort_by(|a, b| {
            product_cost_key(a.accumulated_cost)
                .cmp(&product_cost_key(b.accumulated_cost))
                .then_with(|| a.nfa_states.cmp(&b.nfa_states))
                .then_with(|| a.accumulated_cost.total_cmp(&b.accumulated_cost))
        });
        frontier.dedup_by(|a, b| product_states_share_key(a, b));
    }
}

fn canonicalize_nfa_states(states: &mut Vec<u32>) {
    if states.windows(2).all(|pair| pair[0] < pair[1]) {
        return;
    }

    states.sort_unstable();
    states.dedup();
}

fn product_states_share_key(left: &ProductStateChar, right: &ProductStateChar) -> bool {
    product_cost_key(left.accumulated_cost) == product_cost_key(right.accumulated_cost)
        && left.nfa_states == right.nfa_states
}

#[inline]
fn product_cost_key(cost: f64) -> i64 {
    if cost.is_nan() {
        return i64::MIN;
    }
    if cost.is_infinite() {
        return if cost.is_sign_positive() {
            i64::MAX
        } else {
            i64::MIN + 1
        };
    }

    let scaled = (cost * PRODUCT_COST_KEY_SCALE).round();
    if scaled >= i64::MAX as f64 {
        i64::MAX
    } else if scaled <= i64::MIN as f64 {
        i64::MIN
    } else {
        scaled as i64
    }
}
