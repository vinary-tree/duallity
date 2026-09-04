#!/usr/bin/env python3
"""Dependency-free architectural and packaging checks for duallity's bindings.

The gate cross-checks five authorities that must never drift:

  1. the binding model            bindings/api.json
  2. the Rust C ABI               src/ffi.rs (+ the WfstKind enum in src/bindings.rs)
  3. the public C/C++ headers     include/duallity.h, include/duallity.hpp
  4. the npm facade package       bindings/javascript/**
  5. the PyPI facade package      bindings/python/**

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
  JR-*    Julia/Raku package identity, version, generated ABI enum/constant,
          and native-symbol parity against the same binding model.
  PY-*    Python package identity, version, ABI/API/enum/symbol parity,
          zero-copy resource handoff, and platform-wheel contents.
  MSRV-*  the README rustc badge (and MSRV prose) must equal Cargo.toml's
          `rust-version`.
  ID-*    identity guard: no foreign project identity strings in any
          publishable facade or package file.

Output: a human-readable report (default) or `--json`; exit 1 on any failure.
"""

from __future__ import annotations

import argparse
import ast
import json
import re
import sys
from pathlib import Path

if sys.version_info >= (3, 11):
    import tomllib
else:
    import tomli as tomllib

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
    ".precomp",
    "__pycache__",
    "bin",
    "build",
    "dist",
    "node_modules",
    "obj",
    "target",
}

JULIA_ROOT = ROOT / "bindings" / "julia" / "Duallity"
RAKU_ROOT = ROOT / "bindings" / "raku"
PYTHON_ROOT = ROOT / "bindings" / "python"


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
    match = re.search(rf"pub enum {name} \{{(.*?)\n\}}", source, re.DOTALL)
    if match is None:
        return None
    return {
        screaming_snake(variant): int(value)
        for variant, value in re.findall(r"(\w+)\s*=\s*(\d+)\s*,", match.group(1))
    }


def header_enum_values(source: str, name: str, prefix: str) -> dict[str, int] | None:
    match = re.search(rf"typedef enum {name} \{{(.*?)\}} {name};", source, re.DOTALL)
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
    match = re.search(rf"fn {function_name}\(value: u32\)(.*?)\n\}}", source, re.DOTALL)
    if match is None:
        return None
    return {
        screaming_snake(variant): int(value)
        for value, variant in re.findall(
            rf"(\d+)\s*=>\s*Ok\({enum_path}::(\w+)\)", match.group(1)
        )
    }


def python_class_constants(source: str, class_name: str) -> dict[str, int] | None:
    """Read integer assignments from one Python enum without importing it."""
    tree = ast.parse(source)
    for node in tree.body:
        if isinstance(node, ast.ClassDef) and node.name == class_name:
            values: dict[str, int] = {}
            for statement in node.body:
                if (
                    isinstance(statement, ast.Assign)
                    and len(statement.targets) == 1
                    and isinstance(statement.targets[0], ast.Name)
                    and isinstance(statement.value, ast.Constant)
                    and isinstance(statement.value.value, int)
                ):
                    values[statement.targets[0].id] = statement.value.value
            return values
    return None


def python_literal_assignment(source: str, name: str) -> object | None:
    """Read one module-level literal assignment without executing package code."""
    tree = ast.parse(source)
    for node in tree.body:
        if isinstance(node, (ast.Assign, ast.AnnAssign)) and isinstance(
            node.value, (ast.Constant, ast.List, ast.Tuple, ast.Dict)
        ):
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            if any(
                isinstance(target, ast.Name) and target.id == name for target in targets
            ):
                return ast.literal_eval(node.value)
    return None


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


# ── JR: Julia and Raku parity ───────────────────────────────────────────────


def check_julia_raku(report: Report, model: dict) -> None:
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    version = cargo["package"]["version"]
    julia_project_text = read_text(
        report, "JR-1-julia-project", JULIA_ROOT / "Project.toml"
    )
    raku_meta_text = read_text(report, "JR-2-raku-meta", RAKU_ROOT / "META6.json")
    julia_generated = read_text(
        report, "JR-3-julia-generated", JULIA_ROOT / "src" / "GeneratedAbi.jl"
    )
    raku_generated = read_text(
        report,
        "JR-4-raku-generated",
        RAKU_ROOT / "lib" / "Duallity" / "GeneratedAbi.rakumod",
    )
    julia_facade = read_text(
        report, "JR-5-julia-symbols", JULIA_ROOT / "src" / "Duallity.jl"
    )
    raku_facade = read_text(
        report, "JR-6-raku-symbols", RAKU_ROOT / "lib" / "Duallity.rakumod"
    )
    if any(
        value is None
        for value in (
            julia_project_text,
            raku_meta_text,
            julia_generated,
            raku_generated,
            julia_facade,
            raku_facade,
        )
    ):
        return

    julia_project = tomllib.loads(julia_project_text)
    raku_meta = json.loads(raku_meta_text)
    if (
        julia_project.get("name") == model["packages"]["julia"]
        and julia_project.get("version") == version
    ):
        report.add(
            "JR-1-julia-project",
            True,
            f"Julia package {julia_project['name']} is version {version}",
        )
    else:
        report.add(
            "JR-1-julia-project",
            False,
            f"Julia identity/version drift: {julia_project.get('name')} {julia_project.get('version')}",
        )
    raku_family_version = version.replace("-rc.", ".rc.")
    raku_dependencies = {
        f"Vinary-Tree-Interop:ver<{raku_family_version}>:auth<zef:vinary-tree>"
    }
    raku_test_dependencies = {
        f"Libdictenstein:ver<{raku_family_version}>:auth<zef:vinary-tree>",
        f"Lling-Llang:ver<{raku_family_version}>:auth<zef:vinary-tree>",
        "Test",
    }
    if (
        raku_meta.get("name") == model["packages"]["zef"]
        and raku_meta.get("version") == version
        and set(raku_meta.get("depends", [])) == raku_dependencies
        and set(raku_meta.get("test-depends", [])) == raku_test_dependencies
    ):
        report.add(
            "JR-2-raku-meta",
            True,
            f"Raku package {raku_meta['name']} and coordinated pins are {version}",
        )
    else:
        report.add(
            "JR-2-raku-meta",
            False,
            "Raku identity, version, or coordinated dependency pins drifted: "
            f"{raku_meta.get('name')} {raku_meta.get('version')}",
        )

    abi = int(model["abiVersion"])
    api = int(model["apiRevision"])
    julia_constants = (
        f"const ABI_VERSION = UInt32({abi})" in julia_generated
        and f"const API_REVISION = UInt32({api})" in julia_generated
    )
    raku_constants = (
        f"our constant ABI-VERSION is export = {abi};" in raku_generated
        and f"our constant API-REVISION is export = {api};" in raku_generated
    )
    report.add(
        "JR-3-julia-generated",
        julia_constants,
        f"Julia generated ABI/API constants {'agree' if julia_constants else 'DRIFT'} ({abi}/{api})",
    )
    report.add(
        "JR-4-raku-generated",
        raku_constants,
        f"Raku generated ABI/API constants {'agree' if raku_constants else 'DRIFT'} ({abi}/{api})",
    )

    modeled = {item["name"] for item in model["cFunctions"]}
    julia_symbols = set(re.findall(r"native\(:(duallity_[a-z0-9_]+)\)", julia_facade))
    raku_symbols = set(re.findall(r"symbol\('(duallity_[a-z0-9_]+)'\)", raku_facade))
    julia_required = {
        "duallity_abi_version",
        "duallity_api_revision",
        "duallity_last_error_message",
        "duallity_wfst_new",
        "duallity_wfst_free",
        "duallity_wfst_resource",
    }
    raku_required = (julia_required - {"duallity_wfst_new"}) | {"duallity_wfst_new_ref"}
    julia_ok = julia_symbols == julia_required and julia_symbols <= modeled
    raku_ok = raku_symbols == raku_required and raku_symbols <= modeled
    report.add(
        "JR-5-julia-symbols",
        julia_ok,
        f"Julia native symbol set {'agrees' if julia_ok else 'DRIFT'}: {sorted(julia_symbols)}",
    )
    report.add(
        "JR-6-raku-symbols",
        raku_ok,
        f"Raku native symbol set {'agrees' if raku_ok else 'DRIFT'}: {sorted(raku_symbols)}",
    )

    for check_id, enum_name, values in (
        ("JR-7-status-enums", "status", model["enums"]["status"]["values"]),
        ("JR-8-algorithm-enums", "algorithm", model["enums"]["algorithm"]["values"]),
        ("JR-9-kind-enums", "wfstKind", model["enums"]["wfstKind"]["values"]),
    ):
        missing: list[str] = []
        for name, value in values.items():
            julia_name = {
                "status": f"STATUS_{name}",
                "algorithm": f"ALGORITHM_{name}",
                "wfstKind": f"WFST_{name}",
            }[enum_name]
            raku_name = name.replace("_", "-")
            if not re.search(
                rf"\b{re.escape(julia_name)}\s*=\s*{value}\b", julia_generated
            ):
                missing.append(f"Julia:{julia_name}={value}")
            if not re.search(
                rf"\b{re.escape(raku_name)}\s*=>\s*{value}\b", raku_generated
            ):
                missing.append(f"Raku:{raku_name}={value}")
        report.add(
            check_id,
            not missing,
            f"{len(values)} {enum_name} values agree"
            if not missing
            else f"enum drift: {missing}",
        )


# ── PY: Python facade and wheel parity ──────────────────────────────────────


def check_python(report: Report, model: dict) -> None:
    pyproject_text = read_text(report, "PY-1-project", PYTHON_ROOT / "pyproject.toml")
    abi_source = read_text(
        report, "PY-2-abi", PYTHON_ROOT / "src" / "duallity" / "_abi.py"
    )
    facade_source = read_text(
        report, "PY-3-facade", PYTHON_ROOT / "src" / "duallity" / "__init__.py"
    )
    setup_source = read_text(report, "PY-4-wheel", PYTHON_ROOT / "setup.py")
    manifest_source = read_text(report, "PY-4-wheel", PYTHON_ROOT / "MANIFEST.in")
    typed = PYTHON_ROOT / "src" / "duallity" / "py.typed"
    if any(
        value is None
        for value in (
            pyproject_text,
            abi_source,
            facade_source,
            setup_source,
            manifest_source,
        )
    ):
        return
    assert pyproject_text is not None
    assert abi_source is not None
    assert facade_source is not None
    assert setup_source is not None
    assert manifest_source is not None

    project = tomllib.loads(pyproject_text)["project"]
    python_model = model["python"]
    expected_dependency = (
        f"vinary-tree-interop=={python_model['dependencies']['vinary-tree-interop']}"
    )
    project_ok = (
        project.get("name") == model["packages"]["pypi"] == python_model["package"]
        and project.get("version") == python_model["version"]
        and project.get("requires-python") == python_model["requiresPython"]
        and project.get("dependencies") == [expected_dependency]
    )
    report.add(
        "PY-1-project",
        project_ok,
        (
            f"PyPI package {project.get('name')} {project.get('version')} has exact interop pin"
            if project_ok
            else "Python project identity, version, interpreter range, or dependency pin drifted"
        ),
    )

    constants_ok = (
        python_literal_assignment(abi_source, "ABI_VERSION") == model["abiVersion"]
        and python_literal_assignment(abi_source, "API_REVISION")
        == model["apiRevision"]
        and python_literal_assignment(facade_source, "__version__")
        == python_model["version"]
    )
    report.add(
        "PY-2-constants",
        constants_ok,
        f"Python ABI/API/package constants {'agree' if constants_ok else 'DRIFT'}",
    )

    for check_id, class_name, key in (
        ("PY-3-status-enum", "Status", "status"),
        ("PY-4-algorithm-enum", "Algorithm", "algorithm"),
        ("PY-5-kind-enum", "WfstKind", "wfstKind"),
    ):
        compare_maps(
            report,
            check_id,
            f"Python {class_name}",
            {name: int(value) for name, value in model["enums"][key]["values"].items()},
            python_class_constants(abi_source, class_name),
            "bindings/python/src/duallity/_abi.py",
        )

    modeled = {item["name"] for item in model["cFunctions"]}
    symbols = set(re.findall(r'_bind\(\s*"(duallity_[a-z0-9_]+)"', abi_source))
    required = {
        "duallity_abi_version",
        "duallity_api_revision",
        "duallity_last_error_message",
        "duallity_wfst_new_ref",
        "duallity_wfst_free",
        "duallity_wfst_resource",
    }
    symbols_ok = symbols == required and symbols <= modeled
    report.add(
        "PY-6-symbols",
        symbols_ok,
        f"Python native symbol set {'agrees' if symbols_ok else 'DRIFT'}: {sorted(symbols)}",
    )

    exports = python_literal_assignment(facade_source, "__all__")
    modeled_exports = set(python_model["facadeExports"])
    actual_exports = set(exports) if isinstance(exports, list) else set()
    exports_ok = modeled_exports <= actual_exports
    report.add(
        "PY-7-exports",
        exports_ok,
        (
            f"Python facade exports every modeled name ({len(modeled_exports)})"
            if exports_ok
            else f"Python facade lacks {sorted(modeled_exports - actual_exports)}"
        ),
    )

    wheel_markers = (
        "DUALLITY_PREBUILT_LIBRARY",
        '"python-bindings"',
        'return "py3", "none", platform_tag',
        'shutil.copy2(REPOSITORY_ROOT / "LICENSE"',
    )
    wheel_ok = (
        all(marker in setup_source for marker in wheel_markers)
        and typed.is_file()
        and project.get("license-files") == ["LICENSE"]
        and (PYTHON_ROOT / "LICENSE").read_bytes() == (ROOT / "LICENSE").read_bytes()
    )
    report.add(
        "PY-8-wheel",
        wheel_ok,
        (
            "wheel embeds the native library, license, and py.typed marker"
            if wheel_ok
            else "wheel staging, native build feature, license, or py.typed marker is missing"
        ),
    )

    facade_ok = (
        "class Wfst(ScalarWfst):" in facade_source
        and "duallity_wfst_new_ref" in facade_source
        and "return Wfst.adopt(resource)" in facade_source
        and "DUALLITY_LIBRARY" in abi_source
    )
    report.add(
        "PY-9-resource-handoff",
        facade_ok,
        (
            "Python uses pointer-form construction and zero-copy ScalarWfst adoption"
            if facade_ok
            else "Python resource construction/handoff contract is incomplete"
        ),
    )

    manifest_entries = set(manifest_source.splitlines())
    required_manifest_entries = {
        "include LICENSE",
        "include _registry_manifest.py",
        "include pyrightconfig.json",
        "recursive-include benchmark *.py",
        "recursive-include examples *.py",
        "recursive-include tests *.py",
    }
    sdist_markers = (
        "class SelfContainedSourceDistribution(sdist):",
        'destination / "Cargo.toml"',
        "registry_text(",
        'shutil.copytree(REPOSITORY_ROOT / "src"',
        '"sdist": SelfContainedSourceDistribution',
    )
    sdist_ok = required_manifest_entries <= manifest_entries and all(
        marker in setup_source for marker in sdist_markers
    )
    report.add(
        "PY-10-sdist",
        sdist_ok,
        (
            "source distribution carries validated registry Rust source, tests, examples, and benchmark"
            if sdist_ok
            else "source-distribution Rust source or Python evidence inventory is incomplete"
        ),
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
        if any(part in SKIP_DIR_PARTS for part in path.relative_to(ROOT).parts):
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
        check_julia_raku(report, model)
        check_python(report, model)
    check_msrv(report)
    check_identity(report)

    print(report.render_json() if arguments.json else report.render_text())
    return 1 if report.failures else 0


if __name__ == "__main__":
    sys.exit(main())
