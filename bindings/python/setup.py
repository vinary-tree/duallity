from __future__ import annotations

import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

from setuptools import Distribution, setup
from setuptools.command.bdist_wheel import bdist_wheel
from setuptools.command.build_py import build_py
from setuptools.command.sdist import sdist

BINDING_DIRECTORY = Path(__file__).resolve().parent
sys.path.insert(0, str(BINDING_DIRECTORY))
from _registry_manifest import registry_text

BUNDLED_RUST_ROOT = BINDING_DIRECTORY / "rust"
REPOSITORY_ROOT = (
    BUNDLED_RUST_ROOT
    if (BUNDLED_RUST_ROOT / "Cargo.toml").is_file()
    else BINDING_DIRECTORY.parents[1]
)


def native_library_name() -> str:
    """Return this platform's duallity shared-library filename."""
    system = platform.system()
    if system == "Windows":
        return "duallity.dll"
    if system == "Darwin":
        return "libduallity.dylib"
    return "libduallity.so"


def native_library() -> Path:
    """Resolve a verified prebuilt library or build the exact source tree."""
    explicit = os.environ.get("DUALLITY_PREBUILT_LIBRARY")
    if explicit:
        library = Path(explicit).expanduser().resolve()
        if not library.is_file():
            raise FileNotFoundError(
                f"DUALLITY_PREBUILT_LIBRARY is not a file: {library}"
            )
        return library

    command = [
        "cargo",
        "build",
        "--manifest-path",
        str(REPOSITORY_ROOT / "Cargo.toml"),
        "--release",
        "--no-default-features",
        "--features",
        "python-bindings",
    ]
    target = os.environ.get("DUALLITY_RUST_TARGET")
    if target:
        command.extend(["--target", target])
    subprocess.run(command, cwd=REPOSITORY_ROOT, check=True)

    target_directory = Path(
        os.environ.get("CARGO_TARGET_DIR", REPOSITORY_ROOT / "target")
    )
    profile_directory = (
        target_directory / target / "release"
        if target
        else target_directory / "release"
    )
    library = profile_directory / native_library_name()
    if not library.is_file():
        raise FileNotFoundError(f"Cargo did not produce the native library: {library}")
    return library


class BuildWithNativeLibrary(build_py):
    """Stage the Rust library and license inside the import package."""

    def run(self) -> None:
        super().run()
        destination = Path(self.build_lib) / "duallity"
        native_destination = destination / "native"
        native_destination.mkdir(parents=True, exist_ok=True)
        shutil.copy2(native_library(), native_destination / native_library_name())
        shutil.copy2(REPOSITORY_ROOT / "LICENSE", destination / "LICENSE")


class PlatformDistribution(Distribution):
    """Mark wheels as platform-specific because they embed a Rust library."""

    def has_ext_modules(self) -> bool:
        return True


class PortablePythonPlatformWheel(bdist_wheel):
    """Use one Python-3 ABI tag with the platform's native-library tag."""

    def finalize_options(self) -> None:
        super().finalize_options()
        self.root_is_pure = False

    def get_tag(self) -> tuple[str, str, str]:
        _, _, platform_tag = super().get_tag()
        return "py3", "none", platform_tag


class SelfContainedSourceDistribution(sdist):
    """Bundle the exact Rust source with registry-only sibling dependencies."""

    def make_release_tree(self, base_dir: str, files: list[str]) -> None:
        super().make_release_tree(base_dir, files)
        destination = Path(base_dir) / "rust"
        destination.mkdir(parents=True, exist_ok=False)
        (destination / "Cargo.toml").write_text(
            registry_text((REPOSITORY_ROOT / "Cargo.toml").read_text(encoding="utf-8")),
            encoding="utf-8",
        )
        shutil.copytree(REPOSITORY_ROOT / "src", destination / "src")
        for name in ("LICENSE", "README.md"):
            shutil.copy2(REPOSITORY_ROOT / name, destination / name)


setup(
    cmdclass={
        "build_py": BuildWithNativeLibrary,
        "bdist_wheel": PortablePythonPlatformWheel,
        "sdist": SelfContainedSourceDistribution,
    },
    distclass=PlatformDistribution,
)
