"""Read-only inspection of the Rust-imported object regions."""
import argparse
import json
import pathlib
import struct

ROOT = pathlib.Path(__file__).resolve().parents[1]
CACHE = ROOT / 'local/cache'


class Block:
    def __init__(self, container, index):
        catalog = json.loads((CACHE / 'catalog.json').read_text())
        matches = [c for c in catalog['containers'] if container.lower() in c['source_path'].lower()]
        if len(matches) != 1:
            raise ValueError(f'Expected one container: {[c["source_path"] for c in matches]}')
        self.info = matches[0]['blocks'][index]
        self.chunks = {c['tag']: (CACHE / c['cache_path']).read_bytes() for c in self.info['chunks']}
        self.regions = [self.chunks.get(a, self.chunks.get(b, b'')) for a, b in [('DATX', 'DATA'), ('TEMX', 'TEMP'), ('EXTA', 'EXTA'), ('PLAX', 'PLAT')]]

    def at(self, pointer):
        region, offset = pointer >> 28, pointer & 0x0fffffff
        if region > 3 or offset > len(self.regions[region]):
            raise ValueError(f'Invalid pointer {pointer:08x}')
        return self.regions[region][offset:]

    def dump(self, pointer, length):
        data = self.at(pointer)[:length]
        for i in range(0, len(data) - 3, 4):
            value, = struct.unpack_from('<I', data, i)
            flt, = struct.unpack_from('<f', data, i)
            print(f'{pointer + i:08x} +{i:03x} {value:08x} {flt:12.5g} {data[i:i+4]!r}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('container')
    parser.add_argument('block', type=int)
    parser.add_argument('pointer', type=lambda s: int(s, 0))
    parser.add_argument('--length', type=int, default=128)
    args = parser.parse_args()
    Block(args.container, args.block).dump(args.pointer, args.length)
