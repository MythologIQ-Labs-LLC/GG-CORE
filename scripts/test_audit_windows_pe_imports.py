#!/usr/bin/env python3
"""Pure-stdlib synthetic PE regression tests; no native execution or network."""
import struct
import tempfile
import unittest
from pathlib import Path

from audit_windows_pe_imports import PEError, analyze, read_pe_imports


def synthetic_pe(imports, machine=0x8664):
    """Minimal x64 PE32+ file with one .rdata section and import directory."""
    data = bytearray(0x800)
    data[:2] = b"MZ"
    struct.pack_into('<I', data, 0x3c, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into('<HH', data, 0x84, machine, 1)
    struct.pack_into('<H', data, 0x94, 0xf0)
    opt = 0x98
    struct.pack_into('<H', data, opt, 0x20b)
    struct.pack_into('<I', data, opt + 60, 0x200)
    struct.pack_into('<I', data, opt + 108, 16)
    struct.pack_into('<II', data, opt + 112 + 8, 0x1000, (len(imports) + 1) * 20)
    section = opt + 0xf0
    data[section:section + 8] = b'.rdata\0\0'
    struct.pack_into('<IIII', data, section + 8, 0x600, 0x1000, 0x600, 0x200)
    for i, name in enumerate(imports):
        start = 0x200 + i * 20
        name_offset = 0x300 + i * 40
        struct.pack_into('<I', data, start + 12, 0x1000 + (name_offset - 0x200))
        encoded = name.encode('ascii') + b'\0'
        assert len(encoded) <= 40
        data[name_offset:name_offset + len(encoded)] = encoded
    return bytes(data)


class AuditWindowsPEImportsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.stage = Path(self.temp.name)
        (self.stage / 'gg-core-cli.exe').write_bytes(synthetic_pe(['kernel32.dll', 'msvcp140.dll']))
        (self.stage / 'gg_core.dll').write_bytes(synthetic_pe(['ntdll.dll', 'msvcp140.dll']))

    def test_real_import_names_and_missing_redist(self):
        self.assertEqual(read_pe_imports(self.stage / 'gg-core-cli.exe'), ['kernel32.dll', 'msvcp140.dll'])
        result = analyze(self.stage)
        self.assertEqual(result['result'], 'FAIL')
        self.assertEqual(result['missing_app_local'], ['msvcp140.dll'])

    def test_present_dependency_closes_import_tree(self):
        (self.stage / 'msvcp140.dll').write_bytes(synthetic_pe(['kernel32.dll']))
        result = analyze(self.stage)
        self.assertEqual(result['result'], 'PASS')
        self.assertEqual(result['missing_app_local'], [])
        self.assertEqual(result['unreferenced_dlls'], [])
        self.assertIn('msvcp140.dll', result['binaries'])

    def test_system_dll_side_loading_refused(self):
        (self.stage / 'kernel32.dll').write_bytes(synthetic_pe([]))
        with self.assertRaisesRegex(PEError, 'shadows'):
            analyze(self.stage)

    def test_unreferenced_dll_refused(self):
        (self.stage / 'extra.dll').write_bytes(synthetic_pe([]))
        self.assertIn('extra.dll', analyze(self.stage)['unreferenced_dlls'])

    def test_wrong_architecture_refused(self):
        (self.stage / 'gg-core-cli.exe').write_bytes(synthetic_pe([], machine=0x14c))
        with self.assertRaisesRegex(PEError, 'not Windows x64'):
            analyze(self.stage)

    def test_missing_native_dll_refused(self):
        (self.stage / 'gg_core.dll').unlink()
        with self.assertRaisesRegex(PEError, 'missing packaged binary'):
            analyze(self.stage)

    def test_malformed_import_rva_refused(self):
        broken = bytearray((self.stage / 'gg-core-cli.exe').read_bytes())
        struct.pack_into('<I', broken, 0x200 + 12, 0xdeadbeef)
        (self.stage / 'gg-core-cli.exe').write_bytes(broken)
        with self.assertRaisesRegex(PEError, 'unmapped PE RVA'):
            analyze(self.stage)


if __name__ == '__main__':
    unittest.main(verbosity=2)
