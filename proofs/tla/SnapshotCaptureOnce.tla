---------------------- MODULE SnapshotCaptureOnce ----------------------
(***************************************************************************)
(* duallity_wfst_new (src/ffi.rs -> src/bindings.rs create_wfst) captures  *)
(* the source dictionary's CURRENT revision exactly once, at construction,  *)
(* and every later traversal reads that captured revision -- never the live *)
(* dictionary. The captured snapshot holds its own retain, so it survives   *)
(* mutation and even the drop of the source handle (the query-start         *)
(* snapshot semantics pinned by the Rust test                              *)
(* c_constructor_retains_query_start_dictionary_revision).                  *)
(*                                                                          *)
(* This is a tiny consumer-side model; the underlying retain/release        *)
(* lifetime is the interop resource-lifecycle model's concern               *)
(* (liblevenshtein-rust docs/verification/tla/AbiResourceLifecycle.tla, #1),*)
(* referenced here rather than re-modeled.                                  *)
(*                                                                          *)
(* Safety obligations (DUAL-CAP-1..2):                                      *)
(*   - CaptureOnce: after construction the captured revision never changes; *)
(*   - MutationIsolation: a source mutation never touches the captured      *)
(*     revision;                                                            *)
(*   - SurvivesSourceDrop: a constructed WFST can always be traversed, even *)
(*     after the source handle is dropped.                                  *)
(*                                                                          *)
(* Registry: proofs/doc/abi-invariants.tsv, DUAL-CAP-1..2.                 *)
(***************************************************************************)
EXTENDS Integers

CONSTANT MaxRevisions

VARIABLES
  liveRev,      \* the source dictionary's current revision
  captured,     \* the WFST's captured revision, or -1 before construction
  constructed,  \* whether the WFST has been built
  sourceAlive   \* whether the source dictionary handle is still alive

vars == <<liveRev, captured, constructed, sourceAlive>>

NONE == -1

TypeOK ==
  /\ liveRev \in 0..MaxRevisions
  /\ captured \in (0..MaxRevisions) \cup {NONE}
  /\ constructed \in BOOLEAN
  /\ sourceAlive \in BOOLEAN

Init ==
  /\ liveRev = 0
  /\ captured = NONE
  /\ constructed = FALSE
  /\ sourceAlive = TRUE

\* Mutating the live dictionary bumps its revision. The captured snapshot is
\* untouched (it holds its own retained copy).
Mutate ==
  /\ sourceAlive
  /\ liveRev < MaxRevisions
  /\ liveRev' = liveRev + 1
  /\ UNCHANGED <<captured, constructed, sourceAlive>>

\* Construction captures the current revision -- exactly once (guarded by
\* ~constructed).
Construct ==
  /\ ~constructed
  /\ sourceAlive
  /\ captured' = liveRev
  /\ constructed' = TRUE
  /\ UNCHANGED <<liveRev, sourceAlive>>

\* The source handle may be dropped after construction; the captured snapshot
\* keeps its data alive.
DropSource ==
  /\ sourceAlive
  /\ sourceAlive' = FALSE
  /\ UNCHANGED <<liveRev, captured, constructed>>

\* Traversing a constructed WFST reads the captured revision. It is always
\* enabled once constructed, regardless of the source handle's liveness.
Traverse ==
  /\ constructed
  /\ UNCHANGED vars

Next == Mutate \/ Construct \/ DropSource \/ Traverse

Spec == Init /\ [][Next]_vars

(***************************************************************************)
(* Invariants                                                              *)
(***************************************************************************)

\* DUAL-CAP-1: once constructed, the captured revision never changes again.
CaptureOnce ==
  [][(constructed /\ constructed') => captured' = captured]_vars

\* DUAL-CAP-2 (state form): a mutation of the live dictionary never alters the
\* captured revision.
MutationIsolation ==
  [][Mutate => captured' = captured]_vars

\* Once constructed, the captured revision is a real revision (the capture
\* happened), so a traversal always has a snapshot to read -- even after the
\* source is dropped.
SurvivesSourceDrop ==
  (constructed => captured # NONE)

\* The captured revision is one that actually existed on the source (soundness
\* of the capture: it is never fabricated).
CapturedIsReal ==
  (constructed => captured \in 0..MaxRevisions)

===============================================================================
