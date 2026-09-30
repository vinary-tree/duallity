"""Python conformance across host providers and native family facades."""

from __future__ import annotations

import ctypes
import unittest

import duallity
import libdictenstein
import lling_llang
from vinary_tree_interop import (
    InteropError,
    ScalarWfst,
    UnicodeDictionaryResource,
    UnitDomain,
    WeightDomain,
    WfstFlag,
)


class TrieSnapshot:
    """Small immutable Unicode trie that exercises host-provider ingress."""

    def __init__(self, terms: tuple[str, ...]) -> None:
        nodes: list[dict[str, int]] = [{}]
        finals: set[int] = set()
        for term in terms:
            node = 0
            for label in term:
                node = nodes[node].setdefault(label, len(nodes))
                if node == len(nodes):
                    nodes.append({})
            finals.add(node)
        self._nodes = tuple(tuple(sorted(edges.items())) for edges in nodes)
        self._finals = frozenset(finals)

    def root(self) -> int:
        return 0

    def __len__(self) -> int:
        return len(self._finals)

    def is_final(self, node: int) -> bool:
        return node in self._finals

    def value(self, node: int) -> int | None:
        del node
        return None

    def edges(self, node: int) -> tuple[tuple[str, int], ...]:
        return self._nodes[node]


def language(graph: ScalarWfst) -> dict[str, float]:
    """Enumerate the finite dictionary-side language of a lazy test graph."""
    accepted: dict[str, float] = {}
    frontier = [(graph.start, "", 0.0)]
    visited: set[int] = set()
    while frontier:
        state, output, weight = frontier.pop()
        if state in visited:
            continue
        visited.add(state)
        if len(visited) > 100_000:
            raise AssertionError("WFST traversal did not converge")
        info = graph.state_info(state)
        if info is None:
            continue
        if info.final:
            accepted[output] = weight + info.final_weight
        for arc in graph.arcs(state):
            suffix = (
                ""
                if arc.output_label is None
                else arc.output_label
                if isinstance(arc.output_label, str)
                else chr(arc.output_label)
            )
            frontier.append((arc.target_state, output + suffix, weight + arc.weight))
    return accepted


def case_mapper(alphabet: str) -> lling_llang.Wfst:
    """Construct a one-state lower-to-upper transducer."""
    with lling_llang.WfstBuilder(size_hint=1) as builder:
        state = builder.add_state()
        builder.set_start(state).set_final(state)
        for character in alphabet:
            builder.add_arc(state, character, character.upper(), state)
        return builder.build()


class ApiTests(unittest.TestCase):
    def test_empty_unicode_and_embedded_nul_queries_use_explicit_utf8_lengths(
        self,
    ) -> None:
        snapshot = TrieSnapshot(("", "é", "a\0b"))
        with UnicodeDictionaryResource(lambda: snapshot) as dictionary:
            for query in ("", "é", "a\0b"):
                with (
                    self.subTest(query=query),
                    duallity.wfst(dictionary, query, maximum_distance=0) as graph,
                ):
                    self.assertEqual(language(graph), {query: 0.0})
            with self.assertRaises(UnicodeEncodeError):
                duallity.wfst(dictionary, "\ud800")

    def test_versions_enums_and_all_native_selectors(self) -> None:
        self.assertEqual(duallity.abi_version(), duallity.ABI_VERSION)
        self.assertGreaterEqual(duallity.api_revision(), duallity.API_REVISION)
        self.assertEqual(len(duallity.Algorithm), 4)
        self.assertEqual(len(duallity.WfstKind), 9)

        snapshot = TrieSnapshot(("cat", "cot", "dog"))
        with UnicodeDictionaryResource(lambda: snapshot) as dictionary:
            with duallity.wfst(dictionary.native_resource, "cat") as graph:
                self.assertIsNotNone(graph.state_info(graph.start))

            for kind in duallity.WfstKind:
                with duallity.wfst(dictionary, "cat", kind=kind) as graph:
                    self.assertIsInstance(graph, duallity.Wfst)
                    self.assertIs(graph.unit_domain, UnitDomain.UNICODE_SCALAR)
                    expected = (
                        WeightDomain.ARCTIC_F64
                        if kind is duallity.WfstKind.FZF
                        else WeightDomain.TROPICAL_F64
                    )
                    self.assertIs(graph.weight_domain, expected)
                    self.assertTrue(graph.flags & WfstFlag.LAZY)
                    self.assertIsNone(graph.state_count)
                    self.assertIsNotNone(graph.state_info(graph.start))

            for algorithm in duallity.Algorithm:
                with duallity.wfst(dictionary, "cat", algorithm=algorithm) as graph:
                    self.assertIsNotNone(graph.state_info(graph.start))

    def test_snapshot_survives_source_close_and_composes_with_lling_llang(self) -> None:
        dictionary = libdictenstein.DynamicDawg()
        dictionary.update_many((("cat", None), ("cot", None), ("dog", None)))
        graph = duallity.wfst(dictionary, "cat", maximum_distance=1)
        dictionary.close()
        self.assertEqual(language(graph), {"cat": 0.0, "cot": 1.0})

        mapper = case_mapper("acot")
        product = lling_llang.compose(graph, mapper)
        snapshot = product.snapshot()
        product.close()
        graph.close()
        mapper.close()
        with snapshot:
            self.assertEqual(language(snapshot), {"CAT": 0.0, "COT": 1.0})

    def test_argument_provider_and_lifecycle_failures_are_typed(self) -> None:
        snapshot = TrieSnapshot(("cat",))
        dictionary = UnicodeDictionaryResource(lambda: snapshot)
        for bad in (-1, True, 1.5, 2 ** (8 * ctypes.sizeof(ctypes.c_size_t))):
            with (
                self.subTest(maximum_distance=bad),
                self.assertRaises((TypeError, ValueError)),
            ):
                duallity.wfst(dictionary, "cat", maximum_distance=bad)  # type: ignore[arg-type]
        with self.assertRaises(TypeError):
            duallity.wfst(dictionary, b"cat")  # type: ignore[arg-type]
        with self.assertRaises(ValueError):
            duallity.wfst(dictionary, "cat", algorithm=99)
        with self.assertRaises(ValueError):
            duallity.wfst(dictionary, "cat", kind=99)
        with self.assertRaises(duallity.NativeError) as distance:
            duallity.wfst(
                dictionary,
                "cat",
                maximum_distance=256,
                kind=duallity.WfstKind.GENERALIZED_STANDARD,
            )
        self.assertIs(distance.exception.status, duallity.Status.INVALID_ARGUMENT)
        future = duallity.NativeError(999, "future", "unknown status")
        self.assertEqual(future.status, 999)
        self.assertEqual(future.operation, "future")

        graph = duallity.wfst(dictionary, "cat")
        graph.close()
        graph.close()
        with self.assertRaises(InteropError):
            _ = graph.start
        dictionary.close()
        with self.assertRaises((RuntimeError, duallity.NativeError)):
            duallity.wfst(dictionary, "cat")

        def fail_capture() -> TrieSnapshot:
            raise RuntimeError("intentional capture failure")

        failing = UnicodeDictionaryResource(fail_capture)
        try:
            with self.assertRaises(duallity.NativeError) as provider:
                duallity.wfst(failing, "cat")
            self.assertIs(provider.exception.status, duallity.Status.PROVIDER_ERROR)
            self.assertIsInstance(failing.last_callback_error, RuntimeError)
        finally:
            failing.close()

    def test_configured_old_and_new_consumers_cache_and_owned_readback(self) -> None:
        snapshot = TrieSnapshot(("cat", "cot", "dog"))
        with UnicodeDictionaryResource(lambda: snapshot) as dictionary:
            with duallity.wfst(dictionary, "cat") as old:
                self.assertEqual(old.options().kind, duallity.WfstKind.LEVENSHTEIN)

            options = duallity.default_options()
            self.assertEqual(options.header.struct_size, ctypes.sizeof(options))
            self.assertEqual(options.header.record_version, 1)
            options.maximum_distance = 1
            options.cache_policy = duallity.CachePolicy.NO_CACHE
            with duallity.configured_wfst(dictionary, "cat", options) as graph:
                self.assertEqual(language(graph), {"cat": 0.0, "cot": 1.0})
                self.assertEqual(
                    graph.options().cache_policy, duallity.CachePolicy.NO_CACHE
                )
                self.assertEqual(graph.cache_statistics().resident_states, 0)
                graph.set_cache_policy(duallity.CachePolicy.LRU, 2)
                self.assertEqual(graph.options().cache_capacity, 2)
                self.assertEqual(graph.options().cache_policy, duallity.CachePolicy.LRU)
                graph.clear_cache()
                self.assertGreaterEqual(graph.cache_statistics().clears, 2)
                copied = graph.options()
            self.assertEqual(copied.maximum_distance, 1)
            with self.assertRaises(duallity.NativeError):
                graph.cache_statistics()

    def test_configured_custom_text_is_copied_and_malformed_records_fail(self) -> None:
        snapshot = TrieSnapshot(("cat",))
        with UnicodeDictionaryResource(lambda: snapshot) as dictionary:
            options = duallity.default_options()
            options.kind = duallity.WfstKind.GENERALIZED_STANDARD
            options.maximum_distance = 1
            name = (ctypes.c_uint8 * 6).from_buffer_copy(b"custom")
            operation = duallity.DuallityOperationV1()
            operation.header = duallity.DuallityRecordHeaderV1(
                ctypes.sizeof(operation), 1, 0
            )
            operation.consume_x = 1
            operation.consume_y = 1
            operation.weight = 1.0
            operation.applicability = duallity.OperationApplicability.ANY
            operation.name_data = ctypes.cast(name, ctypes.POINTER(ctypes.c_uint8))
            operation.name_len = len(name)
            operations = (duallity.DuallityOperationV1 * 1)(operation)
            options.operations = operations
            options.operation_count = 1
            options.operation_stride = ctypes.sizeof(operation)
            with duallity.configured_wfst(dictionary, "cat", options) as graph:
                name[0] = ord("X")
                copied = graph.options()
                self.assertEqual(copied.operations[0].name, "custom")
            self.assertEqual(copied.operations[0].name, "custom")

            malformed = duallity.default_options()
            malformed.header.reserved = 1
            with self.assertRaises(duallity.NativeError) as failure:
                duallity.configured_wfst(dictionary, "cat", malformed)
            self.assertIs(failure.exception.status, duallity.Status.INVALID_ARGUMENT)
            malformed = duallity.default_options()
            malformed.header.record_version = 99
            with self.assertRaises(duallity.NativeError) as failure:
                duallity.configured_wfst(dictionary, "cat", malformed)
            self.assertIs(failure.exception.status, duallity.Status.INVALID_ARGUMENT)
            malformed = duallity.default_options()
            malformed.kind = duallity.WfstKind.GENERALIZED_STANDARD
            malformed.operation_count = 1
            malformed.operation_stride = ctypes.sizeof(duallity.DuallityOperationV1)
            with self.assertRaises(duallity.NativeError) as failure:
                duallity.configured_wfst(dictionary, "cat", malformed)
            self.assertIs(failure.exception.status, duallity.Status.NULL_POINTER)
            with self.assertRaises(TypeError):
                duallity.configured_wfst(dictionary, "cat", object())  # type: ignore[arg-type]


if __name__ == "__main__":
    unittest.main()
