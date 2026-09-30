"""Turn fuzzy dictionary queries into composable lazy weighted transducers.

`duallity` captures a Unicode dictionary revision at construction and exposes
one of nine native fuzzy-search transducers through Vinary Tree's shared
`vt.scalar-wfst.1` resource interface. The result is an ordinary
`ScalarWfst`: it supports deterministic context-manager ownership, snapshots,
lazy state traversal, and direct composition with lling-llang.
"""

from __future__ import annotations

import ctypes
from dataclasses import dataclass
from typing import Any

from vinary_tree_interop import (
    NativeResource,
    ScalarWfst,
    ScalarWfstArc,
    ScalarWfstStateInfo,
    UnitDomain,
    VtResource,
    WeightDomain,
    WfstFlag,
)

from ._abi import (
    ABI_VERSION,
    API_REVISION,
    Algorithm,
    CachePolicy,
    DuallityCacheStatisticsV1,
    DuallityGeneralizedLimitsV1,
    DuallityOperationV1,
    DuallityRecordHeaderV1,
    DuallityRestrictionV1,
    DuallityWfstOptionsV1,
    NativeError,
    OperationApplicability,
    Status,
    WfstKind,
    abi_version,
    api_revision,
    check,
    lib,
    native_resource,
)

__version__ = "4.0.0rc6"


class Wfst(ScalarWfst):
    """Owned lazy WFST; its handle and interop-resource retain are independent."""

    _handle: ctypes.c_void_p

    def _live_handle(self) -> ctypes.c_void_p:
        handle = getattr(self, "_handle", None)
        if handle is None or not handle.value:
            raise NativeError(Status.NULL_POINTER, "wfst", "WFST handle is closed")
        return handle

    def options(self) -> OptionsSnapshot:
        """Return a deep copy of effective configuration, safe after close."""
        raw = DuallityWfstOptionsV1()
        raw.header = _header(DuallityWfstOptionsV1)
        check(
            lib.duallity_wfst_options_get(self._live_handle(), ctypes.byref(raw)),
            "wfst_options_get",
        )
        limits = None
        if raw.limits:
            value = raw.limits.contents
            limits = LimitsSnapshot(
                *(int(getattr(value, name)) for name in _LIMIT_FIELDS)
            )
        operations: list[OperationSnapshot] = []
        for index in range(raw.operation_count):
            item = ctypes.cast(
                ctypes.cast(raw.operations, ctypes.c_void_p).value
                + index * raw.operation_stride,
                ctypes.POINTER(DuallityOperationV1),
            ).contents
            restrictions: list[RestrictionSnapshot] = []
            for pair_index in range(item.restriction_count):
                pair = ctypes.cast(
                    ctypes.cast(item.restrictions, ctypes.c_void_p).value
                    + pair_index * item.restriction_stride,
                    ctypes.POINTER(DuallityRestrictionV1),
                ).contents
                restrictions.append(
                    RestrictionSnapshot(
                        _copied_text(pair.source_data, pair.source_len),
                        _copied_text(pair.target_data, pair.target_len),
                    )
                )
            operations.append(
                OperationSnapshot(
                    int(item.consume_x),
                    int(item.consume_y),
                    float(item.weight),
                    OperationApplicability(item.applicability),
                    _copied_text(item.name_data, item.name_len),
                    tuple(restrictions),
                )
            )
        return OptionsSnapshot(
            WfstKind(raw.kind),
            Algorithm(raw.algorithm),
            int(raw.maximum_distance),
            CachePolicy(raw.cache_policy),
            int(raw.cache_capacity),
            limits,
            tuple(operations),
        )

    def cache_statistics(self) -> DuallityCacheStatisticsV1:
        """Copy cumulative counters and the current cache residency."""
        raw = DuallityCacheStatisticsV1()
        raw.header = _header(DuallityCacheStatisticsV1)
        check(
            lib.duallity_wfst_cache_statistics(self._live_handle(), ctypes.byref(raw)),
            "wfst_cache_statistics",
        )
        return raw

    def clear_cache(self) -> None:
        """Clear cached payloads without changing WFST state identities."""
        check(lib.duallity_wfst_cache_clear(self._live_handle()), "wfst_cache_clear")

    def set_cache_policy(
        self,
        policy: CachePolicy | int,
        capacity: int = 0,
    ) -> None:
        """Atomically switch policy and clear existing cache residency."""
        value = _selector(policy, CachePolicy, "cache policy")
        check(
            lib.duallity_wfst_cache_set_policy(
                self._live_handle(), value, _uint64(capacity, "capacity")
            ),
            "wfst_cache_set_policy",
        )

    def close(self) -> None:
        """Release both owned native objects exactly once."""
        try:
            super().close()
        finally:
            handle = getattr(self, "_handle", None)
            if handle is not None and handle.value:
                lib.duallity_wfst_free(handle)
                self._handle = ctypes.c_void_p()


@dataclass(frozen=True)
class RestrictionSnapshot:
    source: str
    target: str


@dataclass(frozen=True)
class OperationSnapshot:
    consume_x: int
    consume_y: int
    weight: float
    applicability: OperationApplicability
    name: str
    restrictions: tuple[RestrictionSnapshot, ...]


@dataclass(frozen=True)
class LimitsSnapshot:
    max_query_bytes: int
    max_query_scalars: int
    max_operation_source_scalars: int
    max_operation_query_scalars: int
    max_retained_dictionary_nodes: int
    max_retained_wfst_states: int
    max_paths_per_expansion: int
    max_work_units_per_expansion: int


@dataclass(frozen=True)
class OptionsSnapshot:
    kind: WfstKind
    algorithm: Algorithm
    maximum_distance: int
    cache_policy: CachePolicy
    cache_capacity: int
    limits: LimitsSnapshot | None
    operations: tuple[OperationSnapshot, ...]


_LIMIT_FIELDS = (
    "max_query_bytes",
    "max_query_scalars",
    "max_operation_source_scalars",
    "max_operation_query_scalars",
    "max_retained_dictionary_nodes",
    "max_retained_wfst_states",
    "max_paths_per_expansion",
    "max_work_units_per_expansion",
)


def _header(record: type[ctypes.Structure]) -> DuallityRecordHeaderV1:
    return DuallityRecordHeaderV1(ctypes.sizeof(record), 1, 0)


def _copied_text(data: Any, length: int) -> str:
    return ctypes.string_at(data, length).decode("utf-8") if length else ""


def default_options() -> DuallityWfstOptionsV1:
    """Return a writable revision-3 options record with native defaults."""
    raw = DuallityWfstOptionsV1()
    raw.header = _header(DuallityWfstOptionsV1)
    check(lib.duallity_wfst_options_default(ctypes.byref(raw)), "wfst_options_default")
    return raw


def _size(value: object, subject: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{subject} must be an integer")
    if not 0 <= value < 2 ** (ctypes.sizeof(ctypes.c_size_t) * 8):
        raise ValueError(f"{subject} does not fit size_t")
    return value


def _uint64(value: object, subject: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{subject} must be an integer")
    if not 0 <= value < 2**64:
        raise ValueError(f"{subject} does not fit uint64_t")
    return value


def _selector(
    value: object, enum: type[Algorithm | WfstKind | CachePolicy], subject: str
) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{subject} must be an integer enum value")
    try:
        return int(enum(value))
    except ValueError as error:
        raise ValueError(f"unknown {subject}: {value!r}") from error


def _query_bytes(query: str) -> tuple[Any, int, object]:
    if not isinstance(query, str):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise TypeError("query must be str")
    encoded = query.encode("utf-8")
    if not encoded:
        return ctypes.POINTER(ctypes.c_uint8)(), 0, None
    buffer = (ctypes.c_uint8 * len(encoded)).from_buffer_copy(encoded)
    return ctypes.cast(buffer, ctypes.POINTER(ctypes.c_uint8)), len(encoded), buffer


def _adopt_handle(handle: ctypes.c_void_p) -> Wfst:
    if not handle.value:
        raise NativeError(Status.PANIC, "wfst_new", "native returned a null handle")
    resource = VtResource()
    try:
        check(
            lib.duallity_wfst_resource(handle, ctypes.byref(resource)), "wfst_resource"
        )
        if not resource.context or not resource.vtable:
            raise NativeError(
                Status.PANIC, "wfst_resource", "native returned a null resource"
            )
        try:
            graph = Wfst(resource)
        finally:
            lib.duallity_resource_release(resource)
        graph._handle = handle
        return graph
    except BaseException:
        lib.duallity_wfst_free(handle)
        raise


def wfst(
    dictionary: NativeResource | VtResource,
    query: str,
    *,
    maximum_distance: int = 1,
    algorithm: Algorithm | int = Algorithm.STANDARD,
    kind: WfstKind | int = WfstKind.LEVENSHTEIN,
) -> Wfst:
    """Capture `dictionary` and construct one lazy fuzzy-search WFST.

    `dictionary` must implement `vt.dictionary.v1` over Unicode scalars.
    The result owns an independent retain of the query-start snapshot, so the
    source may be mutated or closed as soon as this call returns. `algorithm`
    selects the edit family for `WfstKind.LEVENSHTEIN`; the other kinds
    encode their edit family in `kind`. Universal and generalized kinds use
    an unsigned eight-bit distance, while parameterized Levenshtein accepts the
    platform's full `size_t` range and FZF ignores the distance.
    """
    distance = _size(maximum_distance, "maximum_distance")
    algorithm_value = _selector(algorithm, Algorithm, "algorithm")
    kind_value = _selector(kind, WfstKind, "WFST kind")
    raw = native_resource(dictionary)
    data, query_len, _buffer = _query_bytes(query)
    handle = ctypes.c_void_p()
    check(
        lib.duallity_wfst_new_ref(
            ctypes.byref(raw),
            data,
            query_len,
            distance,
            algorithm_value,
            kind_value,
            ctypes.byref(handle),
        ),
        "wfst_new",
    )
    return _adopt_handle(handle)


def configured_wfst(
    dictionary: NativeResource | VtResource,
    query: str,
    options: DuallityWfstOptionsV1,
) -> Wfst:
    """Construct a revision-3 WFST from a versioned raw options record.

    Nested ctypes arrays and buffers must stay alive until this call returns;
    native construction then deep-copies custom operation text. Call
    :func:`default_options` to obtain a correctly initialized base record.
    """
    if not isinstance(options, DuallityWfstOptionsV1):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise TypeError("options must be DuallityWfstOptionsV1")
    raw = native_resource(dictionary)
    data, query_len, _buffer = _query_bytes(query)
    handle = ctypes.c_void_p()
    check(
        lib.duallity_wfst_new_configured_ref(
            ctypes.byref(raw),
            data,
            query_len,
            ctypes.byref(options),
            ctypes.byref(handle),
        ),
        "wfst_new_configured",
    )
    return _adopt_handle(handle)


__all__ = [
    "ABI_VERSION",
    "API_REVISION",
    "Algorithm",
    "CachePolicy",
    "DuallityCacheStatisticsV1",
    "DuallityGeneralizedLimitsV1",
    "DuallityOperationV1",
    "DuallityRecordHeaderV1",
    "DuallityRestrictionV1",
    "DuallityWfstOptionsV1",
    "LimitsSnapshot",
    "NativeError",
    "NativeResource",
    "OperationApplicability",
    "OperationSnapshot",
    "OptionsSnapshot",
    "RestrictionSnapshot",
    "ScalarWfstArc",
    "ScalarWfstStateInfo",
    "Status",
    "UnitDomain",
    "VtResource",
    "WeightDomain",
    "Wfst",
    "WfstFlag",
    "WfstKind",
    "__version__",
    "abi_version",
    "api_revision",
    "configured_wfst",
    "default_options",
    "wfst",
]
