//! Bounded staging and atomic publication of generalized product expansions.

use super::*;

#[cfg(test)]
#[path = "generalized_lock_tests.rs"]
mod lock_tests;

impl<D> GeneralizedWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: DictionaryNode<Unit = char>,
{
    pub(super) fn compute_registered_state(
        &self,
        state: StateId,
    ) -> StateExpansion<char, TropicalWeight> {
        self.compute_registered_state_with_check(state, None, &|| Ok(()))
    }

    pub(crate) fn compute_registered_state_with_check(
        &self,
        state: StateId,
        cancellation: Option<&CancellationToken>,
        check_source: &dyn Fn() -> Result<(), ExpansionError>,
    ) -> StateExpansion<char, TropicalWeight> {
        let result = (|| {
            #[cfg(feature = "bindings-core")]
            let scope = crate::bindings::DictionaryComputationScope::enter()?;
            let check = || {
                #[cfg(feature = "bindings-core")]
                scope.check()?;
                check_source()
            };
            self.try_compute_registered_state(state, cancellation, &check)
        })();
        match result {
            Ok(value) => StateExpansion::Expanded {
                is_final: value.is_final,
                final_weight: value.final_weight,
                transitions: value.transitions,
            },
            Err(ExpansionError::Failure(failure)) => StateExpansion::failed(failure),
            Err(ExpansionError::Cancelled(reason)) => StateExpansion::cancelled(reason),
            Err(error) => StateExpansion::failed(ExpansionFailure::new(
                lling_llang::prelude::ExpansionFailureKind::Source,
                lling_llang::prelude::RetryPolicy::Never,
                error.to_string(),
            )),
        }
    }

    fn try_compute_registered_state(
        &self,
        state: StateId,
        cancellation: Option<&CancellationToken>,
        check_source: &dyn Fn() -> Result<(), ExpansionError>,
    ) -> Result<CachedCharState, ExpansionError> {
        let mut budget = ExpansionBudget::new(self.limits, cancellation);
        budget.check_cancelled()?;
        check_source()?;
        match self
            .registered_state(state)
            .ok_or_else(|| invalid_state_error(state))?
        {
            RegisteredState::Product(product) => {
                self.compute_product_state(state, product, &mut budget, check_source)
            }
            RegisteredState::Emit(emit) => {
                // The full chain was registered atomically with its first arc.
                budget.charge_work(1)?;
                let transitions = smallvec::smallvec![WeightedTransition::new(
                    state,
                    emit.chain.input.get(emit.position).copied(),
                    emit.chain.output.get(emit.position).copied(),
                    emit.next,
                    TropicalWeight::new(0.0),
                )];
                budget.check_cancelled()?;
                check_source()?;
                Ok(CachedCharState::new(
                    false,
                    TropicalWeight::zero(),
                    transitions,
                ))
            }
        }
    }

    pub(super) fn registered_state(&self, state: StateId) -> Option<RegisteredState> {
        crate::read_lock(&self.state_registry).get(state).cloned()
    }

    pub(super) fn final_weight_for_state(&self, state: StateId) -> Option<TropicalWeight> {
        match self.registered_state(state)? {
            RegisteredState::Product(product) => {
                let node = self.dictionary_node(product.dict_node_id)?;
                self.product_final_weight(product, &node)
            }
            RegisteredState::Emit(_) => None,
        }
    }

    fn compute_product_state(
        &self,
        state: StateId,
        product: ProductState,
        budget: &mut ExpansionBudget<'_>,
        check_source: &dyn Fn() -> Result<(), ExpansionError>,
    ) -> Result<CachedCharState, ExpansionError> {
        let node = self
            .dictionary_node(product.dict_node_id)
            .ok_or_else(|| invalid_state_error(state))?;
        if !self.query.is_char_boundary(product.query_byte_pos)
            || !self.cost_within_bound(product.cost)
        {
            return Err(invalid_state_error(state));
        }
        budget.charge_work(1)?;
        let final_weight = self.product_final_weight(product, &node);
        check_source()?;
        let mut staging = ExpansionStaging::new();
        let mut arcs = Vec::new();
        let mut paths_by_width: DictPathCache = width_cache(self.source_width_slot_count, budget)?;
        let mut queries: QuerySegmentCache<'_> = width_cache(self.query_width_slot_count, budget)?;

        for prepared in &self.prepared_operations {
            budget.charge_work(1)?;
            // Avoid overflow even when the configured budget nearly fills usize.
            if prepared.scaled_weight > self.max_cost - product.cost {
                continue;
            }
            let cost = product.cost + prepared.scaled_weight;
            let query_entry = &mut queries[prepared.query_width_slot];
            if matches!(query_entry, WidthCacheEntry::Uncomputed) {
                // Both UTF-8 boundary scanning and scalar materialization.
                budget.charge_work(prepared.consume_y * 2)?;
                *query_entry = match self.query_segment(product.query_byte_pos, prepared.consume_y)
                {
                    Some((segment, end)) => WidthCacheEntry::Ready(QuerySegment::new(segment, end)),
                    None => WidthCacheEntry::Missing,
                };
            }
            let WidthCacheEntry::Ready(query) = query_entry else {
                continue;
            };
            let path_entry = &mut paths_by_width[prepared.source_width_slot];
            if matches!(path_entry, WidthCacheEntry::Uncomputed) {
                *path_entry = WidthCacheEntry::Ready(self.dictionary_paths_exact_chars(
                    product.dict_node_id,
                    prepared.consume_x,
                    &mut staging,
                    budget,
                    check_source,
                )?);
            }
            let WidthCacheEntry::Ready(paths) = path_entry else {
                unreachable!("dictionary cache stores complete path sets");
            };
            let operation = &self.operations.operations()[prepared.index];
            for path in paths.iter() {
                budget.charge_work(1 + path.bytes.len() + query.bytes.len())?;
                if !operation_applies(
                    prepared,
                    operation,
                    &path.output,
                    &path.bytes,
                    &query.chars,
                    query.bytes,
                ) {
                    continue;
                }
                budget.charge_work(1 + path.output.len() + query.chars.len())?;
                arcs.push(PendingOperationArc {
                    target_node: path.target_node,
                    query_byte_pos: query.end_byte_pos,
                    cost,
                    input: query.chars.clone(),
                    output: path.output.clone(),
                    weight: prepared.weight,
                });
            }
        }
        check_source()?;
        let transitions = self.commit_expansion(state, staging, arcs, budget, check_source)?;
        Ok(CachedCharState::new(
            final_weight.is_some(),
            final_weight.unwrap_or_else(TropicalWeight::zero),
            transitions,
        ))
    }

    /// Fixed lock order: nodes then states. No provider callbacks under either
    /// lock; the final checker only observes the invocation's captured fault.
    fn commit_expansion(
        &self,
        state: StateId,
        mut staging: ExpansionStaging<D::Node>,
        arcs: Vec<PendingOperationArc>,
        budget: &mut ExpansionBudget<'_>,
        check_source: &dyn Fn() -> Result<(), ExpansionError>,
    ) -> Result<SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>, ExpansionError> {
        budget.check_cancelled()?;
        // Declared before guards: failed preflight must release every guard
        // before any retired owner can run user-defined destruction.
        let mut retired = Vec::new();
        retired.try_reserve(staging.nodes.len()).map_err(|_| {
            expansion_failure(GeneralizedWfstError::AllocationFailed(
                "reserving retired node owners",
            ))
        })?;
        let (mut nodes, resolved, additions) = loop {
            let nodes = crate::write_lock(&self.node_registry);
            let mut resolved = Vec::with_capacity(staging.nodes.len());
            let mut additions: Vec<(DictionaryNodeKey, Arc<D::Node>)> =
                Vec::with_capacity(staging.nodes.len());
            let mut new_ids = FxHashMap::default();
            for staged in &mut staging.nodes {
                budget.charge_work(1)?;
                let key =
                    DictionaryNodeKey::child(resolved_node(staged.parent, &resolved), staged.label);
                let id = match nodes.get_id(key).or_else(|| new_ids.get(&key).copied()) {
                    Some(id) => {
                        let canonical = nodes.get_node(id).unwrap_or_else(|| {
                            &additions
                                [usize::try_from(id).expect("node ID fits usize") - nodes.len()]
                            .1
                        });
                        if !Arc::ptr_eq(&staged.node, canonical) {
                            retired
                                .push(std::mem::replace(&mut staged.node, Arc::clone(canonical)));
                        }
                        id
                    }
                    None => {
                        let count = nodes.len().checked_add(additions.len()).ok_or_else(|| {
                            expansion_failure(GeneralizedWfstError::ArithmeticOverflow(
                                "counting dictionary nodes",
                            ))
                        })?;
                        let required = count.checked_add(1).ok_or_else(|| {
                            expansion_failure(GeneralizedWfstError::ArithmeticOverflow(
                                "counting dictionary nodes",
                            ))
                        })?;
                        require_at_most(
                            GeneralizedWfstResource::RetainedDictionaryNodes,
                            required,
                            self.limits.max_retained_dictionary_nodes,
                        )
                        .map_err(expansion_failure)?;
                        let id = next_registry_id(count).ok_or_else(|| {
                            expansion_failure(GeneralizedWfstError::ArithmeticOverflow(
                                "encoding a dictionary node ID",
                            ))
                        })?;
                        new_ids.insert(key, id);
                        additions.push((key, Arc::clone(&staged.node)));
                        id
                    }
                };
                resolved.push(id);
            }
            if retired.is_empty() {
                break (nodes, resolved, additions);
            }
            // Destructor callbacks may reenter or fail. Retire before any
            // publication, then recompute provisional IDs under a fresh guard.
            drop(nodes);
            drop(additions);
            retired.clear();
            budget.check_cancelled()?;
            check_source()?;
        };
        // Every staged owner now has the same Arc as either an existing node
        // or this batch's additions. Postcommit staging cleanup cannot run N::Drop.
        let mut states = crate::write_lock(&self.state_registry);
        let mut batch = StateBatch::new();
        let mut transitions = SmallVec::with_capacity(arcs.len());
        for arc in arcs {
            // Conservative scalar work covers copies, chain hashing/comparison,
            // continuation records, and the later map insertion.
            budget.charge_work(1 + 8 * (arc.input.len() + arc.output.len()))?;
            let target = batch.product(
                &states,
                ProductState {
                    dict_node_id: resolved_node(arc.target_node, &resolved),
                    query_byte_pos: arc.query_byte_pos,
                    cost: arc.cost,
                },
                self.limits.max_retained_wfst_states,
            )?;
            let input = arc.input.first().copied();
            let output = arc.output.first().copied();
            let to = if arc.input.len().max(arc.output.len()) <= 1 {
                target
            } else {
                batch.chain(
                    &states,
                    EmissionChain {
                        input: Arc::from(arc.input.as_slice()),
                        output: Arc::from(arc.output.as_slice()),
                        target,
                    },
                    self.limits.max_retained_wfst_states,
                )?
            };
            transitions.push(WeightedTransition::new(
                state,
                input,
                output,
                to,
                TropicalWeight::new(arc.weight),
            ));
        }
        nodes.try_reserve_additional(additions.len()).map_err(|_| {
            expansion_failure(GeneralizedWfstError::AllocationFailed(
                "reserving dictionary identities",
            ))
        })?;
        batch.prepare_registry(&mut states)?;
        budget.check_cancelled()?;
        check_source()?;
        // Linearization point. All later operations move prepared data or clone
        // Arcs into reserved storage; every referenced ID becomes visible together.
        nodes.commit_prepared(additions);
        states.commit_prepared(batch.states);
        Ok(transitions)
    }

    fn dictionary_paths_exact_chars(
        &self,
        start: u32,
        width: usize,
        staging: &mut ExpansionStaging<D::Node>,
        budget: &mut ExpansionBudget<'_>,
        check_source: &dyn Fn() -> Result<(), ExpansionError>,
    ) -> Result<DictPaths, ExpansionError> {
        let node = self
            .dictionary_node(start)
            .ok_or_else(|| invalid_state_error(start))?;
        let start = PendingDictionaryNode::Registered(start);
        let mut paths = DictPaths::new();
        if width == 0 {
            budget.charge_path()?;
            budget.charge_work(1)?;
            paths.push(DictPath {
                target_node: start,
                output: LabelBuffer::new(),
                bytes: ByteBuffer::new(),
            });
            return Ok(paths);
        }
        let edges = self.bounded_edges(&node, budget, check_source)?;
        let mut stack = vec![(start, edges.into_iter(), 0usize, 0usize)];
        let mut output = LabelBuffer::new();
        let mut bytes = ByteBuffer::new();
        while let Some((parent, edges, output_len, byte_len)) = stack.last_mut() {
            budget.charge_work(1)?;
            output.truncate(*output_len);
            bytes.truncate(*byte_len);
            let Some((label, node)) = edges.next() else {
                stack.pop();
                continue;
            };
            let registered = match *parent {
                PendingDictionaryNode::Registered(id) => {
                    let registry = crate::read_lock(&self.node_registry);
                    registry.get_id(DictionaryNodeKey::child(id, label))
                }
                PendingDictionaryNode::Staged(_) => None,
            };
            // User-defined Clone/Drop must never execute under registry locks.
            let child = staging.stage_child(registered, *parent, label, &node);
            output.push(label);
            let mut encoded = [0; 4];
            bytes.extend_from_slice(label.encode_utf8(&mut encoded).as_bytes());
            if output.len() == width {
                budget.charge_path()?;
                budget.charge_work(1 + output.len() + bytes.len())?;
                paths.push(DictPath {
                    target_node: child,
                    output: output.clone(),
                    bytes: bytes.clone(),
                });
            } else {
                let edges = self.bounded_edges(&node, budget, check_source)?;
                stack.push((child, edges.into_iter(), output.len(), bytes.len()));
            }
        }
        Ok(paths)
    }

    /// Bound borrowed adjacency before moving it into an owned DFS frame.
    /// Never reserve from a provider's edge-count or iterator size hint.
    fn bounded_edges(
        &self,
        node: &D::Node,
        budget: &mut ExpansionBudget<'_>,
        check_source: &dyn Fn() -> Result<(), ExpansionError>,
    ) -> Result<Vec<(char, D::Node)>, ExpansionError> {
        budget.charge_work(1)?;
        let mut edges = node.edges();
        let mut owned = Vec::new();
        loop {
            budget.charge_work(1)?;
            match edges.next() {
                Some(edge) => owned.push(edge),
                None => break,
            }
            check_source()?;
        }
        check_source()?;
        Ok(owned)
    }
}

fn width_cache<T>(
    count: usize,
    budget: &mut ExpansionBudget<'_>,
) -> Result<SmallVec<[WidthCacheEntry<T>; 6]>, ExpansionError> {
    budget.charge_work(count)?;
    let mut cache = SmallVec::new();
    cache.try_reserve(count).map_err(|_| {
        expansion_failure(GeneralizedWfstError::AllocationFailed(
            "reserving width-cache slots",
        ))
    })?;
    cache.resize_with(count, WidthCacheEntry::default);
    Ok(cache)
}
