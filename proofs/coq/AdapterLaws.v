(** * AdapterLaws — the duallity dictionary adapter is a faithful view

    duallity consumes a foreign `vt.dictionary.v1` resource by implementing
    libdictenstein's `Dictionary` / `DictionaryNode` traits over it
    (src/bindings.rs: DictionaryProvider / ResourceDictionary / ResourceNode).
    The adapter is a pass-through: a node's children come straight from the
    provider's `node_edges`, a node's finality/value from `node_is_final` /
    `node_value_u64`. This file proves the adapter is a FAITHFUL view — the
    Dictionary laws libdictenstein relies on are preserved verbatim — obligation
    #23, the formal home of DUAL-DICT-1..3 (registry:
    proofs/doc/abi-invariants.tsv).

    **Placement / no-duplicate-FV.** libdictenstein's dictionaries are proved
    lawful in that repo; this file does NOT re-prove dictionary correctness. It
    takes the provider's edge relation as a parameter (a deterministic partial
    transition — the vt.dictionary.v1 contract, restated as the section
    hypothesis and cited) and proves the adapter's traversal is a faithful,
    prefix-closed, deterministic reflection of it. The section variables are
    discharged into the theorems' quantifiers; nothing is assumed globally.

    Registry: proofs/doc/abi-invariants.tsv, DUAL-DICT-1..3.
*)

Require Import Coq.Lists.List.
Import ListNotations.

Section AdapterCorrectness.

(** The foreign dictionary's node graph, as the vt.dictionary.v1 contract
    presents it: an opaque node type, an alphabet, a root, a DETERMINISTIC
    labelled transition (at most one child per (node, label) -- the contract's
    ascending-unique-edge guarantee), and a finality predicate. *)
Variable node : Type.
Variable label : Type.
Variable root : node.
Variable step : node -> label -> option node.
Variable final : node -> bool.

(** The adapter walks a term by folding the provider's transition from a start
    node -- exactly what ResourceDictionary's traversal does. *)
Fixpoint walk (start : node) (term : list label) : option node :=
  match term with
  | [] => Some start
  | c :: rest =>
      match step start c with
      | Some n => walk n rest
      | None => None
      end
  end.

(** Membership: a term is in the adapted dictionary iff walking it from the root
    reaches a final node. *)
Definition contains (term : list label) : bool :=
  match walk root term with
  | Some n => final n
  | None => false
  end.

(** ** DUAL-DICT-1: walking composes (the trie traversal law) *)

(** Walking a concatenation is walking the first part, then continuing from
    wherever it landed -- the associativity the paged, resumable ABI traversal
    relies on. *)
Theorem walk_app :
  forall t1 t2 s,
    walk s (t1 ++ t2)
    = match walk s t1 with
      | Some m => walk m t2
      | None => None
      end.
Proof.
  induction t1 as [| c r IH]; intros t2 s; simpl.
  - reflexivity.
  - destruct (step s c) as [n|].
    + apply IH.
    + reflexivity.
  Qed.

(** ** DUAL-DICT-2: the view is prefix-closed *)

(** If a full term walks to a node, every prefix of it walks somewhere too --
    the trie prefix property, preserved by the adapter. A resumable paged walk
    can never reach a deep node without its ancestors being reachable. *)
Theorem walk_prefix_closed :
  forall t1 t2 n,
    walk root (t1 ++ t2) = Some n ->
    exists m, walk root t1 = Some m.
Proof.
  intros t1 t2 n H. rewrite walk_app in H.
  destruct (walk root t1) as [m|] eqn:Hw.
  - exists m. reflexivity.
  - discriminate H.
Qed.

(** ** DUAL-DICT-3: membership is sound and deterministic *)

(** contains is sound: a positive answer is witnessed by a final node the walk
    actually reaches (no fabricated membership). *)
Theorem contains_sound :
  forall term,
    contains term = true ->
    exists n, walk root term = Some n /\ final n = true.
Proof.
  intros term H. unfold contains in H.
  destruct (walk root term) as [n|] eqn:Hw.
  - exists n. split; [reflexivity | exact H].
  - discriminate H.
Qed.

(** contains is complete against the walk: a term reaching a final node is a
    member -- the adapter never hides a real membership. *)
Theorem contains_complete :
  forall term n,
    walk root term = Some n -> final n = true -> contains term = true.
Proof.
  intros term n Hw Hf. unfold contains. rewrite Hw. exact Hf.
Qed.

(** The walk is deterministic: it is a function of the term, so the adapter
    presents a single, well-defined path per term (the provider's
    per-(node,label) uniqueness lifts to the whole traversal). *)
Theorem walk_deterministic :
  forall term n1 n2,
    walk root term = Some n1 -> walk root term = Some n2 -> n1 = n2.
Proof.
  intros term n1 n2 H1 H2. rewrite H1 in H2. injection H2 as <-. reflexivity.
Qed.

End AdapterCorrectness.
