"""Installed-wheel smoke test with no optional family facade dependencies."""

from __future__ import annotations

import unittest

import duallity
from vinary_tree_interop import UnicodeDictionaryResource, UnitDomain


class OneTerm:
    """Immutable trie snapshot containing exactly `cat`."""

    _edges = ((("c", 1),), (("a", 2),), (("t", 3),), ())

    def root(self) -> int:
        return 0

    def __len__(self) -> int:
        return 1

    def is_final(self, node: int) -> bool:
        return node == 3

    def value(self, node: int) -> int | None:
        del node
        return None

    def edges(self, node: int) -> tuple[tuple[str, int], ...]:
        return self._edges[node]


class WheelTests(unittest.TestCase):
    def test_embedded_library_constructs_and_traverses(self) -> None:
        snapshot = OneTerm()
        with UnicodeDictionaryResource(lambda: snapshot) as dictionary:
            graph = duallity.wfst(dictionary, "cat", maximum_distance=1)
        with graph:
            self.assertIs(graph.unit_domain, UnitDomain.UNICODE_SCALAR)
            self.assertIsNotNone(graph.state_info(graph.start))
            self.assertTrue(graph.arcs(graph.start))


if __name__ == "__main__":
    unittest.main()
