"""Exact `ctypes` declarations for duallity's stable C ABI."""

from __future__ import annotations

import ctypes
import ctypes.util
import os
import platform
from enum import IntEnum
from pathlib import Path
from typing import Any

from vinary_tree_interop import NativeResource, VtResource

ABI_VERSION = 1
API_REVISION = 3


class Status(IntEnum):
    """Stable status values returned by duallity's native boundary."""

    OK = 0
    INVALID_ARGUMENT = 1
    INVALID_UTF8 = 2
    NULL_POINTER = 3
    PANIC = 4
    INCOMPATIBLE_RESOURCE = 5
    PROVIDER_ERROR = 6
    LIMIT_EXCEEDED = 7


class Algorithm(IntEnum):
    """Edit-operation family for the parameterized Levenshtein adapter."""

    STANDARD = 0
    TRANSPOSITION = 1
    MERGE_AND_SPLIT = 2
    DAMERAU_LEVENSHTEIN = 3


class WfstKind(IntEnum):
    """Lazy weighted-transducer implementation selected at construction."""

    LEVENSHTEIN = 0
    UNIVERSAL_STANDARD = 1
    UNIVERSAL_TRANSPOSITION = 2
    UNIVERSAL_MERGE_AND_SPLIT = 3
    GENERALIZED_STANDARD = 4
    GENERALIZED_TRANSPOSITION = 5
    GENERALIZED_MERGE_AND_SPLIT = 6
    GENERALIZED_PHONETIC = 7
    FZF = 8


class DuallityRecordHeaderV1(ctypes.Structure):
    """Common sized-record prefix; callers set all three fields before a call."""

    _fields_ = [
        ("struct_size", ctypes.c_uint32),
        ("record_version", ctypes.c_uint32),
        ("reserved", ctypes.c_uint64),
    ]


class DuallityRestrictionV1(ctypes.Structure):
    """Borrowed UTF-8 source/target pair for one listed operation."""

    _fields_ = [
        ("header", DuallityRecordHeaderV1),
        ("source_data", ctypes.POINTER(ctypes.c_uint8)),
        ("source_len", ctypes.c_uint64),
        ("target_data", ctypes.POINTER(ctypes.c_uint8)),
        ("target_len", ctypes.c_uint64),
        ("reserved", ctypes.c_uint64 * 2),
    ]


class DuallityOperationV1(ctypes.Structure):
    """One custom edit operation; nested buffers are borrowed for construction."""

    _fields_ = [
        ("header", DuallityRecordHeaderV1),
        ("consume_x", ctypes.c_uint64),
        ("consume_y", ctypes.c_uint64),
        ("weight", ctypes.c_double),
        ("applicability", ctypes.c_uint32),
        ("reserved_zero", ctypes.c_uint32),
        ("name_data", ctypes.POINTER(ctypes.c_uint8)),
        ("name_len", ctypes.c_uint64),
        ("restrictions", ctypes.POINTER(DuallityRestrictionV1)),
        ("restriction_count", ctypes.c_uint64),
        ("restriction_stride", ctypes.c_uint64),
        ("reserved", ctypes.c_uint64 * 2),
    ]


class DuallityGeneralizedLimitsV1(ctypes.Structure):
    """Inclusive generalized-construction and expansion ceilings."""

    _fields_ = [
        ("header", DuallityRecordHeaderV1),
        ("max_query_bytes", ctypes.c_uint64),
        ("max_query_scalars", ctypes.c_uint64),
        ("max_operation_source_scalars", ctypes.c_uint64),
        ("max_operation_query_scalars", ctypes.c_uint64),
        ("max_retained_dictionary_nodes", ctypes.c_uint64),
        ("max_retained_wfst_states", ctypes.c_uint64),
        ("max_paths_per_expansion", ctypes.c_uint64),
        ("max_work_units_per_expansion", ctypes.c_uint64),
        ("reserved", ctypes.c_uint64 * 2),
    ]


class DuallityWfstOptionsV1(ctypes.Structure):
    """Configured WFST input or handle-borrowed effective readback."""

    _fields_ = [
        ("header", DuallityRecordHeaderV1),
        ("kind", ctypes.c_uint32),
        ("algorithm", ctypes.c_uint32),
        ("maximum_distance", ctypes.c_uint64),
        ("cache_policy", ctypes.c_uint32),
        ("reserved_zero", ctypes.c_uint32),
        ("cache_capacity", ctypes.c_uint64),
        ("limits", ctypes.POINTER(DuallityGeneralizedLimitsV1)),
        ("operations", ctypes.POINTER(DuallityOperationV1)),
        ("operation_count", ctypes.c_uint64),
        ("operation_stride", ctypes.c_uint64),
        ("reserved", ctypes.c_uint64 * 2),
    ]


class DuallityCacheStatisticsV1(ctypes.Structure):
    """Cumulative provider-cache counters and current residency."""

    _fields_ = [
        ("header", DuallityRecordHeaderV1),
        ("hits", ctypes.c_uint64),
        ("misses", ctypes.c_uint64),
        ("faults", ctypes.c_uint64),
        ("uncacheable_results", ctypes.c_uint64),
        ("insertions", ctypes.c_uint64),
        ("evictions", ctypes.c_uint64),
        ("raced_publications", ctypes.c_uint64),
        ("clears", ctypes.c_uint64),
        ("resident_states", ctypes.c_uint64),
        ("recency_records", ctypes.c_uint64),
        ("reserved", ctypes.c_uint64 * 2),
    ]


class NativeError(RuntimeError):
    """Native duallity failure with its status, operation, and copied detail."""

    def __init__(self, status: int | Status, operation: str, message: str) -> None:
        super().__init__(f"{operation} failed: {message}")
        try:
            self.status: Status | int = Status(status)
        except ValueError:
            self.status = int(status)
        self.operation = operation


def _library_names() -> tuple[str, ...]:
    system = platform.system()
    if system == "Windows":
        return ("duallity.dll",)
    if system == "Darwin":
        return ("libduallity.dylib",)
    return ("libduallity.so",)


def _load_library() -> ctypes.CDLL:
    candidates: list[str] = []
    if explicit := os.environ.get("DUALLITY_LIBRARY"):
        candidates.append(explicit)
    package = Path(__file__).resolve().parent
    candidates.extend(str(package / "native" / name) for name in _library_names())
    if discovered := ctypes.util.find_library("duallity"):
        candidates.append(discovered)
    candidates.extend(_library_names())
    failures: list[str] = []
    for candidate in candidates:
        try:
            return ctypes.CDLL(candidate)
        except OSError as error:
            failures.append(f"{candidate}: {error}")
    raise ImportError(
        "could not load duallity; set DUALLITY_LIBRARY\n" + "\n".join(failures)
    )


lib: Any = _load_library()


def _bind(
    name: str,
    arguments: list[Any],
    result: object = ctypes.c_uint32,
) -> None:
    function = getattr(lib, name)
    function.argtypes = arguments
    function.restype = result


_bind("duallity_abi_version", [])
_bind("duallity_api_revision", [])
_bind("duallity_last_error_message", [], ctypes.c_char_p)
_bind(
    "duallity_wfst_new_ref",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(ctypes.c_uint8),
        ctypes.c_size_t,
        ctypes.c_size_t,
        ctypes.c_uint32,
        ctypes.c_uint32,
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind("duallity_wfst_free", [ctypes.c_void_p], None)
_bind(
    "duallity_wfst_resource",
    [ctypes.c_void_p, ctypes.POINTER(VtResource)],
)
_bind("duallity_wfst_options_default", [ctypes.POINTER(DuallityWfstOptionsV1)])
_bind(
    "duallity_wfst_new_configured_ref",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(ctypes.c_uint8),
        ctypes.c_size_t,
        ctypes.POINTER(DuallityWfstOptionsV1),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "duallity_wfst_options_get",
    [ctypes.c_void_p, ctypes.POINTER(DuallityWfstOptionsV1)],
)
_bind(
    "duallity_wfst_cache_statistics",
    [ctypes.c_void_p, ctypes.POINTER(DuallityCacheStatisticsV1)],
)
_bind("duallity_wfst_cache_clear", [ctypes.c_void_p])
_bind(
    "duallity_wfst_cache_set_policy",
    [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint64],
)


def last_error_message() -> str:
    """Copy this thread's diagnostic before another ABI call can replace it."""
    raw = lib.duallity_last_error_message()
    return raw.decode("utf-8", "replace") if raw else "native operation failed"


def check(status: int, operation: str) -> None:
    """Raise `NativeError` unless `status` denotes success."""
    if status != Status.OK:
        raise NativeError(status, operation, last_error_message())


def native_resource(resource: NativeResource | VtResource) -> VtResource:
    """Copy a live borrowed two-word resource from a compatible facade."""
    if isinstance(resource, VtResource):
        raw = resource
    else:
        try:
            raw = resource.native_resource
        except AttributeError as error:
            raise TypeError(
                "dictionary must be a VtResource or expose native_resource"
            ) from error
    if not isinstance(raw, VtResource):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise TypeError("native_resource must return VtResource")
    if not raw.context or not raw.vtable:
        raise NativeError(
            Status.INCOMPATIBLE_RESOURCE, "dictionary", "resource is closed"
        )
    return VtResource(raw.context, raw.vtable)


def abi_version() -> int:
    """Return the loaded native ABI version."""
    return int(lib.duallity_abi_version())


def api_revision() -> int:
    """Return the loaded native additive API revision."""
    return int(lib.duallity_api_revision())


if abi_version() != ABI_VERSION:
    raise ImportError(
        f"duallity native ABI {abi_version()} does not match {ABI_VERSION}"
    )
if api_revision() < API_REVISION:
    raise ImportError(
        f"duallity native API revision {api_revision()} is older than {API_REVISION}"
    )
