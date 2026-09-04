"""Validated Cargo-manifest normalization shared by wheel and sdist builds."""

from __future__ import annotations

import re
import sys

if sys.version_info >= (3, 11):
    import tomllib
else:
    import tomli as tomllib

EXPECTED: dict[str, dict[str, str]] = {
    "dependencies": {
        "liblevenshtein": "../liblevenshtein-rust",
        "lling-llang": "../lling-llang",
        "libdictenstein": "../libdictenstein",
        "vinary-tree-interop": "../vinary-tree-interop",
    },
    "dev-dependencies": {
        "libdictenstein": "../libdictenstein",
    },
}


def validate_manifest(document: dict[str, object], *, expect_paths: bool) -> list[str]:
    """Return all violations of the exact family dependency contract."""
    failures: list[str] = []
    package = document.get("package")
    if not isinstance(package, dict) or not isinstance(package.get("version"), str):
        return ["package.version is missing"]
    version = package["version"]
    for section_name, expected_paths in EXPECTED.items():
        section = document.get(section_name)
        if not isinstance(section, dict):
            failures.append(f"[{section_name}] is missing")
            continue
        for dependency, expected_path in expected_paths.items():
            declaration = section.get(dependency)
            if not isinstance(declaration, dict):
                failures.append(f"{section_name}.{dependency} is not an inline table")
                continue
            if declaration.get("version") != f"={version}":
                failures.append(
                    f"{section_name}.{dependency}.version must be ={version}"
                )
            actual_path = declaration.get("path")
            if expect_paths and actual_path != expected_path:
                failures.append(
                    f"{section_name}.{dependency}.path must be {expected_path}"
                )
            if not expect_paths and actual_path is not None:
                failures.append(
                    f"{section_name}.{dependency}.path remained in registry manifest"
                )
    return failures


def registry_text(source: str) -> str:
    """Return an exact-version registry manifest after fail-closed validation."""
    document = tomllib.loads(source)
    path_failures = validate_manifest(document, expect_paths=True)
    if path_failures:
        registry_failures = validate_manifest(document, expect_paths=False)
        if not registry_failures:
            return source
        raise ValueError(
            "manifest is neither the coordinated-path nor registry form:\n"
            + "\n".join(path_failures)
        )

    current_section = ""
    rewritten: list[str] = []
    changed: set[tuple[str, str]] = set()
    section_pattern = re.compile(r"^\[([^]]+)]\s*$")
    path_pattern = re.compile(r'path\s*=\s*"[^"]+"\s*,\s*')
    for line in source.splitlines(keepends=True):
        if section_match := section_pattern.match(line.strip()):
            current_section = section_match.group(1)
        if current_section in EXPECTED:
            for dependency in EXPECTED[current_section]:
                if re.match(rf"^{re.escape(dependency)}\s*=\s*\{{", line):
                    updated, count = path_pattern.subn("", line, count=1)
                    if count != 1:
                        raise ValueError(
                            f"{current_section}.{dependency} has no removable path key"
                        )
                    line = updated
                    changed.add((current_section, dependency))
                    break
        rewritten.append(line)

    expected = {
        (section, dependency)
        for section, dependencies in EXPECTED.items()
        for dependency in dependencies
    }
    if changed != expected:
        missing = sorted(expected - changed)
        raise ValueError(f"dependency declarations were not rewritten: {missing}")

    result = "".join(rewritten)
    failures = validate_manifest(tomllib.loads(result), expect_paths=False)
    if failures:
        raise ValueError("\n".join(failures))
    return result
