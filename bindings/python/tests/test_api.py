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


if __name__ == "__main__":
    unittest.main()
