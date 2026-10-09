#!/usr/bin/env python3
"""Check the actual arm64 Mach-O entry stub without running the extension."""
import pathlib
import struct
import sys


def verify_entrypoint(path):
    data = pathlib.Path(path).read_bytes()
    magic, cpu, _, filetype, count, _, _, _ = struct.unpack_from('<8I', data)
    assert (magic, cpu, filetype) == (0xFEEDFACF, 0x0100000C, 2), 'Expected an arm64 Mach-O executable'
    cursor = 32
    entry = symtab = indirect = None
    stubs = []
    for _ in range(count):
        command, size = struct.unpack_from('<2I', data, cursor)
        assert size >= 8 and cursor + size <= len(data), 'Invalid Mach-O load command'
        if command == 0x80000028:  # LC_MAIN; entryoff is a file offset
            entry = struct.unpack_from('<Q', data, cursor + 8)[0]
        elif command == 2:  # LC_SYMTAB
            symtab = struct.unpack_from('<4I', data, cursor + 8)
        elif command == 0xB:  # LC_DYSYMTAB
            indirect = struct.unpack_from('<2I', data, cursor + 56)
        elif command == 0x19:  # LC_SEGMENT_64
            sections = struct.unpack_from('<I', data, cursor + 64)[0]
            for i in range(sections):
                section = struct.unpack_from('<16s16sQQ8I', data, cursor + 72 + i * 80)
                if section[8] & 0xFF == 8:  # S_SYMBOL_STUBS
                    stubs.append((section[4], section[3], section[9], section[10]))
        cursor += size
    assert entry is not None and symtab and indirect, 'Extension bootstrap metadata missing'
    for offset, size, first, stride in stubs:
        if offset <= entry < offset + size:
            assert stride and (entry - offset) % stride == 0, 'Entry must begin at a symbol stub'
            index = first + (entry - offset) // stride
            assert index < indirect[1], 'Invalid indirect symbol index'
            symbol = struct.unpack_from('<I', data, indirect[0] + index * 4)[0]
            assert symbol < symtab[1], 'Invalid entry symbol'
            string = struct.unpack_from('<I', data, symtab[0] + symbol * 16)[0]
            assert string < symtab[3], 'Invalid entry symbol name'
            start = symtab[2] + string
            name = data[start:data.index(b'\0', start)].decode()
            assert name == '_NSExtensionMain', f'Extension entry is {name}, expected _NSExtensionMain'
            print(f'Widget LC_MAIN enters {name} at file offset {entry:#x}')
            return
    raise AssertionError('Widget LC_MAIN bypasses NSExtensionMain; ordinary Swift main is not the macOS extension bootstrap')


if __name__ == '__main__':
    verify_entrypoint(sys.argv[1])
