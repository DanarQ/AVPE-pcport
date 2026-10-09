"""Read-only XBE disassembly helper; no original game code is executed on the host."""
import argparse
import json
import pathlib
import struct
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'local/tools/pylibs'))
from capstone import Cs, CS_ARCH_X86, CS_MODE_32


class Xbe:
    def __init__(self, path=ROOT / 'Aliens Versus Predator Extinction/default.xbe'):
        self.data = path.read_bytes()
        self.u32 = lambda o: struct.unpack_from('<I', self.data, o)[0]
        self.base = self.u32(0x104)
        self.sections = []
        for i in range(self.u32(0x11c)):
            o = self.u32(0x120) - self.base + 56 * i
            flags, va, vs, raw, size, name = struct.unpack_from('<6I', self.data, o)
            no = name - self.base
            self.sections.append(dict(name=self.data[no:self.data.index(0, no)].decode(), flags=flags,
                                      va=va, virtual_size=vs, raw=raw, size=size))
        self.disassembler = Cs(CS_ARCH_X86, CS_MODE_32)

    def offset(self, va):
        for s in self.sections:
            if s['va'] <= va < s['va'] + s['size']:
                return s['raw'] + va - s['va']
        if self.base <= va < self.base + self.u32(0x108):
            return va - self.base
        raise ValueError(f'Unbacked virtual address: {va:08x}')

    def va(self, offset):
        for s in self.sections:
            if s['raw'] <= offset < s['raw'] + s['size']:
                return s['va'] + offset - s['raw']
        return self.base + offset

    def disasm(self, va, length=256):
        o = self.offset(va)
        for insn in self.disassembler.disasm(self.data[o:o + length], va):
            print(f'{insn.address:08x} {insn.mnemonic:9s} {insn.op_str}')

    def refs(self, value):
        needle = struct.pack('<I', value)
        start = 0
        while (start := self.data.find(needle, start)) >= 0:
            print(f'{self.va(start):08x} (file {start:08x})')
            start += 1


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('command', choices=['disasm', 'refs', 'find', 'sections'])
    p.add_argument('value', nargs='?', default='0')
    p.add_argument('--length', type=int, default=256)
    a = p.parse_args()
    x = Xbe()
    if a.command == 'sections':
        print(json.dumps(x.sections, indent=2))
    elif a.command == 'disasm':
        x.disasm(int(a.value, 0), a.length)
    elif a.command == 'refs':
        x.refs(int(a.value, 0))
    else:
        o = x.data.find(a.value.encode())
        if o < 0:
            raise SystemExit('String not found')
        print(f'file={o:08x} va={x.va(o):08x}')
        x.refs(x.va(o))
