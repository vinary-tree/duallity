#!/usr/bin/env python3
"""Convert duallity's validated development paths to registry dependencies.

The reviewed manifest keeps exact-version sibling paths so a coordinated source
graph is tested locally. Isolated wheel builders cannot mount directories above
their project root. This tool verifies every expected path and exact version,
then removes only those path keys in an ephemeral release checkout. Cargo still
resolves the same package identities and versions from crates.io.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

if sys.version_info >= (3, 11):
    import tomllib
else:
    import tomli as tomllib

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "Cargo.toml"
sys.path.insert(0, str(ROOT / "bindings" / "python"))
from _registry_manifest import registry_text, validate_manifest


def self_test() -> None:
    """Exercise exact rewriting and fail-closed behavior without filesystem I/O."""
    source = """[package]
version = "4.0.0-rc.6"
[dependencies]
liblevenshtein = { path = "../liblevenshtein-rust", version = "=4.0.0-rc.6" }
lling-llang = { path = "../lling-llang", version = "=4.0.0-rc.6", default-features = false }
libdictenstein = { path = "../libdictenstein", version = "=4.0.0-rc.6" }
vinary-tree-interop = { path = "../vinary-tree-interop", version = "=4.0.0-rc.6", optional = true }
[dev-dependencies]
libdictenstein = { path = "../libdictenstein", version = "=4.0.0-rc.6", features = ["bindings-core"] }
"""
    result = registry_text(source)
    assert "path =" not in result
    assert 'version = "=4.0.0-rc.6"' in result
    assert registry_text(result) == result
    try:
        registry_text(source.replace('version = "=4.0.0-rc.6"', 'version = "4"', 1))
    except ValueError:
        pass
    else:
        raise AssertionError("inexact family version was accepted")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write",
        action="store_true",
        help="replace Cargo.toml with its validated registry-only form",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="exercise the transformation and its fail-closed validation",
    )
    arguments = parser.parse_args()

    if arguments.self_test:
        self_test()
        print("registry-manifest self-test passed")
    if arguments.write:
        source = MANIFEST.read_text(encoding="utf-8")
        result = registry_text(source)
        temporary = MANIFEST.with_suffix(".toml.registry-preparation")
        temporary.write_text(result, encoding="utf-8")
        temporary.replace(MANIFEST)
        print("prepared Cargo.toml for exact crates.io dependency resolution")
    if not arguments.self_test and not arguments.write:
        failures = validate_manifest(
            tomllib.loads(MANIFEST.read_text(encoding="utf-8")), expect_paths=True
        )
        if failures:
            raise SystemExit("\n".join(failures))
        print("development Cargo.toml has exact coordinated sibling paths")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
