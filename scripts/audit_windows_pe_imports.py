#!/usr/bin/env python3
"""Check the x64 PE import closure of a packaged GG-CORE Windows CLI and DLL.

No platform binaries are executed. Standard-library-only, fail-closed, and
suitable for preflight on Linux as well as a restricted Windows builder.
This checks imported filenames and architecture, NOT Microsoft signatures or
runtime version compatibility; the separate PowerShell verifier handles those.
"""
import argparse
import hashlib
import json
import struct
from pathlib import Path

SYSTEM_DLLS = frozenset({
    "advapi32.dll", "bcrypt.dll", "bcryptprimitives.dll", "cabinet.dll",
    "cfgmgr32.dll", "combase.dll", "crypt32.dll", "cryptbase.dll",
    "gdi32.dll", "iphlpapi.dll", "kernel32.dll", "kernelbase.dll",
    "mswsock.dll", "netapi32.dll", "ntdll.dll", "ole32.dll", "oleaut32.dll",
    "rpcrt4.dll", "secur32.dll", "shell32.dll", "shlwapi.dll",
    "user32.dll", "userenv.dll", "ucrtbase.dll", "version.dll",
    "winhttp.dll", "winmm.dll", "ws2_32.dll", "wsock32.dll",
})
EXPECTED_ROOTS = ("gg-core-cli.exe", "gg_core.dll")


class PEError(ValueError):
    pass


def read_pe_imports(path):
    data = path.read_bytes()
    def u16(p):
        if p < 0 or p + 2 > len(data):
            raise PEError(f"truncated PE short: {path.name}")
        return struct.unpack_from("<H", data, p)[0]
    def u32(p):
        if p < 0 or p + 4 > len(data):
            raise PEError(f"truncated PE word: {path.name}")
        return struct.unpack_from("<I", data, p)[0]
    if len(data) < 0x100 or data[:2] != b"MZ":
        raise PEError(f"not an MZ executable: {path.name}")
    nt = u32(0x3c)
    if nt < 0x40 or nt + 24 > len(data) or data[nt:nt + 4] != b"PE\0\0":
        raise PEError(f"invalid PE signature: {path.name}")
    coff = nt + 4
    if u16(coff) != 0x8664:
        raise PEError(f"not Windows x64 PE: {path.name}")
    section_count, optional_size = u16(coff + 2), u16(coff + 16)
    opt = coff + 20
    if u16(opt) != 0x20b or opt + optional_size > len(data) or optional_size < 112:
        raise PEError(f"invalid PE32+ optional header: {path.name}")
    directories = u32(opt + 108)
    sections_start = opt + optional_size
    if not (1 <= section_count <= 96) or sections_start + section_count * 40 > len(data):
        raise PEError(f"invalid PE section count: {path.name}")
    sections = []
    for i in range(section_count):
        pos = sections_start + i * 40
        sections.append((u32(pos + 12), max(u32(pos + 8), u32(pos + 16)),
                         u32(pos + 20), u32(pos + 16)))
    headers_size = u32(opt + 60)
    def offset(rva, need=1):
        if rva < headers_size and rva + need <= len(data):
            return rva
        for start, span, raw, raw_size in sections:
            delta = rva - start
            if 0 <= delta and delta + need <= min(span, raw_size) and raw + delta + need <= len(data):
                return raw + delta
        raise PEError(f"unmapped PE RVA 0x{rva:x}: {path.name}")
    def cstring(rva):
        pos = offset(rva)
        limit = min(len(data), pos + 260)
        end = data.find(b"\0", pos, limit)
        if end < 0:
            raise PEError(f"invalid DLL name at RVA 0x{rva:x}: {path.name}")
        name = data[pos:end].decode("ascii", errors="strict").lower()
        if not name.endswith(".dll") or "/" in name or "\\" in name or name in (".dll", "..dll"):
            raise PEError(f"invalid imported DLL name: {name}")
        return name
    def descriptor_table(index, descriptor_size, name_field, is_delay=False):
        if index >= directories:
            return []
        base = opt + 112 + 8 * index
        if base + 8 > opt + optional_size:
            raise PEError(f"missing PE data directory: {path.name}")
        table_rva, table_size = u32(base), u32(base + 4)
        if table_rva == 0 and table_size == 0:
            return []
        if table_rva == 0 or table_size < descriptor_size:
            raise PEError(f"bad import directory: {path.name}")
        imports = []
        # Each directory must have a terminating all-zero descriptor.
        for i in range(min(table_size // descriptor_size, 4096)):
            pos = offset(table_rva + i * descriptor_size, descriptor_size)
            desc = data[pos:pos + descriptor_size]
            if not any(desc):
                return imports
            if is_delay and u32(pos) & 1 == 0:
                raise PEError(f"unsupported absolute-address delay import: {path.name}")
            name_rva = u32(pos + name_field)
            imports.append(cstring(name_rva))
        raise PEError(f"unterminated import directory: {path.name}")
    names = descriptor_table(1, 20, 12) + descriptor_table(13, 32, 4, True)
    return sorted(set(names))


def analyze(stage):
    entries = {p.name.lower(): p for p in stage.iterdir() if p.is_file() and not p.is_symlink()}
    if len(entries) != len([p for p in stage.iterdir() if p.is_file()]):
        raise PEError("case-colliding filenames or symlinks in package")
    # DLL search order and side-loading risks make unexpected app-local files
    # security-sensitive, even when they are not referenced in import tables.
    shadowed = sorted(name for name in entries if name in SYSTEM_DLLS or
                      name.startswith(("api-ms-win-", "ext-ms-win-")))
    if shadowed:
        raise PEError(f"packaged DLL shadows a Windows system/API-set name: {shadowed}")
    for name in EXPECTED_ROOTS:
        if name not in entries:
            raise PEError(f"missing packaged binary: {name}")
    pending = list(EXPECTED_ROOTS)
    checked = {}
    missing = set()
    while pending:
        name = pending.pop()
        if name in checked:
            continue
        path = entries[name]
        imports = read_pe_imports(path)
        checked[name] = {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                         "imports": imports}
        for dll in imports:
            if dll.startswith(("api-ms-win-", "ext-ms-win-")) or dll in SYSTEM_DLLS:
                continue
            if dll in entries:
                pending.append(dll)
            else:
                missing.add(dll)
    unused = sorted(name for name in entries if name.endswith(".dll") and name not in checked)
    return {"format": "gg-core-pe-import-audit-v1", "binaries": dict(sorted(checked.items())),
            "missing_app_local": sorted(missing), "unreferenced_dlls": unused,
            "result": "FAIL" if missing or unused else "PASS"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", type=Path, help="Extracted Windows portable package directory")
    args = parser.parse_args()
    report = analyze(args.stage)
    print(json.dumps(report, indent=2, sort_keys=True))
    if report["missing_app_local"] or report["unreferenced_dlls"]:
        raise SystemExit(2)


if __name__ == "__main__":
    main()
