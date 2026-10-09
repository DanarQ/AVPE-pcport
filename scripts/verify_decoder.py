"""Compare Rust-imported chunks with the Xbox decompressor in Unicorn x86.

This executes only 0x133340..0x1334fd with isolated buffers, no Xbox OS or host calls.
Run after `cargo run -p avpe-tools -- import`.
"""
import argparse
import hashlib
import json
import pathlib
import struct
from xbe_probe import Xbe, ROOT
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--limit', type=int, default=0)
    args=parser.parse_args()
    x=Xbe(); cache=ROOT/'local/cache'
    catalog=json.loads((cache/'catalog.json').read_text())
    u=Uc(UC_ARCH_X86,UC_MODE_32)
    u.mem_map(0x10000,0x400000)
    for s in x.sections:
        u.mem_write(s['va'],x.data[s['raw']:s['raw']+s['size']])
    state=0x1000000; stack=0x1100000; src=0x2000000; dst=0x10000000
    u.mem_map(state,0x1000); u.mem_map(stack,0x10000)
    capacity=128*1024*1024
    u.mem_map(src,capacity); u.mem_map(dst,capacity)
    report=[]
    for container in catalog['containers']:
        original=(ROOT/'Aliens Versus Predator Extinction'/container['source_path']).read_bytes()
        for block in container['blocks']:
            for chunk in block['chunks']:
                if chunk['tag'] not in ['DATX','TEMX','PLAX']: continue
                o=chunk['file_offset']; data=original[o:o+chunk['original_bytes']]
                expected=chunk['decoded_bytes']; window=struct.unpack_from('<H',data,4)[0]
                shift=16-window.bit_length(); mask=(1<<shift)-1
                fields=[src,src+2,src+len(data),dst,dst,dst+expected+128,src+len(data),dst,window,mask,shift]
                state_bytes=bytearray(0x38)
                struct.pack_into('<11I',state_bytes,0,*fields)
                struct.pack_into('<H',state_bytes,0x2c,mask)
                u.mem_write(state,bytes(state_bytes)); u.mem_write(src,data[4:]+bytes(128))
                # Ensure malformed source cannot be mistaken for fresh output.
                u.mem_write(dst,bytes([0xcd])*expected)
                sp=stack+0xff00
                u.mem_write(sp,struct.pack('<II',0x10000,state)); u.reg_write(UC_X86_REG_ESP,sp)
                u.emu_start(0x133340,0x10000,timeout=10000000,count=100000000)
                assert u.reg_read(UC_X86_REG_EIP)==0x10000, 'Xbox decoder did not return'
                end=struct.unpack('<I',u.mem_read(state+0x10,4))[0]
                actual=bytes(u.mem_read(dst,end-dst))
                ref=(cache/chunk['cache_path']).read_bytes()
                ok=actual==ref
                report.append(dict(file=container['source_path'],block=block['index'],tag=chunk['tag'],bytes=len(actual),match=ok,sha256=hashlib.sha256(actual).hexdigest()))
                if not ok: raise RuntimeError(report[-1])
                if len(report)%100==0: print(f'{len(report)} source comparisons passed',flush=True)
                if args.limit and len(report)>=args.limit: break
            if args.limit and len(report)>=args.limit: break
        if args.limit and len(report)>=args.limit: break
    result={'xbe_sha256':hashlib.sha256(x.data).hexdigest(),'function':'0x00133340','method':'isolated x86 function; whole input/output buffers; refill wrapper not exercised','matches':len(report),'mismatches':0,'chunks':report}
    destination=ROOT/'local/research/decoder-parity.json'
    destination.parent.mkdir(parents=True,exist_ok=True)
    destination.write_text(json.dumps(result,indent=2))
    print(f'PASS: {len(report)} chunks compared byte for byte')


if __name__=='__main__': main()
