"""Measure Python-boundary construction and first-state expansion costs."""

from __future__ import annotations

import argparse
import json
import statistics
import time

import duallity
import libdictenstein


def sample(iterations: int, terms: int, warmups: int) -> tuple[list[int], list[int]]:
    construction: list[int] = []
    expansion: list[int] = []
    with libdictenstein.DynamicDawg() as dictionary:
        dictionary.update_many((f"term-{index:08d}", None) for index in range(terms))
        for index in range(warmups + iterations):
            start = time.perf_counter_ns()
            graph = duallity.wfst(dictionary, "term-00000042", maximum_distance=2)
            constructed = time.perf_counter_ns()
            graph.arcs(graph.start)
            expanded = time.perf_counter_ns()
            graph.close()
            if index >= warmups:
                construction.append(constructed - start)
                expansion.append(expanded - constructed)
    return construction, expansion


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iterations", type=int, default=100)
    parser.add_argument("--terms", type=int, default=10_000)
    parser.add_argument("--warmups", type=int, default=10)
    parser.add_argument(
        "--json",
        action="store_true",
        help="emit raw nanosecond samples and summary statistics as JSON",
    )
    arguments = parser.parse_args()
    if arguments.iterations <= 0 or arguments.terms <= 0 or arguments.warmups < 0:
        parser.error(
            "iterations and terms must be positive; warmups cannot be negative"
        )
    construction, expansion = sample(
        arguments.iterations, arguments.terms, arguments.warmups
    )
    result = {
        "iterations": arguments.iterations,
        "terms": arguments.terms,
        "warmups": arguments.warmups,
        "construction_ns": construction,
        "first_expansion_ns": expansion,
        "construction_median_ns": round(statistics.median(construction)),
        "first_expansion_median_ns": round(statistics.median(expansion)),
    }
    if arguments.json:
        print(json.dumps(result, indent=2))
    else:
        print(
            "construction median ns:",
            result["construction_median_ns"],
            "first expansion median ns:",
            result["first_expansion_median_ns"],
        )


if __name__ == "__main__":
    main()
