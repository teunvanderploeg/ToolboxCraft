#!/usr/bin/env python3
"""Require the exact bundled artwork in each executable's Windows icon resources."""
import pathlib
import struct
import sys


def ico_images(data):
    reserved, kind, count = struct.unpack_from('<HHH', data)
    assert reserved == 0 and kind == 1 and count > 0, 'Invalid source ICO'
    images = []
    for index in range(count):
        _, _, _, _, _, _, size, offset = struct.unpack_from('<BBBBHHII', data, 6 + index * 16)
        payload = data[offset:offset + size]
        assert len(payload) == size, 'Truncated ICO image'
        images.append(payload)
    return images


def pe_resources(data):
    pe = struct.unpack_from('<I', data, 0x3c)[0]
    assert data[pe:pe + 4] == b'PE\0\0', 'Not a Windows executable'
    sections = struct.unpack_from('<H', data, pe + 6)[0]
    optional_size = struct.unpack_from('<H', data, pe + 20)[0]
    optional = pe + 24
    magic = struct.unpack_from('<H', data, optional)[0]
    assert magic in (0x10b, 0x20b), 'Unknown PE optional header'
    directories = optional + (112 if magic == 0x20b else 96)
    resource_rva = struct.unpack_from('<I', data, directories + 16)[0]
    assert resource_rva, 'Executable has no Windows resources'
    table = optional + optional_size

    def offset(rva):
        for index in range(sections):
            virtual_size, address, raw_size, raw_offset = struct.unpack_from('<IIII', data, table + index * 40 + 8)
            if address <= rva < address + max(virtual_size, raw_size):
                return raw_offset + rva - address
        raise AssertionError(f'Unmapped resource RVA {rva}')

    base = offset(resource_rva)
    result = {}

    def walk(relative, keys=()):
        assert len(keys) <= 3, 'Invalid resource tree depth'
        named, ids = struct.unpack_from('<HH', data, base + relative + 12)
        for index in range(named + ids):
            name, target = struct.unpack_from('<II', data, base + relative + 16 + index * 8)
            path = keys + (name,)
            if target & 0x80000000:
                walk(target & 0x7fffffff, path)
            else:
                rva, length = struct.unpack_from('<II', data, base + target)
                start = offset(rva)
                payload = data[start:start + length]
                assert len(payload) == length, 'Truncated PE resource'
                result[path] = payload
    walk(0)
    return result


def verify(ico, executable):
    expected = ico_images(ico.read_bytes())
    resources = pe_resources(executable.read_bytes())
    groups = [(path, payload) for path, payload in resources.items() if path[0] == 14]
    assert groups, f'{executable.name} has no executable icon'
    for path, group in groups:
        reserved, kind, count = struct.unpack_from('<HHH', group)
        if reserved or kind != 1 or count != len(expected):
            continue
        actual = []
        for index in range(count):
            icon_id = struct.unpack_from('<H', group, 6 + index * 14 + 12)[0]
            payload = resources.get((3, icon_id, path[-1]))
            if payload is None:
                payload = next((payload for key, payload in resources.items() if key[:2] == (3, icon_id)), None)
            actual.append(payload)
        if actual == expected:
            print(f'{executable.name}: ToolboxCraft icon verified at all {count} sizes')
            return
    raise AssertionError(f'{executable.name} does not contain the expected ToolboxCraft artwork')


if __name__ == '__main__':
    if len(sys.argv) < 3:
        sys.exit('Usage: verify_windows_icons.py artwork.ico executable.exe [executable.exe ...]')
    for name in sys.argv[2:]:
        verify(pathlib.Path(sys.argv[1]), pathlib.Path(name))
