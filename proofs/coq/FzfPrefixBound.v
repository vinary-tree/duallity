(** * FzfPrefixBound — the fzf arctic-weight telescoping and pruning bound

    duallity's [FzfWfst] (src/fzf_wfst.rs) is a lazy arctic-weighted WFST whose
    arc weights are fzf `FuzzyMatchV2` score DELTAS: "their max-plus product
    telescopes to the candidate's exact score" (src/fzf_wfst.rs:13-14). The
    prefix-shared DFS then prunes a subtree whose score ceiling cannot beat the
    running best without changing the exact top-k result
    (docs/scientific-ledger/fzf-prefix-bound-2026-08-02.md). This file mechanizes
    both facts -- obligation #24, the formal home of DUAL-FZF-1..2 (registry:
    proofs/doc/abi-invariants.tsv).

    **Placement / no-duplicate-FV.** The arctic (max-plus) semiring laws are
    proved in lling-llang (proofs/coq/foundations/ArcticWeight.v); this file does
    not re-prove them. It works over the arctic operations on the score type
    ($`\mathbb{Z}`$, since fzf scores are `i32` and may be negative), citing that
    model, and derives the two duallity-specific results: the delta product
    telescopes to the exact score (DUAL-FZF-1), and max-plus pruning is sound
    (DUAL-FZF-2).

    Registry: proofs/doc/abi-invariants.tsv, DUAL-FZF-1..2.
*)

Require Import Coq.ZArith.ZArith.
Require Import Coq.Lists.List.
Import ListNotations.

Open Scope Z_scope.

(** The arctic operations on scores (cited: lling-llang ArcticWeight, max-plus):
    the combine [oplus] is max, the extend [otimes] is addition, the arctic one
    (the [otimes] identity) is 0. *)
Definition oplus (a b : Z) : Z := Z.max a b.
Definition otimes (a b : Z) : Z := a + b.
Definition aone : Z := 0.

(** A path's arctic weight is the [otimes]-fold of its arc deltas from [aone]. *)
Definition path_weight (deltas : list Z) : Z := fold_right otimes aone deltas.

(** The running score after a prefix, tracked so deltas can be formed. *)
Fixpoint last_score (prev : Z) (scores : list Z) : Z :=
  match scores with
  | [] => prev
  | s :: rest => last_score s rest
  end.

(** The arc deltas along a scored prefix chain: each arc carries the increment
    from the previous prefix score to the current one. *)
Fixpoint deltas (prev : Z) (scores : list Z) : list Z :=
  match scores with
  | [] => []
  | s :: rest => (s - prev) :: deltas s rest
  end.

(** ** DUAL-FZF-1: the delta product telescopes to the exact score *)

(** The arctic product ([otimes] = +) of the arc deltas telescopes to the
    difference between the final prefix score and the starting score. *)
Theorem deltas_telescope :
  forall scores prev, path_weight (deltas prev scores) = last_score prev scores - prev.
Proof.
  induction scores as [| s rest IH]; intro prev; simpl.
  - unfold path_weight; simpl; unfold aone; ring.
  - unfold path_weight in *; simpl. rewrite IH. unfold otimes. ring.
Qed.

(** With the empty prefix scoring 0, an accepting path's weight IS the
    candidate's exact fzf score -- the telescoping claim of fzf_wfst.rs. *)
Theorem path_weight_is_exact_score :
  forall scores, path_weight (deltas 0 scores) = last_score 0 scores.
Proof.
  intro scores. rewrite deltas_telescope. ring.
Qed.

(** ** DUAL-FZF-2: max-plus pruning is sound *)

(** A candidate whose score ceiling cannot beat the running best does not change
    the best under the arctic combine -- so pruning that subtree preserves the
    exact maximum (the top-k soundness the DFS relies on). *)
Theorem prune_preserves_best :
  forall best ceiling, ceiling <= best -> oplus best ceiling = best.
Proof.
  intros best ceiling H. unfold oplus. apply Z.max_l. exact H.
Qed.

(** The arctic sum over a candidate list is an upper bound on every candidate:
    the fold never drops below the seed, and dominates each element. *)
Theorem fold_oplus_ge_seed :
  forall l x, x <= fold_right oplus x l.
Proof.
  induction l as [| c r IH]; intro x; simpl.
  - apply Z.le_refl.
  - unfold oplus. eapply Z.le_trans; [apply IH | apply Z.le_max_r].
Qed.

Theorem fold_oplus_ge_elem :
  forall l x c, In c l -> c <= fold_right oplus x l.
Proof.
  induction l as [| a r IH]; intros x c Hin; simpl in *.
  - contradiction.
  - destruct Hin as [-> | Hin].
    + unfold oplus. apply Z.le_max_l.
    + unfold oplus. eapply Z.le_trans; [apply (IH x c Hin) | apply Z.le_max_r].
Qed.

(** ** Arctic algebra facts used above (cited: lling ArcticWeight) *)

Theorem otimes_assoc : forall a b c, otimes (otimes a b) c = otimes a (otimes b c).
Proof. intros; unfold otimes; ring. Qed.

Theorem otimes_one_l : forall a, otimes aone a = a.
Proof. intro a; unfold otimes, aone; ring. Qed.

Theorem oplus_comm : forall a b, oplus a b = oplus b a.
Proof. intros; unfold oplus; apply Z.max_comm. Qed.

Theorem oplus_assoc : forall a b c, oplus (oplus a b) c = oplus a (oplus b c).
Proof. intros; unfold oplus; symmetry; apply Z.max_assoc. Qed.
