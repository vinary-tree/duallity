#!/usr/bin/env python3
"""Generate and verify duallity's Raku NativeCall ABI.

``bindings/api.json`` owns language-neutral signatures, versions, ownership,
and nullability. ``include/duallity.h`` remains the public C declaration that
the generator verifies before it emits Raku. This makes a model/header drift a
hard failure instead of silently generating a facade for the wrong ABI.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import NoReturn

ROOT = Path(__file__).resolve().parents[1]
MODEL_PATH = ROOT / "bindings" / "api.json"
HEADER_PATH = ROOT / "include" / "duallity.h"
OUTPUT_PATH = ROOT / "bindings" / "raku" / "lib" / "Duallity" / "GeneratedAbi.rakumod"

RAKU_RETURN_TYPES: dict[str, str | None] = {
    "const char*": "Str",
    "DuallityStatus": "uint32",
    "uint32_t": "uint32",
    "void": None,
}

RAKU_PARAMETER_TYPES = {
    "const VtResource*": "Vinary::Tree::Interop::RawResource",
    "const uint8_t*": "Pointer",
    "const DuallityWfst*": "Pointer",
    "DuallityWfst*": "Pointer",
    "DuallityWfst**": "Pointer",
    "VtResource*": "Vinary::Tree::Interop::RawResource",
    "size_t": "size_t",
    "uint32_t": "uint32",
}


def abort(message: str) -> NoReturn:
    raise SystemExit(f"generate-raku-abi: {message}")


def load_model() -> dict:
    try:
        model = json.loads(MODEL_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        abort(f"cannot read {MODEL_PATH.relative_to(ROOT)}: {error}")
    if not isinstance(model, dict):
        abort("bindings/api.json must contain a JSON object")
    return model


def normalize_c(declaration: str) -> str:
    declaration = re.sub(r"\s+", " ", declaration.strip())
    declaration = re.sub(r"\s*\*\s*", "*", declaration)
    declaration = re.sub(r"\s*,\s*", ",", declaration)
    declaration = re.sub(r"\(\s*", "(", declaration)
    declaration = re.sub(r"\s*\)", ")", declaration)
    return declaration


def expected_prototype(function: dict) -> str:
    parameters = function["parameters"]
    rendered_parameters = ", ".join(
        f"{parameter['cType']} {parameter['name']}" for parameter in parameters
    )
    if not rendered_parameters:
        rendered_parameters = "void"
    return (
        f"DUALLITY_API {function['return']['cType']} {function['name']}"
        f"({rendered_parameters});"
    )


def header_prototypes() -> dict[str, str]:
    try:
        source = HEADER_PATH.read_text(encoding="utf-8")
    except OSError as error:
        abort(f"cannot read {HEADER_PATH.relative_to(ROOT)}: {error}")
    source = re.sub(r"/\*.*?\*/", " ", source, flags=re.DOTALL)
    matches = re.finditer(
        r"DUALLITY_API\s+(?P<return>[^;(]+?)\s+"
        r"(?P<name>duallity_[a-z0-9_]+)\s*"
        r"\((?P<parameters>.*?)\)\s*;",
        source,
        flags=re.DOTALL,
    )
    result: dict[str, str] = {}
    for match in matches:
        name = match.group("name")
        if name in result:
            abort(f"duplicate C declaration for {name}")
        result[name] = normalize_c(
            f"DUALLITY_API {match.group('return')} {name}({match.group('parameters')});"
        )
    return result


def validate_model(model: dict) -> None:
    api_revision = model.get("apiRevision")
    if not isinstance(api_revision, int) or api_revision < 1:
        abort("apiRevision must be a positive integer")

    raku = model.get("raku")
    if not isinstance(raku, dict) or raku.get("module") != "Duallity::GeneratedAbi":
        abort("raku.module must be Duallity::GeneratedAbi")
    library = raku.get("library")
    required_library_keys = {"environment", "linux", "macos", "windows"}
    if not isinstance(library, dict) or set(library) != required_library_keys:
        abort(f"raku.library must define exactly {sorted(required_library_keys)}")
    if not all(isinstance(library[key], str) and library[key] for key in library):
        abort("every raku.library value must be a non-empty string")

    ffi_types = model.get("ffiTypes")
    if not isinstance(ffi_types, dict):
        abort("ffiTypes must be an object")
    expected_ffi_types = {
        "DuallityWfst": ("opaque", "Pointer"),
        "VtResource": (
            "imported-struct",
            "Vinary::Tree::Interop::RawResource",
        ),
    }
    for name, (kind, raku_type) in expected_ffi_types.items():
        item = ffi_types.get(name)
        if not isinstance(item, dict):
            abort(f"ffiTypes.{name} must be an object")
        if item.get("cKind") != kind or item.get("rakuType") != raku_type:
            abort(f"ffiTypes.{name} kind or Raku representation drifted")
        if not isinstance(item.get("ownership"), str) or not item["ownership"]:
            abort(f"ffiTypes.{name}.ownership must be non-empty")

    functions = model.get("cFunctions")
    if not isinstance(functions, list) or not functions:
        abort("cFunctions must be a non-empty array")
    names: set[str] = set()
    for index, function in enumerate(functions):
        where = f"cFunctions[{index}]"
        if not isinstance(function, dict):
            abort(f"{where} must be an object")
        name = function.get("name")
        if not isinstance(name, str) or not re.fullmatch(r"duallity_[a-z0-9_]+", name):
            abort(f"{where}.name is not a duallity C symbol")
        if name in names:
            abort(f"duplicate modeled C function {name}")
        names.add(name)
        since = function.get("sinceApiRevision")
        if not isinstance(since, int) or not 1 <= since <= api_revision:
            abort(f"{name}.sinceApiRevision must be within 1..{api_revision}")
        returned = function.get("return")
        if not isinstance(returned, dict):
            abort(f"{name}.return must be an object")
        for field in ("cType", "ownership", "nullability"):
            if not isinstance(returned.get(field), str) or not returned[field]:
                abort(f"{name}.return.{field} must be non-empty")
        parameters = function.get("parameters")
        if not isinstance(parameters, list):
            abort(f"{name}.parameters must be an array")
        parameter_names: set[str] = set()
        for parameter in parameters:
            if not isinstance(parameter, dict):
                abort(f"{name} has a non-object parameter")
            parameter_name = parameter.get("name")
            if not isinstance(parameter_name, str) or not parameter_name:
                abort(f"{name} has a parameter without a name")
            if parameter_name in parameter_names:
                abort(f"{name} has duplicate parameter {parameter_name}")
            parameter_names.add(parameter_name)
            if parameter.get("direction") not in {"in", "out", "inout"}:
                abort(f"{name}.{parameter_name}.direction is invalid")
            for field in ("cType", "ownership", "nullability"):
                if not isinstance(parameter.get(field), str) or not parameter[field]:
                    abort(f"{name}.{parameter_name}.{field} must be non-empty")
        raku_binding = function.get("raku")
        if not isinstance(raku_binding, dict) or not isinstance(
            raku_binding.get("bind"), bool
        ):
            abort(f"{name}.raku.bind must be Boolean")
        if not raku_binding["bind"]:
            if (
                not isinstance(raku_binding.get("reason"), str)
                or not raku_binding["reason"]
            ):
                abort(f"{name}.raku.reason is required when bind is false")
            continue
        c_return = returned["cType"]
        if c_return not in RAKU_RETURN_TYPES:
            abort(f"{name} has no Raku return mapping for {c_return}")
        for parameter in parameters:
            c_type = parameter["cType"]
            if c_type not in RAKU_PARAMETER_TYPES:
                abort(f"{name} has no Raku parameter mapping for {c_type}")

    actual = header_prototypes()
    if set(actual) != names:
        missing = sorted(names - set(actual))
        extra = sorted(set(actual) - names)
        abort(f"C header/model symbol drift: missing={missing}, extra={extra}")
    for function in functions:
        expected = normalize_c(expected_prototype(function))
        found = actual[function["name"]]
        if expected != found:
            abort(
                f"C signature drift for {function['name']}:\n"
                f"  model:  {expected}\n"
                f"  header: {found}"
            )


def raku_quote(value: str) -> str:
    return "'" + value.replace("\\", "\\\\").replace("'", "\\'") + "'"


def append_map(
    lines: list[str], name: str, entries: list[tuple[str, str | int]]
) -> None:
    lines.append(f"our constant {name} is export = Map.new(")
    for key, value in entries:
        rendered = str(value) if isinstance(value, int) else raku_quote(value)
        lines.append(f"    {raku_quote(key)} => {rendered},")
    lines.extend([");", ""])


def render_raku(model: dict) -> str:
    raku = model["raku"]
    library = raku["library"]
    lines = [
        f"unit module {raku['module']};",
        "",
        "use NativeCall;",
        "need Vinary::Tree::Interop;",
        "",
        "# Code generated by scripts/generate-raku-abi.py from bindings/api.json",
        "# after validating include/duallity.h; DO NOT EDIT.",
        f"our constant ABI-VERSION is export = {model['abiVersion']};",
        f"our constant API-REVISION is export = {model['apiRevision']};",
        "",
    ]
    for enum_key, public_name in (
        ("status", "Status"),
        ("algorithm", "Algorithm"),
        ("wfstKind", "WfstKind"),
    ):
        lines.append(f"our enum {public_name} is export (")
        for name, value in model["enums"][enum_key]["values"].items():
            lines.append(f"    {name.replace('_', '-')} => {value},")
        lines.extend([");", ""])

    append_map(
        lines,
        "TYPE-KINDS",
        [(name, item["cKind"]) for name, item in model["ffiTypes"].items()],
    )
    append_map(
        lines,
        "TYPE-RAKU-REPRESENTATIONS",
        [(name, item["rakuType"]) for name, item in model["ffiTypes"].items()],
    )
    append_map(
        lines,
        "TYPE-OWNERSHIP",
        [(name, item["ownership"]) for name, item in model["ffiTypes"].items()],
    )

    functions = model["cFunctions"]
    append_map(
        lines,
        "C-SIGNATURES",
        [(function["name"], expected_prototype(function)) for function in functions],
    )
    append_map(
        lines,
        "FUNCTION-SINCE-API-REVISION",
        [(function["name"], function["sinceApiRevision"]) for function in functions],
    )
    ownership: list[tuple[str, str]] = []
    nullability: list[tuple[str, str]] = []
    for function in functions:
        name = function["name"]
        ownership.append((f"{name}:return", function["return"]["ownership"]))
        nullability.append((f"{name}:return", function["return"]["nullability"]))
        for parameter in function["parameters"]:
            key = f"{name}:{parameter['name']}"
            ownership.append((key, parameter["ownership"]))
            nullability.append((key, parameter["nullability"]))
    append_map(lines, "FUNCTION-OWNERSHIP", ownership)
    append_map(lines, "FUNCTION-NULLABILITY", nullability)
    append_map(
        lines,
        "RAKU-BINDING-EXCLUSIONS",
        [
            (function["name"], function["raku"]["reason"])
            for function in functions
            if not function["raku"]["bind"]
        ],
    )

    lines.extend(
        [
            "sub native-library(--> Str:D) {",
            (
                f"    return %*ENV<{library['environment']}> if "
                f"%*ENV<{library['environment']}>:exists;"
            ),
            f"    $*DISTRO.is-win ?? {raku_quote(library['windows'])} !!",
            f"        $*KERNEL.name eq 'darwin' ?? {raku_quote(library['macos'])} !!",
            f"        {raku_quote(library['linux'])}",
            "}",
            "",
        ]
    )

    for function in functions:
        if not function["raku"]["bind"]:
            continue
        name = function["name"]
        raku_name = name.replace("_", "-")
        parameters = []
        for parameter in function["parameters"]:
            rendered = RAKU_PARAMETER_TYPES[parameter["cType"]]
            if parameter["direction"] in {"out", "inout"}:
                rendered += " is rw"
            parameters.append(rendered)
        returned = RAKU_RETURN_TYPES[function["return"]["cType"]]
        signature_parts = parameters[:]
        if returned is not None:
            signature_parts.append(f"--> {returned}")
        signature = ",\n    ".join(signature_parts)
        if signature:
            lines.append(f"our sub {raku_name}(")
            lines.append(f"    {signature}")
            lines.append(")")
        else:
            lines.append(f"our sub {raku_name}()")
        lines.extend(
            [
                "    is native(&native-library)",
                f"    is symbol({raku_quote(name)})",
                "    is export(:native)",
                "{ * }",
                "",
            ]
        )
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="verify generated output")
    mode.add_argument("--write", action="store_true", help="rewrite generated output")
    args = parser.parse_args()

    model = load_model()
    validate_model(model)
    rendered = render_raku(model)
    if args.write:
        OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
        OUTPUT_PATH.write_text(rendered, encoding="utf-8")
        print(f"wrote {OUTPUT_PATH.relative_to(ROOT)}")
        return 0

    try:
        current = OUTPUT_PATH.read_text(encoding="utf-8")
    except OSError as error:
        abort(f"cannot read {OUTPUT_PATH.relative_to(ROOT)}: {error}")
    if current != rendered:
        print(
            f"{OUTPUT_PATH.relative_to(ROOT)} is stale; run "
            "python3 scripts/generate-raku-abi.py --write",
            file=sys.stderr,
        )
        return 1
    print("Raku ABI model, C header, and generated NativeCall declarations agree")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
