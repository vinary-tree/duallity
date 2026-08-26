#!/usr/bin/env python3
"""Dependency-free architectural and packaging checks for duallity's bindings.

The gate cross-checks four authorities that must never drift:

  1. the binding model            bindings/api.json
  2. the Rust C ABI               src/ffi.rs (+ the WfstKind enum in src/bindings.rs)
  3. the public C/C++ headers     include/duallity.h, include/duallity.hpp
  4. the npm facade package       bindings/javascript/**

Check groups (stable ids):

  SYM-*   symbol parity: model == `pub extern "C" fn duallity_*` == header
          declarations; the C++ header references only declared symbols.
  ENUM-*  enum and constant parity: DuallityStatus, DuallityAlgorithm,
          DuallityWfstKind values and DUALLITY_ABI_VERSION /
          DUALLITY_API_REVISION across model, Rust, and header.
  JS-*    JavaScript facade parity: export-map subpaths resolve; the
          d.ts/mjs/cjs/cljs surfaces export the same names; every
          @vinary-tree/* dependency is exact-pinned; versions agree with
          Cargo.toml and the model.
  MSRV-*  the README rustc badge (and MSRV prose) must equal Cargo.toml's
          `rust-version`.
  ID-*    identity guard: no foreign project identity strings in any
          publishable facade or package file.

Output: a human-readable report (default) or `--json`; exit 1 on any failure.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

import tomllib

ROOT = Path(__file__).resolve().parents[1]

# The retired upstream identities that must never appear in publishable files.
# Assembled from fragments so this gate does not flag itself when other repos'
# identity scans (which walk sibling `scripts/` trees) read this file.
FORBIDDEN_IDENTITIES = (
    "f1r3" + "fly",
    "universal-auto" + "mata",
    "universal_auto" + "mata",
)

SKIP_DIR_PARTS = {
    ".build",
    "_build",
    ".cpcache",
    ".gradle",
    "__pycache__",
    "bin",
    "build",
    "dist",
    "node_modules",
    "obj",
    "target",
}


class Report:
    """Collects check outcomes; renders text or JSON; carries the exit code."""

    def __init__(self) -> None:
        self.checks: list[dict[str, object]] = []

    def add(self, check_id: str, ok: bool, detail: str) -> None:
        self.checks.append({"id": check_id, "ok": ok, "detail": detail})

    @property
    def failures(self) -> list[dict[str, object]]:
        return [check for check in self.checks if not check["ok"]]

    def render_text(self) -> str:
        lines = []
        for check in self.checks:
            marker = "PASS" if check["ok"] else "FAIL"
            lines.append(f"{marker}  {check['id']:<22} {check['detail']}")
        failed = len(self.failures)
        lines.append("-" * 78)
        verdict = (
            "all bindings checks passed" if failed == 0 else "BINDINGS GATE FAILED"
        )
        lines.append(
            f"{verdict}: {len(self.checks) - failed}/{len(self.checks)} checks passed, {failed} failed"
        )
        return "\n".join(lines)

    def render_json(self) -> str:
        return json.dumps(
            {
                "ok": not self.failures,
                "passed": len(self.checks) - len(self.failures),
                "failed": len(self.failures),
                "checks": self.checks,
            },
            indent=2,
        )


def read_text(report: Report, check_id: str, path: Path) -> str | None:
    if not path.is_file():
        report.add(
            check_id, False, f"required file is missing: {path.relative_to(ROOT)}"
        )
        return None
    return path.read_text(encoding="utf-8")


def screaming_snake(name: str) -> str:
    """CamelCase Rust variant -> SCREAMING_SNAKE C enumerator (digits bind left)."""
    return re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", name).upper()


def rust_enum_values(source: str, name: str) -> dict[str, int] | None:
    match = re.search(rf"pub enum {name} \{{(.*?)\n\}}", source, re.S)
    if match is None:
        return None
    return {
        screaming_snake(variant): int(value)
        for variant, value in re.findall(r"(\w+)\s*=\s*(\d+)\s*,", match.group(1))
    }


def header_enum_values(source: str, name: str, prefix: str) -> dict[str, int] | None:
    match = re.search(rf"typedef enum {name} \{{(.*?)\}} {name};", source, re.S)
    if match is None:
        return None
    values: dict[str, int] = {}
    for enumerator, value in re.findall(r"([A-Z0-9_]+)\s*=\s*(\d+)", match.group(1)):
        if enumerator.startswith(prefix):
            values[enumerator[len(prefix) :]] = int(value)
    return values


def match_arm_values(
    source: str, function_name: str, enum_path: str
) -> dict[str, int] | None:
    match = re.search(rf"fn {function_name}\(value: u32\)(.*?)\n\}}", source, re.S)
    if match is None:
        return None
    return {
        screaming_snake(variant): int(value)
        for value, variant in re.findall(
            rf"(\d+)\s*=>\s*Ok\({enum_path}::(\w+)\)", match.group(1)
        )
    }


def compare_maps(
    report: Report,
    check_id: str,
    subject: str,
    expected: dict[str, int],
    actual: dict[str, int] | None,
    source: str,
) -> None:
    if actual is None:
        report.add(check_id, False, f"{subject}: could not parse {source}")
        return
    if actual == expected:
        report.add(
            check_id, True, f"{subject}: {len(expected)} values agree with {source}"
        )
        return
    missing = sorted(set(expected) - set(actual))
    extra = sorted(set(actual) - set(expected))
    drifted = sorted(
        key for key in set(expected) & set(actual) if expected[key] != actual[key]
    )
    report.add(
        check_id,
        False,
        f"{subject} disagrees with {source}: missing={missing} extra={extra} value-drift={drifted}",
    )


# ── SYM: C symbol parity ─────────────────────────────────────────────────────


def check_symbols(report: Report, model: dict) -> None:
    modeled = {item["name"] for item in model["cFunctions"]}
    ffi = read_text(report, "SYM-1-ffi", ROOT / "src" / "ffi.rs")
    header = read_text(report, "SYM-2-header", ROOT / "include" / "duallity.h")
    hpp = read_text(report, "SYM-3-hpp", ROOT / "include" / "duallity.hpp")
    if ffi is None or header is None or hpp is None:
        return

    exported = set(
        re.findall(
            r'pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+(duallity_[a-z0-9_]+)\s*\(', ffi
        )
    )
    if exported == modeled:
        report.add(
            "SYM-1-ffi",
            True,
            f"src/ffi.rs exports exactly the {len(modeled)} modeled symbols",
        )
    else:
        report.add(
            "SYM-1-ffi",
            False,
            f"src/ffi.rs symbol drift: missing={sorted(modeled - exported)} extra={sorted(exported - modeled)}",
        )

    declared = set(re.findall(r"\b(duallity_[a-z0-9_]+)\s*\(", header))
    if declared == modeled:
        report.add(
            "SYM-2-header",
            True,
            f"include/duallity.h declares exactly the {len(modeled)} modeled symbols",
        )
    else:
        report.add(
            "SYM-2-header",
            False,
            f"include/duallity.h drift: missing={sorted(modeled - declared)} extra={sorted(declared - modeled)}",
        )

    referenced = set(re.findall(r"\b(duallity_[a-z0-9_]+)\s*\(", hpp))
    undeclared = sorted(referenced - declared)
    if undeclared:
        report.add(
            "SYM-3-hpp",
            False,
            f"include/duallity.hpp references undeclared symbols: {undeclared}",
        )
    else:
        report.add(
            "SYM-3-hpp",
            True,
            f"include/duallity.hpp references only declared symbols ({len(referenced)} used)",
        )
    if '#include "duallity.h"' in hpp:
        report.add(
            "SYM-4-hpp-include", True, "include/duallity.hpp includes duallity.h"
        )
    else:
        report.add(
            "SYM-4-hpp-include", False, "include/duallity.hpp must include duallity.h"
        )
    for marker in (
        "#ifndef VT_INTEROP_HEADER",
        '#define VT_INTEROP_HEADER "vinary_tree_interop.h"',
        "#include VT_INTEROP_HEADER",
    ):
        if marker not in header:
            report.add(
                "SYM-5-interop",
                False,
                f"include/duallity.h is missing the overridable interop include: {marker}",
            )
            break
    else:
        report.add(
            "SYM-5-interop",
            True,
            "include/duallity.h consumes the overridable shared interop header",
        )


# ── ENUM: enum and constant parity ───────────────────────────────────────────


def check_enums(report: Report, model: dict) -> None:
    ffi = read_text(report, "ENUM-0-ffi", ROOT / "src" / "ffi.rs")
    bindings = read_text(report, "ENUM-0-bindings", ROOT / "src" / "bindings.rs")
    header = read_text(report, "ENUM-0-header", ROOT / "include" / "duallity.h")
    if ffi is None or bindings is None or header is None:
        return

    enums = model["enums"]
    status_model = {
        name: int(value) for name, value in enums["status"]["values"].items()
    }
    algorithm_model = {
        name: int(value) for name, value in enums["algorithm"]["values"].items()
    }
    kind_model = {
        name: int(value) for name, value in enums["wfstKind"]["values"].items()
    }

    compare_maps(
        report,
        "ENUM-1-status-rust",
        "DuallityStatus",
        status_model,
        rust_enum_values(ffi, "DuallityStatus"),
        "src/ffi.rs",
    )
    compare_maps(
        report,
        "ENUM-2-status-header",
        "DuallityStatus",
        status_model,
        header_enum_values(
            header, enums["status"]["cType"], enums["status"]["cPrefix"]
        ),
        "include/duallity.h",
    )
    compare_maps(
        report,
        "ENUM-3-algorithm-rust",
        "algorithm mapping",
        algorithm_model,
        match_arm_values(ffi, "algorithm", "Algorithm"),
        "src/ffi.rs fn algorithm",
    )
    compare_maps(
        report,
        "ENUM-4-algorithm-header",
        "DuallityAlgorithm",
        algorithm_model,
        header_enum_values(
            header, enums["algorithm"]["cType"], enums["algorithm"]["cPrefix"]
        ),
        "include/duallity.h",
    )
    compare_maps(
        report,
        "ENUM-5-kind-rust",
        "WfstKind",
        kind_model,
        rust_enum_values(bindings, "WfstKind"),
        "src/bindings.rs",
    )
    compare_maps(
        report,
        "ENUM-6-kind-mapping",
        "kind mapping",
        kind_model,
        match_arm_values(ffi, "kind", "WfstKind"),
        "src/ffi.rs fn kind",
    )
    compare_maps(
        report,
        "ENUM-7-kind-header",
        "DuallityWfstKind",
        kind_model,
        header_enum_values(
            header, enums["wfstKind"]["cType"], enums["wfstKind"]["cPrefix"]
        ),
        "include/duallity.h",
    )

    for check_id, constant, model_value in (
        ("ENUM-8-abi-version", "DUALLITY_ABI_VERSION", int(model["abiVersion"])),
        ("ENUM-9-api-revision", "DUALLITY_API_REVISION", int(model["apiRevision"])),
    ):
        rust_match = re.search(rf"pub const {constant}: u32 = (\d+);", ffi)
        header_match = re.search(rf"#define {constant} (\d+)u", header)
        if rust_match is None or header_match is None:
            report.add(
                check_id,
                False,
                f"{constant} is missing from src/ffi.rs or include/duallity.h",
            )
            continue
        rust_value = int(rust_match.group(1))
        header_value = int(header_match.group(1))
        if rust_value == header_value == model_value:
            report.add(
                check_id,
                True,
                f"{constant} = {model_value} in model, src/ffi.rs, and include/duallity.h",
            )
        else:
            report.add(
                check_id,
                False,
                f"{constant} drift: model={model_value} ffi.rs={rust_value} duallity.h={header_value}",
            )


# ── JS: facade parity ────────────────────────────────────────────────────────

EXACT_VERSION = re.compile(r"^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$")


def esm_export_names(source: str) -> tuple[set[str], bool]:
    names = set(re.findall(r"export\s+(?:const|function)\s+(\w+)", source))
    return names, "export default" in source


def check_javascript(report: Report, model: dict) -> None:
    js_root = ROOT / "bindings" / "javascript"
    js_model = model["javascript"]
    package_text = read_text(report, "JS-1-package", js_root / "package.json")
    if package_text is None:
        return
    package = json.loads(package_text)

    if package["name"] == js_model["package"] == model["packages"]["npm"]:
        report.add("JS-1-package", True, f"npm package name is {package['name']}")
    else:
        report.add(
            "JS-1-package",
            False,
            f"npm package name drift: package.json={package['name']} model={js_model['package']}/{model['packages']['npm']}",
        )

    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    versions = {
        "package.json": package["version"],
        "api.json packageVersion": model["packageVersion"],
        "api.json javascript.version": js_model["version"],
        "Cargo.toml": cargo["package"]["version"],
    }
    if len(set(versions.values())) == 1:
        report.add(
            "JS-2-version", True, f"one version everywhere: {package['version']}"
        )
    else:
        report.add("JS-2-version", False, f"version drift: {versions}")
    if cargo["package"]["name"] == model["packages"]["cratesIo"]:
        report.add(
            "JS-3-crate", True, f"crates.io package is {cargo['package']['name']}"
        )
    else:
        report.add(
            "JS-3-crate",
            False,
            f"crate name drift: Cargo.toml={cargo['package']['name']} model={model['packages']['cratesIo']}",
        )

    modeled_exports = set(js_model["exports"])
    actual_exports = set(package.get("exports", {}))
    if modeled_exports == actual_exports:
        report.add(
            "JS-4-export-map",
            True,
            f"export map has exactly the {len(modeled_exports)} modeled subpaths",
        )
    else:
        report.add(
            "JS-4-export-map",
            False,
            f"export-map drift: missing={sorted(modeled_exports - actual_exports)} extra={sorted(actual_exports - modeled_exports)}",
        )

    unresolved: list[str] = []
    for subpath, target in package.get("exports", {}).items():
        candidates = [target] if isinstance(target, str) else list(target.values())
        for candidate in candidates:
            if not (js_root / candidate).is_file():
                unresolved.append(f"{subpath} -> {candidate}")
    if unresolved:
        report.add(
            "JS-5-export-files", False, f"export targets do not resolve: {unresolved}"
        )
    else:
        report.add(
            "JS-5-export-files", True, "every export-map target resolves to a file"
        )

    types_target = package.get("types")
    if types_target and (js_root / types_target).is_file():
        report.add("JS-6-types", True, f"types entry resolves: {types_target}")
    else:
        report.add(
            "JS-6-types", False, f"types entry missing or unresolved: {types_target!r}"
        )

    loose = {
        name: version
        for name, version in package.get("dependencies", {}).items()
        if name.startswith("@vinary-tree/") and not EXACT_VERSION.match(version)
    }
    if loose:
        report.add(
            "JS-7-exact-pins",
            False,
            f"@vinary-tree/* dependencies must be exact-pinned: {loose}",
        )
    else:
        report.add(
            "JS-7-exact-pins", True, "every @vinary-tree/* dependency is exact-pinned"
        )
    if package.get("dependencies", {}) == js_model["dependencies"]:
        report.add(
            "JS-8-dep-model",
            True,
            f"dependency pins match the model: {js_model['dependencies']}",
        )
    else:
        report.add(
            "JS-8-dep-model",
            False,
            f"dependency drift: package.json={package.get('dependencies')} model={js_model['dependencies']}",
        )

    facade_names = set(js_model["facadeExports"])
    dts = read_text(report, "JS-9-dts", js_root / "index.d.ts")
    if dts is not None:
        dts_names, dts_default = esm_export_names(dts)
        if facade_names <= dts_names and dts_default:
            report.add(
                "JS-9-dts",
                True,
                f"index.d.ts exports {sorted(facade_names)} and a default",
            )
        else:
            report.add(
                "JS-9-dts",
                False,
                f"index.d.ts drift: exports={sorted(dts_names)} default={dts_default} expected>={sorted(facade_names)}",
            )

    native_names: set[str] = set()
    for index, facade in enumerate(("native.mjs", "wasm.mjs", "wasi.mjs"), start=10):
        check_id = f"JS-{index}-{facade.split('.', maxsplit=1)[0]}"
        source = read_text(report, check_id, js_root / "facades" / facade)
        if source is None:
            continue
        names, has_default = esm_export_names(source)
        native_names = names if facade == "native.mjs" else native_names
        problems = []
        if not facade_names <= names:
            problems.append(f"exports={sorted(names)} expected>={sorted(facade_names)}")
        if not has_default:
            problems.append("missing default export")
        if "assertSameRuntime" not in source:
            problems.append("missing runtime identity guard")
        if "assertDictionaryResource" not in source:
            problems.append("missing dictionary interface guard")
        expected_runtime = {
            "native.mjs": model["wasm"]["runtimePackage"],
            "wasm.mjs": model["wasm"]["runtimePackage"] + "/wasm",
            "wasi.mjs": model["wasm"]["runtimePackage"] + "/wasi",
        }[facade]
        if f'from "{expected_runtime}"' not in source:
            problems.append(f"does not import the shared runtime {expected_runtime}")
        if problems:
            report.add(check_id, False, f"facades/{facade}: {'; '.join(problems)}")
        else:
            report.add(
                check_id,
                True,
                f"facades/{facade} exports {sorted(names)} over {expected_runtime}",
            )

    for index, facade in enumerate(("typescript.mjs", "clojurescript.mjs"), start=13):
        check_id = f"JS-{index}-{facade.split('.', maxsplit=1)[0]}"
        source = read_text(report, check_id, js_root / "facades" / facade)
        if source is None:
            continue
        if (
            'export * from "./native.mjs"' in source
            and 'export { default } from "./native.mjs"' in source
        ):
            report.add(
                check_id, True, f"facades/{facade} re-exports the native surface"
            )
        else:
            names, has_default = esm_export_names(source)
            if facade_names <= names and has_default:
                report.add(check_id, True, f"facades/{facade} exports {sorted(names)}")
            else:
                report.add(
                    check_id,
                    False,
                    f"facades/{facade} neither re-exports native.mjs nor exports {sorted(facade_names)}",
                )

    for index, facade in enumerate(
        ("native.cjs", "typescript.cjs", "clojurescript.cjs"), start=15
    ):
        check_id = f"JS-{index}-{facade.split('.', maxsplit=1)[0]}-cjs"
        source = read_text(report, check_id, js_root / "facades" / facade)
        if source is None:
            continue
        if re.search(r'module\.exports\s*=\s*require\("\./native\.cjs"\)', source):
            report.add(check_id, True, f"facades/{facade} re-exports native.cjs")
            continue
        literal = re.search(r"module\.exports\s*=\s*\{([^}]*)\}", source)
        exported = set(re.findall(r"[\w$]+", literal.group(1))) if literal else set()
        if literal and facade_names <= exported and "default" in exported:
            report.add(
                check_id,
                True,
                f"facades/{facade} exports {sorted(facade_names)} and default",
            )
        else:
            report.add(
                check_id,
                False,
                f"facades/{facade} module.exports lacks {sorted(facade_names | {'default'})}",
            )

    cljs_relative = js_model["exports"]["./cljs/vinary_tree/duallity.cljs"]["file"]
    cljs = read_text(report, "JS-18-cljs", js_root / cljs_relative)
    if cljs is not None:
        problems = []
        namespace = js_model["cljsNamespace"]
        if f"(ns {namespace}" not in cljs:
            problems.append(f"namespace is not {namespace}")
        for function_name in js_model["cljsFunctions"]:
            if f"(defn {function_name}" not in cljs:
                problems.append(f"missing (defn {function_name}")
        if problems:
            report.add("JS-18-cljs", False, f"{cljs_relative}: {'; '.join(problems)}")
        else:
            report.add(
                "JS-18-cljs",
                True,
                f"{cljs_relative} defines {js_model['cljsFunctions']} in {namespace}",
            )
        cljs_map = package.get("cljs", {}).get("namespaces", {})
        if cljs_map.get(namespace) == "./" + cljs_relative:
            report.add(
                "JS-19-cljs-map",
                True,
                f"package.json cljs namespace map pins {namespace}",
            )
        else:
            report.add(
                "JS-19-cljs-map",
                False,
                f"package.json cljs namespace map drift: {cljs_map}",
            )

    deps_cljs = read_text(report, "JS-20-deps-cljs", js_root / "deps.cljs")
    if deps_cljs is not None:
        pin = re.search(
            rf'"{re.escape(js_model["package"])}"\s+"([0-9][0-9A-Za-z.+-]*)"', deps_cljs
        )
        if pin and pin.group(1) == model["packageVersion"]:
            report.add(
                "JS-20-deps-cljs",
                True,
                f"deps.cljs pins {js_model['package']} {pin.group(1)}",
            )
        else:
            report.add(
                "JS-20-deps-cljs",
                False,
                f"deps.cljs must pin {js_model['package']} to {model['packageVersion']} (found {pin.group(1) if pin else 'nothing'})",
            )

    published = set(package.get("files", []))
    required_published = {"index.d.ts", "facades/", "cljs/", "deps.cljs"}
    if required_published <= published:
        report.add(
            "JS-21-files",
            True,
            f"package.json files covers {sorted(required_published)}",
        )
    else:
        report.add(
            "JS-21-files",
            False,
            f"package.json files is missing {sorted(required_published - published)}",
        )


# ── MSRV: badge guard ────────────────────────────────────────────────────────


def check_msrv(report: Report) -> None:
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    rust_version = cargo["package"].get("rust-version")
    if rust_version is None:
        report.add(
            "MSRV-1-badge", False, "Cargo.toml lacks a rust-version to compare against"
        )
        return
    readme = read_text(report, "MSRV-1-badge", ROOT / "README.md")
    if readme is None:
        return
    badge = re.search(r"img\.shields\.io/badge/rustc-([0-9.]+)%2B", readme)
    if badge is None:
        report.add("MSRV-1-badge", False, "README.md has no shields.io rustc badge")
    elif badge.group(1) == rust_version:
        report.add(
            "MSRV-1-badge",
            True,
            f"README rustc badge {badge.group(1)}+ matches rust-version {rust_version}",
        )
    else:
        report.add(
            "MSRV-1-badge",
            False,
            f"README rustc badge says {badge.group(1)}+ but Cargo.toml rust-version is {rust_version}",
        )
    for prose in re.finditer(
        r"Minimum supported Rust version: \*\*([0-9.]+)\*\*", readme
    ):
        if prose.group(1) != rust_version:
            report.add(
                "MSRV-2-prose",
                False,
                f"README MSRV prose says {prose.group(1)} but Cargo.toml rust-version is {rust_version}",
            )
            break
    else:
        report.add(
            "MSRV-2-prose",
            True,
            f"README MSRV prose agrees with rust-version {rust_version}",
        )


# ── ID: identity guard ───────────────────────────────────────────────────────


def check_identity(report: Report) -> None:
    roots = [
        ROOT / "bindings",
        ROOT / "include",
        ROOT / "src",
        ROOT / "Cargo.toml",
        ROOT / "README.md",
        ROOT / "scripts" / "stage-native-package.sh",
    ]
    files: list[Path] = []
    for root in roots:
        if root.is_file():
            files.append(root)
        elif root.is_dir():
            files.extend(path for path in root.rglob("*") if path.is_file())
    offenders: list[str] = []
    scanned = 0
    for path in files:
        if any(part in SKIP_DIR_PARTS for part in path.parts):
            continue
        scanned += 1
        source = path.read_text(encoding="utf-8", errors="ignore").lower()
        for forbidden in FORBIDDEN_IDENTITIES:
            if forbidden in source:
                offenders.append(f"{path.relative_to(ROOT)}: {forbidden}")
    if offenders:
        report.add(
            "ID-1-identity",
            False,
            f"foreign identity strings in publishable files: {offenders}",
        )
    else:
        report.add(
            "ID-1-identity",
            True,
            f"no foreign identity strings across {scanned} publishable files",
        )


# ── main ─────────────────────────────────────────────────────────────────────


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("--json", action="store_true", help="emit the report as JSON")
    arguments = parser.parse_args()

    report = Report()
    model_path = ROOT / "bindings" / "api.json"
    if not model_path.is_file():
        report.add("MODEL-0", False, "bindings/api.json is missing")
    else:
        model = json.loads(model_path.read_text(encoding="utf-8"))
        report.add(
            "MODEL-0",
            True,
            f"loaded binding model for {model['name']} {model['packageVersion']}",
        )
        check_symbols(report, model)
        check_enums(report, model)
        check_javascript(report, model)
    check_msrv(report)
    check_identity(report)

    print(report.render_json() if arguments.json else report.render_text())
    return 1 if report.failures else 0


if __name__ == "__main__":
    sys.exit(main())
