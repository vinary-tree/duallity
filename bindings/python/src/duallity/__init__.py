"""Turn fuzzy dictionary queries into composable lazy weighted transducers.

`duallity` captures a Unicode dictionary revision at construction and exposes
one of nine native fuzzy-search transducers through Vinary Tree's shared
`vt.scalar-wfst.1` resource interface. The result is an ordinary
`ScalarWfst`: it supports deterministic context-manager ownership, snapshots,
lazy state traversal, and direct composition with lling-llang.
"""

from __future__ import annotations

import ctypes

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
    NativeError,
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
    """Owned lazy duallity WFST with shared snapshot and traversal operations."""


def _size(value: object, subject: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{subject} must be an integer")
    if not 0 <= value < 2 ** (ctypes.sizeof(ctypes.c_size_t) * 8):
        raise ValueError(f"{subject} does not fit size_t")
    return value


def _selector(value: object, enum: type[Algorithm | WfstKind], subject: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{subject} must be an integer enum value")
    try:
        return int(enum(value))
    except ValueError as error:
        raise ValueError(f"unknown {subject}: {value!r}") from error


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
    if not isinstance(query, str):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise TypeError("query must be str")
    distance = _size(maximum_distance, "maximum_distance")
    algorithm_value = _selector(algorithm, Algorithm, "algorithm")
    kind_value = _selector(kind, WfstKind, "WFST kind")
    raw = native_resource(dictionary)
    encoded = query.encode("utf-8")
    buffer = (
        (ctypes.c_uint8 * len(encoded)).from_buffer_copy(encoded) if encoded else None
    )
    data = (
        ctypes.cast(buffer, ctypes.POINTER(ctypes.c_uint8))
        if buffer is not None
        else ctypes.POINTER(ctypes.c_uint8)()
    )
    handle = ctypes.c_void_p()
    check(
        lib.duallity_wfst_new_ref(
            ctypes.byref(raw),
            data,
            len(encoded),
            distance,
            algorithm_value,
            kind_value,
            ctypes.byref(handle),
        ),
        "wfst_new",
    )
    if not handle.value:
        raise NativeError(
            Status.PANIC,
            "wfst_new",
            "native operation returned a null successful handle",
        )
    resource = VtResource()
    try:
        check(
            lib.duallity_wfst_resource(handle, ctypes.byref(resource)),
            "wfst_resource",
        )
    finally:
        lib.duallity_wfst_free(handle)
    if not resource.context or not resource.vtable:
        raise NativeError(
            Status.PANIC,
            "wfst_resource",
            "native operation returned a null successful resource",
        )
    return Wfst.adopt(resource)


__all__ = [
    "ABI_VERSION",
    "API_REVISION",
    "Algorithm",
    "NativeError",
    "NativeResource",
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
    "wfst",
]
