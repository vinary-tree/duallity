(** * StatusMapping — the duallity ABI status contract

    Every `duallity_*` C entry point returns a [DuallityStatus]; the binding
    layer classifies its internal [BindingError] into that alphabet through
    `map_error` (src/ffi.rs), and the `boundary` wrapper turns a caught panic
    into a distinct status. This file is the formal model of that classification
    and its safety properties -- obligation #25, the formal home of
    DUAL-STAT-1..3 (registry: proofs/doc/abi-invariants.tsv).

    The properties mirror the sibling status models of libdictenstein,
    liblevenshtein, and lling-llang: no error is ever swallowed into `Ok`, each
    error class maps as documented, and a caught panic is a distinct code from
    both success and every ordinary failure.

    Registry: proofs/doc/abi-invariants.tsv, DUAL-STAT-1..3.
*)

(** The interop status alphabet (vinary-tree-interop::VtStatus, discriminants
    0..8), carried by [Provider] failures. *)
Inductive VtStatus : Type :=
  | VOk
  | VEnd
  | VInvalidArgument
  | VNullPointer
  | VUnsupported
  | VIoError
  | VClosed
  | VLimitExceeded
  | VProviderError.

(** The duallity status alphabet (DuallityStatus, discriminants 0..7). *)
Inductive DuallityStatus : Type :=
  | DOk
  | DInvalidArgument
  | DInvalidUtf8
  | DNullPointer
  | DPanic
  | DIncompatibleResource
  | DProviderError
  | DLimitExceeded.

(** The binding-layer error type (src/bindings.rs::BindingError). *)
Inductive BindingError : Type :=
  | NullResource
  | Provider (s : VtStatus)
  | InvalidProviderOutput
  | InvalidArgument
  | IncompatibleResourceAbi
  | MissingDictionaryInterface
  | IncompatibleDictionaryInterface
  | UnitDomainMismatch.

(** `map_error` (src/ffi.rs). *)
Definition map_error (e : BindingError) : DuallityStatus :=
  match e with
  | NullResource => DNullPointer
  | Provider _ => DProviderError
  | InvalidProviderOutput => DProviderError
  | InvalidArgument => DInvalidArgument
  | IncompatibleResourceAbi => DIncompatibleResource
  | MissingDictionaryInterface => DIncompatibleResource
  | IncompatibleDictionaryInterface => DIncompatibleResource
  | UnitDomainMismatch => DIncompatibleResource
  end.

(** ** DUAL-STAT-1: no error is silently swallowed into success *)

Theorem map_error_never_ok : forall e, map_error e <> DOk.
Proof. destruct e; discriminate. Qed.

(** ** DUAL-STAT-2: the classification is exactly as documented *)

Theorem null_is_null_pointer : map_error NullResource = DNullPointer.
Proof. reflexivity. Qed.

Theorem provider_faults_are_provider_error :
  forall s, map_error (Provider s) = DProviderError
            /\ map_error InvalidProviderOutput = DProviderError.
Proof. intro s; split; reflexivity. Qed.

Theorem invalid_argument_is_invalid_argument :
  map_error InvalidArgument = DInvalidArgument.
Proof. reflexivity. Qed.

Theorem incompatibilities_are_incompatible_resource :
  map_error IncompatibleResourceAbi = DIncompatibleResource
  /\ map_error MissingDictionaryInterface = DIncompatibleResource
  /\ map_error IncompatibleDictionaryInterface = DIncompatibleResource
  /\ map_error UnitDomainMismatch = DIncompatibleResource.
Proof. repeat split; reflexivity. Qed.

(** The classification is exhaustive: every error maps into one of exactly four
    non-Ok status classes. *)
Theorem map_error_is_classified :
  forall e,
    map_error e = DNullPointer
    \/ map_error e = DProviderError
    \/ map_error e = DInvalidArgument
    \/ map_error e = DIncompatibleResource.
Proof.
  destruct e; simpl;
    ((left; reflexivity)
     || (right; left; reflexivity)
     || (right; right; left; reflexivity)
     || (right; right; right; reflexivity)).
Qed.

(** ** DUAL-STAT-3: the boundary wrapper *)

(** The `boundary()` wrapper: success yields Ok, a returned error status is
    forwarded verbatim, and an unwound panic yields Panic. *)
Inductive boundary_outcome : Type :=
  | Success
  | Failure (s : DuallityStatus)
  | Panicked.

Definition boundary_status (o : boundary_outcome) : DuallityStatus :=
  match o with
  | Success => DOk
  | Failure s => s
  | Panicked => DPanic
  end.

Theorem boundary_success_is_ok : boundary_status Success = DOk.
Proof. reflexivity. Qed.

Theorem boundary_panic_is_panic : boundary_status Panicked = DPanic.
Proof. reflexivity. Qed.

(** A failure carrying a mapped binding error never reports Ok. *)
Theorem boundary_mapped_failure_never_ok :
  forall e, boundary_status (Failure (map_error e)) <> DOk.
Proof. intro e; simpl; apply map_error_never_ok. Qed.

(** A caught panic is distinct from success. *)
Theorem panic_is_not_success :
  boundary_status Panicked <> boundary_status Success.
Proof. discriminate. Qed.
