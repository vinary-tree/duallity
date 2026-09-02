use std::cmp::Reverse;
use std::collections::BinaryHeap;

use lling_llang::prelude::{
    ExpansionError, ExpansionFailure, ExpansionStatus, StateExpansion, StateId, TropicalWeight,
    WeightedTransition,
};
use lling_llang::wfst::CachePolicy;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::DirectStateSource;

#[derive(Clone)]
pub(crate) struct CachedCharState {
    pub(crate) is_final: bool,
    pub(crate) final_weight: TropicalWeight,
    pub(crate) transitions: SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
}

impl CachedCharState {
    #[inline]
    pub(crate) fn new(
        is_final: bool,
        final_weight: TropicalWeight,
        transitions: SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) -> Self {
        Self {
            is_final,
            final_weight,
            transitions,
        }
    }

    #[inline]
    pub(crate) fn from_state_expansion(
        expansion: StateExpansion<char, TropicalWeight>,
    ) -> Result<Self, ExpansionError> {
        match expansion {
            StateExpansion::Expanded {
                is_final,
                final_weight,
                transitions,
            } => Ok(Self::new(is_final, final_weight, transitions)),
            StateExpansion::Failed(failure) => Err(ExpansionError::Failure(failure)),
            StateExpansion::Cancelled(reason) => Err(ExpansionError::Cancelled(reason)),
        }
    }

    #[inline]
    fn expansion_status(&self) -> ExpansionStatus {
        if self.transitions.is_empty() {
            ExpansionStatus::ExpandedEmpty
        } else {
            ExpansionStatus::ExpandedNonempty
        }
    }
}

impl LazyStateCache<CachedCharState> {
    pub(crate) fn total_cached_transitions(&self) -> usize {
        let entry_transitions = self
            .entries
            .values()
            .map(|entry| entry.cached.transitions.len())
            .sum::<usize>();
        let scratch_transitions = self
            .scratch
            .as_ref()
            .map_or(0, |(_, cached)| cached.transitions.len());

        entry_transitions.saturating_add(scratch_transitions)
    }
}

pub(crate) fn ensure_cached_char_state<S, F>(
    cache: &mut LazyStateCache<CachedCharState>,
    state_source: &S,
    state: StateId,
    is_valid: F,
) -> Result<ExpansionStatus, ExpansionError>
where
    S: DirectStateSource<char, TropicalWeight>,
    F: FnOnce(&S, StateId) -> bool,
{
    if cache.touch_if_cached(state) {
        return cached_char_state_status(cache, state);
    }

    if !is_valid(state_source, state) {
        return Err(invalid_state_error(state));
    }

    cache_char_state_expansion(cache, state, state_source.expand_state(state))
}

#[inline]
pub(crate) fn cache_char_state_expansion(
    cache: &mut LazyStateCache<CachedCharState>,
    state: StateId,
    expansion: StateExpansion<char, TropicalWeight>,
) -> Result<ExpansionStatus, ExpansionError> {
    let cached = CachedCharState::from_state_expansion(expansion)?;
    let status = cached.expansion_status();
    cache.insert(state, cached);
    Ok(status)
}

#[inline]
pub(crate) fn cached_char_state_status(
    cache: &LazyStateCache<CachedCharState>,
    state: StateId,
) -> Result<ExpansionStatus, ExpansionError> {
    cache
        .get(state)
        .map(CachedCharState::expansion_status)
        .ok_or_else(|| invalid_state_error(state))
}

#[inline]
pub(crate) fn invalid_state_error(state: StateId) -> ExpansionError {
    ExpansionError::Failure(ExpansionFailure::invalid_state(state))
}

#[inline]
pub(crate) fn empty_char_transitions() -> &'static [WeightedTransition<char, TropicalWeight>] {
    &[]
}

#[derive(Clone)]
struct CacheEntry<T> {
    cached: T,
    last_access: u64,
}

#[derive(Clone)]
pub(crate) struct LazyStateCache<T> {
    entries: FxHashMap<StateId, CacheEntry<T>>,
    lru_heap: BinaryHeap<Reverse<(u64, StateId)>>,
    scratch: Option<(StateId, T)>,
    clock: u64,
    policy: CachePolicy,
    max_lru_states: usize,
}

impl<T> LazyStateCache<T> {
    pub(crate) fn new(max_lru_states: usize) -> Self {
        Self {
            entries: FxHashMap::default(),
            lru_heap: BinaryHeap::new(),
            scratch: None,
            clock: 0,
            policy: CachePolicy::CacheAll,
            max_lru_states: max_lru_states.max(1),
        }
    }

    #[inline]
    pub(crate) fn get(&self, state: StateId) -> Option<&T> {
        self.entries
            .get(&state)
            .map(|entry| &entry.cached)
            .or_else(|| {
                self.scratch
                    .as_ref()
                    .and_then(|(scratch_state, cached)| (*scratch_state == state).then_some(cached))
            })
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[inline]
    pub(crate) fn policy(&self) -> CachePolicy {
        self.policy
    }

    #[inline]
    pub(crate) fn set_policy(&mut self, policy: CachePolicy) {
        self.policy = policy;
        self.clear();
        self.reserve_policy_capacity();
    }

    #[inline]
    pub(crate) fn set_max_lru_states(&mut self, max_lru_states: usize) {
        self.max_lru_states = max_lru_states.max(1);
        if matches!(self.policy, CachePolicy::Lru { max_states: 0 }) {
            self.reserve_lru_capacity(self.max_lru_states);
            self.evict_lru_until_at_most(self.max_lru_states);
        }
    }

    #[inline]
    pub(crate) fn is_expanded(&self, state: StateId) -> bool {
        !matches!(self.policy, CachePolicy::NoCache) && self.entries.contains_key(&state)
    }

    pub(crate) fn touch_if_cached(&mut self, state: StateId) -> bool {
        if self.entries.contains_key(&state) {
            self.touch(state);
            return true;
        }

        self.scratch
            .as_ref()
            .is_some_and(|(scratch_state, _)| *scratch_state == state)
    }

    pub(crate) fn insert(&mut self, state: StateId, cached: T) {
        match self.policy {
            CachePolicy::NoCache => {
                self.scratch = Some((state, cached));
            }
            CachePolicy::CacheAll => {
                self.entries.insert(
                    state,
                    CacheEntry {
                        cached,
                        last_access: 0,
                    },
                );
                self.scratch = None;
            }
            CachePolicy::Lru { max_states } => {
                let limit = self.lru_limit(max_states);
                if !self.entries.contains_key(&state) {
                    self.evict_lru_before_insert(limit);
                }
                let tick = self.next_tick();
                self.entries.insert(
                    state,
                    CacheEntry {
                        cached,
                        last_access: tick,
                    },
                );
                self.lru_heap.push(Reverse((tick, state)));
                self.compact_lru_heap_if_needed();
                self.scratch = None;
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.lru_heap.clear();
        self.scratch = None;
        self.clock = 0;
    }

    #[inline]
    fn touch(&mut self, state: StateId) {
        if !matches!(self.policy, CachePolicy::Lru { .. }) {
            return;
        }

        let tick = self.next_tick();
        if let Some(entry) = self.entries.get_mut(&state) {
            entry.last_access = tick;
            self.lru_heap.push(Reverse((tick, state)));
            self.compact_lru_heap_if_needed();
        }
    }

    #[inline]
    fn next_tick(&mut self) -> u64 {
        if self.clock == u64::MAX {
            self.renumber_lru_ticks();
        }
        self.clock = self.clock.saturating_add(1).max(1);
        self.clock
    }

    fn renumber_lru_ticks(&mut self) {
        let mut entries_by_age = Vec::with_capacity(self.entries.len());
        entries_by_age.extend(self.entries.iter().filter_map(|(&state, entry)| {
            (entry.last_access > 0).then_some((entry.last_access, state))
        }));
        entries_by_age.sort_unstable();

        self.clock = 0;
        for (_, state) in entries_by_age {
            self.clock = self.clock.saturating_add(1);
            if let Some(entry) = self.entries.get_mut(&state) {
                entry.last_access = self.clock;
            }
        }
        self.rebuild_lru_heap();
    }

    #[inline]
    fn lru_limit(&self, max_states: usize) -> usize {
        if max_states > 0 {
            max_states
        } else {
            self.max_lru_states
        }
        .max(1)
    }

    fn evict_lru_before_insert(&mut self, limit: usize) {
        while self.entries.len() >= limit {
            if !self.evict_one_lru() {
                break;
            }
        }
    }

    fn evict_lru_until_at_most(&mut self, limit: usize) {
        while self.entries.len() > limit {
            if !self.evict_one_lru() {
                break;
            }
        }
    }

    fn evict_one_lru(&mut self) -> bool {
        let victim = match self.pop_lru_victim() {
            Some(victim) => victim,
            None => {
                self.rebuild_lru_heap();
                let Some(victim) = self.pop_lru_victim() else {
                    return false;
                };
                victim
            }
        };
        self.entries.remove(&victim);
        true
    }

    fn pop_lru_victim(&mut self) -> Option<StateId> {
        while let Some(Reverse((tick, state))) = self.lru_heap.pop() {
            if self
                .entries
                .get(&state)
                .is_some_and(|entry| entry.last_access == tick)
            {
                return Some(state);
            }
        }
        None
    }

    fn rebuild_lru_heap(&mut self) {
        self.lru_heap = self
            .entries
            .iter()
            .filter_map(|(&state, entry)| {
                (entry.last_access > 0).then_some(Reverse((entry.last_access, state)))
            })
            .collect();
    }

    fn compact_lru_heap_if_needed(&mut self) {
        let max_heap_len = self.entries.len().saturating_mul(4).max(64);
        if self.lru_heap.len() > max_heap_len {
            self.rebuild_lru_heap();
        }
    }

    fn reserve_policy_capacity(&mut self) {
        if let CachePolicy::Lru { max_states } = self.policy {
            self.reserve_lru_capacity(self.lru_limit(max_states));
        }
    }

    fn reserve_lru_capacity(&mut self, limit: usize) {
        let reserve_target = crate::capped_size_hint_capacity(limit);
        if self.entries.capacity() < reserve_target {
            self.entries
                .reserve(reserve_target.saturating_sub(self.entries.len()));
        }
        if self.lru_heap.capacity() < reserve_target {
            self.lru_heap
                .reserve(reserve_target.saturating_sub(self.lru_heap.len()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CachedCharState, LazyStateCache};
    use lling_llang::prelude::{Semiring, TropicalWeight, WeightedTransition};
    use lling_llang::wfst::CachePolicy;
    use smallvec::SmallVec;

    fn cached_state_with_transition_count(count: usize) -> CachedCharState {
        let transitions = (0..count)
            .filter_map(|index| {
                let state = u32::try_from(index).ok()?;
                Some(WeightedTransition::new(
                    state,
                    Some('a'),
                    Some('a'),
                    state,
                    TropicalWeight::one(),
                ))
            })
            .collect::<SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>>();

        CachedCharState::new(false, TropicalWeight::zero(), transitions)
    }

    #[test]
    fn no_cache_keeps_only_scratch_state() {
        let mut cache = LazyStateCache::new(8);
        cache.set_policy(CachePolicy::NoCache);

        cache.insert(1, "one");
        assert_eq!(cache.len(), 0);
        assert!(!cache.is_expanded(1));
        assert_eq!(cache.get(1), Some(&"one"));

        cache.insert(2, "two");
        assert_eq!(cache.get(1), None);
        assert_eq!(cache.get(2), Some(&"two"));
    }

    #[test]
    fn cached_char_state_transition_count_includes_cache_entries() {
        let mut cache = LazyStateCache::new(8);

        cache.insert(1, cached_state_with_transition_count(2));
        cache.insert(2, cached_state_with_transition_count(3));

        assert_eq!(cache.total_cached_transitions(), 5);
    }

    #[test]
    fn cached_char_state_transition_count_includes_no_cache_scratch() {
        let mut cache = LazyStateCache::new(8);
        cache.set_policy(CachePolicy::NoCache);

        cache.insert(1, cached_state_with_transition_count(2));
        assert_eq!(cache.total_cached_transitions(), 2);

        cache.insert(2, cached_state_with_transition_count(4));
        assert_eq!(cache.total_cached_transitions(), 4);
    }

    #[test]
    fn cache_all_does_not_allocate_lru_metadata() {
        let mut cache = LazyStateCache::new(8);

        cache.insert(1, "one");
        assert!(cache.touch_if_cached(1));

        assert_eq!(cache.get(1), Some(&"one"));
        assert_eq!(cache.clock, 0);
        assert!(cache.entries.values().all(|entry| entry.last_access == 0));
        assert_eq!(cache.lru_heap.len(), 0);
    }

    #[test]
    fn lru_policy_evicts_least_recently_touched_state() {
        let mut cache = LazyStateCache::new(8);
        cache.set_policy(CachePolicy::Lru { max_states: 2 });

        cache.insert(1, "one");
        cache.insert(2, "two");
        assert!(cache.touch_if_cached(1));
        cache.insert(3, "three");

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(1), Some(&"one"));
        assert_eq!(cache.get(2), None);
        assert_eq!(cache.get(3), Some(&"three"));
    }

    #[test]
    fn lru_zero_policy_uses_configured_bound() {
        let mut cache = LazyStateCache::new(2);
        cache.set_policy(CachePolicy::Lru { max_states: 0 });

        cache.insert(1, "one");
        cache.insert(2, "two");
        cache.insert(3, "three");

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(1), None);
        assert_eq!(cache.get(2), Some(&"two"));
        assert_eq!(cache.get(3), Some(&"three"));
    }

    #[test]
    fn lru_policy_preallocates_bounded_cache_storage() {
        let mut cache = LazyStateCache::<&str>::new(2);

        cache.set_policy(CachePolicy::Lru { max_states: 3 });

        assert!(cache.entries.capacity() >= 3);
        assert!(cache.lru_heap.capacity() >= 3);
    }

    #[test]
    fn lru_zero_policy_reserves_configured_bound_updates() {
        let mut cache = LazyStateCache::<&str>::new(1);
        cache.set_policy(CachePolicy::Lru { max_states: 0 });

        cache.set_max_lru_states(4);

        assert!(cache.entries.capacity() >= 4);
        assert!(cache.lru_heap.capacity() >= 4);
    }

    #[test]
    fn lru_policy_treats_large_limits_as_speculative_reservations() {
        let mut explicit = LazyStateCache::<&str>::new(2);
        explicit.set_policy(CachePolicy::Lru {
            max_states: usize::MAX,
        });
        explicit.insert(1, "one");

        assert_eq!(explicit.get(1), Some(&"one"));
        assert!(explicit.entries.capacity() <= crate::MAX_SPECULATIVE_PREALLOCATION * 2);
        assert!(explicit.lru_heap.capacity() <= crate::MAX_SPECULATIVE_PREALLOCATION);

        let mut configured = LazyStateCache::<&str>::new(usize::MAX);
        configured.set_policy(CachePolicy::Lru { max_states: 0 });
        configured.insert(1, "one");

        assert_eq!(configured.get(1), Some(&"one"));
        assert!(configured.entries.capacity() <= crate::MAX_SPECULATIVE_PREALLOCATION * 2);
        assert!(configured.lru_heap.capacity() <= crate::MAX_SPECULATIVE_PREALLOCATION);
    }

    #[test]
    fn lru_zero_policy_shrinks_to_updated_configured_bound() {
        let mut cache = LazyStateCache::new(4);
        cache.set_policy(CachePolicy::Lru { max_states: 0 });

        cache.insert(1, "one");
        cache.insert(2, "two");
        cache.insert(3, "three");
        assert!(cache.touch_if_cached(3));

        cache.set_max_lru_states(1);

        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(1), None);
        assert_eq!(cache.get(2), None);
        assert_eq!(cache.get(3), Some(&"three"));
    }

    #[test]
    fn lru_reinsert_updates_without_evicting_another_state() {
        let mut cache = LazyStateCache::new(2);
        cache.set_policy(CachePolicy::Lru { max_states: 2 });

        cache.insert(1, "old");
        cache.insert(2, "two");
        cache.insert(1, "new");

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(1), Some(&"new"));
        assert_eq!(cache.get(2), Some(&"two"));

        cache.insert(3, "three");

        assert_eq!(cache.get(1), Some(&"new"));
        assert_eq!(cache.get(2), None);
        assert_eq!(cache.get(3), Some(&"three"));
    }

    #[test]
    fn clear_resets_cached_state_and_lru_clock_generation() {
        let mut cache = LazyStateCache::new(2);
        cache.set_policy(CachePolicy::Lru { max_states: 2 });

        cache.insert(1, "one");
        assert!(cache.clock > 0);
        assert!(!cache.lru_heap.is_empty());

        cache.clear();

        assert_eq!(cache.len(), 0);
        assert_eq!(cache.clock, 0);
        assert!(cache.lru_heap.is_empty());
        assert_eq!(cache.get(1), None);
    }

    #[test]
    fn repeated_lru_touches_do_not_grow_heap_without_bound() {
        let mut cache = LazyStateCache::new(2);
        cache.set_policy(CachePolicy::Lru { max_states: 2 });
        cache.insert(1, "one");
        cache.insert(2, "two");

        for _ in 0..200 {
            assert!(cache.touch_if_cached(1));
        }

        assert!(cache.lru_heap.len() <= cache.len().saturating_mul(4).max(64));
        cache.insert(3, "three");

        assert_eq!(cache.get(1), Some(&"one"));
        assert_eq!(cache.get(2), None);
        assert_eq!(cache.get(3), Some(&"three"));
    }

    #[test]
    fn lru_clock_rollover_preserves_recency_order() {
        let mut cache = LazyStateCache::new(2);
        cache.set_policy(CachePolicy::Lru { max_states: 2 });
        cache.insert(1, "one");
        cache.insert(2, "two");

        cache.clock = u64::MAX;
        cache
            .entries
            .get_mut(&1)
            .expect("state 1 cached")
            .last_access = u64::MAX - 1;
        cache
            .entries
            .get_mut(&2)
            .expect("state 2 cached")
            .last_access = u64::MAX;
        cache.rebuild_lru_heap();

        assert!(cache.touch_if_cached(1));
        cache.insert(3, "three");

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(1), Some(&"one"));
        assert_eq!(cache.get(2), None);
        assert_eq!(cache.get(3), Some(&"three"));
        assert!(cache.clock < u64::MAX);
    }
}
