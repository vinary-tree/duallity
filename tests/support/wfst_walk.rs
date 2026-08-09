//! Consumer-side walker for a live `vt.scalar-wfst.1` resource.
//!
//! Every duallity WFST crosses the ABI as this interface, so the family/ABI
//! suites drive the *real* exported surface (`query_interface` →
//! `state_info`/`state_arcs`) rather than the in-crate Rust types. The walker
//! pages arcs at [`VT_RECOMMENDED_ARC_BATCH`] and enforces the paging law as it
//! goes, then exposes the transduced language for oracle comparison.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;

use vinary_tree_interop::{
    VtResource, VtStatus, VtWeightDomain, VtWfstArc, VtWfstVTable, VT_RECOMMENDED_ARC_BATCH,
    VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
};

/// A borrowed view over a `vt.scalar-wfst.1` resource. Does not own a retain;
/// the caller keeps the resource alive and releases it.
pub struct WfstView {
    context: *mut c_void,
    table: *const VtWfstVTable,
}

/// One fully paged state: validity, finality, final weight, and every arc.
pub struct StateExpansion {
    pub valid: bool,
    pub is_final: bool,
    pub final_weight: f64,
    pub arcs: Vec<VtWfstArc>,
}

impl WfstView {
    /// Negotiate the WFST interface on a live resource.
    ///
    /// # Panics
    /// If the resource does not publish `vt.scalar-wfst.1`.
    pub fn new(resource: VtResource) -> Self {
        assert!(!resource.is_null(), "resource must be non-null");
        let mut interface: *const c_void = std::ptr::null();
        let status = unsafe {
            (*resource.vtable).query_interface.expect("query_interface")(
                resource.context,
                &VT_WFST_INTERFACE_ID,
                VT_WFST_INTERFACE_VERSION,
                &mut interface,
            )
        };
        assert_eq!(status, VtStatus::Ok.to_raw(), "WFST interface negotiation");
        assert!(!interface.is_null(), "WFST vtable must be non-null");
        Self {
            context: resource.context,
            table: interface.cast::<VtWfstVTable>(),
        }
    }

    fn table(&self) -> &VtWfstVTable {
        unsafe { &*self.table }
    }

    pub fn weight_domain(&self) -> VtWeightDomain {
        self.table().weight_domain
    }

    pub fn start(&self) -> u64 {
        let mut start = 0u64;
        let status = unsafe { self.table().start.expect("start")(self.context, &mut start) };
        assert_eq!(status, VtStatus::Ok.to_raw(), "start");
        start
    }

    /// Raw status of a single `state_arcs` page for `state`. Used to observe a
    /// latched provider fault surfacing during traversal.
    pub fn first_page_status(&self, state: u64) -> u32 {
        let mut arcs = vec![VtWfstArc::default(); VT_RECOMMENDED_ARC_BATCH];
        let mut written = 0usize;
        let mut total = 0usize;
        unsafe {
            self.table().state_arcs.expect("state_arcs")(
                self.context,
                state,
                0,
                arcs.as_mut_ptr(),
                arcs.len(),
                &mut written,
                &mut total,
            )
        }
    }

    /// Fully expand one state, paging arcs and enforcing the paging law.
    /// Returns `Err(status)` if any callback reports a non-Ok status.
    pub fn expand(&self, state: u64) -> Result<StateExpansion, u32> {
        let mut valid = 0u8;
        let mut is_final = 0u8;
        let mut final_weight = 0.0f64;
        let info = unsafe {
            self.table().state_info.expect("state_info")(
                self.context,
                state,
                &mut valid,
                &mut is_final,
                &mut final_weight,
            )
        };
        if info != VtStatus::Ok.to_raw() {
            return Err(info);
        }

        let mut arcs = Vec::new();
        let mut offset = 0usize;
        loop {
            let mut page = vec![VtWfstArc::default(); VT_RECOMMENDED_ARC_BATCH];
            let mut written = 0usize;
            let mut total = 0usize;
            let status = unsafe {
                self.table().state_arcs.expect("state_arcs")(
                    self.context,
                    state,
                    offset,
                    page.as_mut_ptr(),
                    page.len(),
                    &mut written,
                    &mut total,
                )
            };
            if status != VtStatus::Ok.to_raw() {
                return Err(status);
            }
            assert!(written <= page.len(), "written exceeds capacity");
            assert!(
                offset.saturating_add(written) <= total,
                "page runs past total"
            );
            arcs.extend(page.into_iter().take(written));
            offset += written;
            if offset >= total || written == 0 {
                break;
            }
        }

        Ok(StateExpansion {
            valid: valid == 1,
            is_final: is_final == 1,
            final_weight,
            arcs,
        })
    }

    /// Breadth-first walk of every reachable state. Returns the number of
    /// distinct states visited. Panics on any expansion error.
    pub fn visit_all(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        let mut frontier = vec![self.start()];
        while let Some(state) = frontier.pop() {
            if !seen.insert(state) {
                continue;
            }
            let expansion = self.expand(state).expect("state expands without fault");
            frontier.extend(expansion.arcs.iter().map(|arc| arc.target_state));
        }
        seen.len()
    }

    /// The transduced language as `output term -> best accumulated weight`.
    ///
    /// `minimize` selects the semiring aggregation: `true` keeps the minimum
    /// (tropical / min-plus, so the weight is the edit distance), `false` keeps
    /// the maximum (arctic / max-plus, so the weight is the fzf score). The
    /// output string of an accepting path spells the dictionary term.
    pub fn language(&self, minimize: bool) -> BTreeMap<String, f64> {
        let mut best_at: HashMap<(u64, String), f64> = HashMap::new();
        let mut accepted: BTreeMap<String, f64> = BTreeMap::new();
        let mut frontier = vec![(self.start(), String::new(), 0.0f64)];
        let mut guard = 0usize;
        while let Some((state, output, weight)) = frontier.pop() {
            guard += 1;
            assert!(guard < 4_000_000, "traversal did not converge (cycle?)");
            match best_at.get(&(state, output.clone())) {
                Some(&seen) if better_or_equal(seen, weight, minimize) => continue,
                _ => {
                    best_at.insert((state, output.clone()), weight);
                }
            }
            let expansion = self.expand(state).expect("state expands without fault");
            if !expansion.valid {
                continue;
            }
            if expansion.is_final {
                let total = weight + expansion.final_weight;
                accepted
                    .entry(output.clone())
                    .and_modify(|existing| {
                        if better(total, *existing, minimize) {
                            *existing = total;
                        }
                    })
                    .or_insert(total);
            }
            for arc in &expansion.arcs {
                let mut next_output = output.clone();
                if arc.has_output == 1 {
                    let scalar = u32::try_from(arc.output_label)
                        .ok()
                        .and_then(char::from_u32)
                        .expect("output label is a Unicode scalar");
                    next_output.push(scalar);
                }
                frontier.push((arc.target_state, next_output, weight + arc.weight));
            }
        }
        accepted
    }
}

fn better(candidate: f64, incumbent: f64, minimize: bool) -> bool {
    if minimize {
        candidate < incumbent
    } else {
        candidate > incumbent
    }
}

fn better_or_equal(incumbent: f64, candidate: f64, minimize: bool) -> bool {
    if minimize {
        incumbent <= candidate
    } else {
        incumbent >= candidate
    }
}
