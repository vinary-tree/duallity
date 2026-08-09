(** * StateEncoding — the duallity product-state codec

    duallity packs a product state $`(\text{dict\_node}, \text{automaton\_state})`$
    into a single lling-llang `u32` [StateId] so a Levenshtein (or universal /
    generalized / phonetic / fzf) automaton can be composed as a scalar WFST.
    The codec is `duallity::state_encoding::{try_encode, decode}` (src/lib.rs):

      try_encode(d, a, M) = if M = 0 or a >= M then None
                            else checked(d * M + a)         (* None on u32 overflow *)
      decode(id, M)       = if M = 0 then None else Some (id / M, id mod M)

    This file is the formal model of that codec and its correctness — obligation
    #21, the formal home of DUAL-ENC-1..4 (registry: proofs/doc/abi-invariants.tsv).

    The `u32` overflow of Rust's `checked_mul`/`checked_add` is modeled by a word
    bound [WORD] = 2^32: `try_encode` succeeds exactly when the packed value fits
    in a machine word (equivalently, both checked operations succeed, since
    `d*M + a < 2^32` implies `d*M < 2^32`). All arithmetic is over exact [nat];
    the theorems therefore hold for the mathematical codec and its bounded
    realization alike.

    Registry: proofs/doc/abi-invariants.tsv, DUAL-ENC-1..4.
*)

Require Import Coq.Arith.Arith.
Require Import Coq.Bool.Bool.

(** The machine-word bound: a packed id must fit in a u32. *)
Definition WORD : nat := 4294967296. (* 2 ^ 32 *)

(** try_encode: pack (d, a) with product-space width M. *)
Definition try_encode (d a M : nat) : option nat :=
  if orb (M =? 0) (M <=? a) then None
  else if (d * M + a) <? WORD then Some (d * M + a) else None.

(** decode: unpack an id with width M. *)
Definition decode (id M : nat) : option (nat * nat) :=
  if M =? 0 then None else Some (id / M, id mod M).

(** ** DUAL-ENC-1: decode is a left inverse of try_encode (exact round trip) *)

Theorem decode_round_trip :
  forall d a M id, try_encode d a M = Some id -> decode id M = Some (d, a).
Proof.
  intros d a M id H. unfold try_encode in H.
  destruct (orb (M =? 0) (M <=? a)) eqn:Hg; [discriminate|].
  destruct ((d * M + a) <? WORD) eqn:Hlt; [|discriminate].
  injection H as <-.
  apply orb_false_iff in Hg. destruct Hg as [HM Ha].
  apply Nat.eqb_neq in HM. apply Nat.leb_gt in Ha.
  unfold decode.
  destruct (M =? 0) eqn:HM0; [apply Nat.eqb_eq in HM0; contradiction|].
  assert (Hdiv : (d * M + a) / M = d).
  { rewrite Nat.div_add_l by exact HM.
    rewrite (Nat.div_small a M Ha). apply Nat.add_0_r. }
  assert (Hmod : (d * M + a) mod M = a).
  { rewrite Nat.add_comm, Nat.Div0.mod_add.
    apply Nat.mod_small; exact Ha. }
  rewrite Hdiv, Hmod. reflexivity.
Qed.

(** ** DUAL-ENC-2: try_encode is injective *)

Theorem try_encode_injective :
  forall d1 a1 d2 a2 M id,
    try_encode d1 a1 M = Some id ->
    try_encode d2 a2 M = Some id ->
    d1 = d2 /\ a1 = a2.
Proof.
  intros d1 a1 d2 a2 M id H1 H2.
  apply decode_round_trip in H1.
  apply decode_round_trip in H2.
  rewrite H1 in H2. injection H2 as -> ->. split; reflexivity.
Qed.

(** ** DUAL-ENC-3: try_encode is defined exactly outside the reject conditions

    Stated as a direction pair (reject / accept) rather than a single iff, so the
    boundary between "None" and "Some (d*M+a)" is pinned in both directions. *)

(** try_encode returns None when the width is zero, the automaton index is out
    of range, or the packed value overflows a machine word. *)
Theorem try_encode_rejects :
  forall d a M,
    (M = 0 \/ M <= a \/ WORD <= d * M + a) -> try_encode d a M = None.
Proof.
  intros d a M H. unfold try_encode.
  destruct H as [HM0 | [Hle | Hover]].
  - subst M. reflexivity.
  - assert (Hg : orb (M =? 0) (M <=? a) = true).
    { apply orb_true_iff. right. apply Nat.leb_le. exact Hle. }
    rewrite Hg. reflexivity.
  - destruct (orb (M =? 0) (M <=? a)) eqn:Hg; [reflexivity|].
    assert (Hlt : (d * M + a) <? WORD = false).
    { apply Nat.ltb_ge. exact Hover. }
    rewrite Hlt. reflexivity.
Qed.

Theorem try_encode_accepts :
  forall d a M,
    M <> 0 -> a < M -> d * M + a < WORD -> try_encode d a M = Some (d * M + a).
Proof.
  intros d a M HM Ha Hover. unfold try_encode.
  assert (Hg : orb (M =? 0) (M <=? a) = false).
  { apply orb_false_iff. split.
    - apply Nat.eqb_neq. exact HM.
    - apply Nat.leb_gt. exact Ha. }
  rewrite Hg.
  assert (Hlt : (d * M + a) <? WORD = true).
  { apply Nat.ltb_lt. exact Hover. }
  rewrite Hlt. reflexivity.
Qed.

(** ** DUAL-ENC-4: decode always yields an in-range automaton index *)

(** Whenever decode succeeds, the automaton component is a valid slot index
    (strictly below the width) -- the decoded pair is always well-formed. *)
Theorem decode_automaton_in_range :
  forall id M d a, decode id M = Some (d, a) -> a < M.
Proof.
  intros id M d a H. unfold decode in H.
  destruct (M =? 0) eqn:HM0; [discriminate|].
  apply Nat.eqb_neq in HM0.
  injection H as <- <-.
  apply Nat.mod_upper_bound. exact HM0.
Qed.

(** decode never fails for a positive width -- the codec is total on the
    representable side. *)
Theorem decode_total :
  forall id M, M <> 0 -> exists d a, decode id M = Some (d, a).
Proof.
  intros id M HM. unfold decode.
  destruct (M =? 0) eqn:HM0.
  - apply Nat.eqb_eq in HM0. contradiction.
  - exists (id / M), (id mod M). reflexivity.
Qed.
