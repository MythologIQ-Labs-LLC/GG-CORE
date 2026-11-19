"""Build-input integrity for the native Windows audit gate (#225 review F1/F2).

A correct Git HEAD does not pin the compiled artifact: environment overrides
(RUSTFLAGS, RUSTC_WRAPPER, MSVC CL/_CL_, llama.cpp GGML_*/CMAKE_* cache
options) and Cargo config files outside the checkout change what is built.
The gate refuses them and records the exact toolchain binaries it used.
"""

import hashlib
import os
from pathlib import Path
import shutil
import subprocess

FORBIDDEN_NAMES = {
    "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS",
    "RUSTC", "RUSTDOC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTC_BOOTSTRAP",
    "CC", "CXX", "CFLAGS", "CXXFLAGS", "LDFLAGS", "CL", "_CL_", "LINK", "_LINK_",
    "CMAKE_ARGS", "CMAKE_TOOLCHAIN_FILE",
}
FORBIDDEN_PREFIXES = (
    "CARGO_BUILD_", "CARGO_PROFILE_", "CARGO_TARGET_", "CARGO_HOST_", "CARGO_UNSTABLE_",
    "CARGO_SOURCE_", "GGML_", "LLAMA_", "CMAKE_",
)
# Required by the gate itself or by a plain MSVC + Ninja toolchain.
ALLOWED_NAMES = {"CARGO_TARGET_DIR", "CMAKE_GENERATOR"}
CONFIG_NAMES = ("config", "config.toml")


def forbidden_overrides(environ):
    """Names of build-affecting variables present in `environ` (case-insensitive)."""
    found = []
    for name in environ:
        upper = name.upper()
        if upper in ALLOWED_NAMES:
            continue
        if upper in FORBIDDEN_NAMES or upper.startswith(FORBIDDEN_PREFIXES):
            found.append(name)
    return sorted(found)


def external_cargo_configs(repo, environ):
    """Cargo config files that Cargo would merge from outside the checkout."""
    home = Path(environ.get("CARGO_HOME") or Path.home() / ".cargo")
    candidates = [home / name for name in CONFIG_NAMES]
    for ancestor in Path(repo).resolve().parents:
        candidates.extend(ancestor / ".cargo" / name for name in CONFIG_NAMES)
    return [str(path) for path in candidates if path.is_file()]


def require_controlled_build(repo, environ):
    """Refuse unexpected build overrides; return the recorded build inputs."""
    overrides = forbidden_overrides(environ)
    if overrides:
        raise RuntimeError(f"Build-affecting environment overrides present: {overrides}")
    configs = external_cargo_configs(repo, environ)
    if configs:
        raise RuntimeError(f"Cargo config outside the checkout would alter the build: {configs}")
    return {"cargo_home": environ.get("CARGO_HOME", ""),
            "cmake_generator": environ.get("CMAKE_GENERATOR", "")}


def _sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def toolchain_identity(cwd):
    """Paths and SHA-256 of the rustc/cargo binaries actually used, not just versions."""
    run = lambda *argv: subprocess.check_output(argv, cwd=cwd, text=True).strip()
    exe = ".exe" if os.name == "nt" else ""
    rustc = Path(run("rustc", "--print", "sysroot")) / "bin" / ("rustc" + exe)
    cargo = Path(shutil.which("cargo") or "")
    return {
        "rustc": run("rustc", "--version"), "rustc_verbose": run("rustc", "-vV"),
        "cargo": run("cargo", "--version"),
        "rustc_path": str(rustc), "rustc_sha256": _sha256(rustc),
        "cargo_path": str(cargo), "cargo_sha256": _sha256(cargo),
    }
