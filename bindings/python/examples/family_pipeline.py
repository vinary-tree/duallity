"""Executable libdictenstein → duallity → lling-llang composition example."""

from __future__ import annotations

from contextlib import ExitStack

import duallity
import libdictenstein
import lling_llang


def main() -> None:
    with ExitStack() as stack:
        dictionary = stack.enter_context(libdictenstein.DynamicDawg())
        dictionary.update_many((("cat", None), ("cot", None), ("dog", None)))
        fuzzy = stack.enter_context(
            duallity.wfst(dictionary, "cat", maximum_distance=1)
        )

        with lling_llang.WfstBuilder(size_hint=1) as builder:
            state = builder.add_state()
            builder.set_start(state).set_final(state)
            for character in "acot":
                builder.add_arc(state, character, character.upper(), state)
            uppercase = builder.build()
        stack.enter_context(uppercase)
        composed = stack.enter_context(lling_llang.compose(fuzzy, uppercase))

        print("start:", composed.start)
        print("first arcs:", composed.arcs(composed.start))


if __name__ == "__main__":
    main()
