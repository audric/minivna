import pefile, struct, sys, json, re
from capstone import *
from capstone.x86 import *
pe = pefile.PE('unpacked.exe'); base = pe.OPTIONAL_HEADER.ImageBase
img = pe.get_memory_mapped_image()
code = [s for s in pe.sections if s.Name.startswith(b'CODE')][0]
c0 = base + code.VirtualAddress; c1 = c0 + code.Misc_VirtualSize
meths = {int(k,16): v for k, v in json.load(open('meths.json')).items()}
imps = {}
for e in pe.DIRECTORY_ENTRY_IMPORT:
    for i in e.imports: imps[i.address] = (i.name or b'?').decode()
def rd(a, n): return img[a-base:a-base+n]
def ext(b):
    m, se = struct.unpack('<QH', b); s = -1 if se & 0x8000 else 1; e = se & 0x7fff
    if e == 0 and m == 0: return 0.0
    return s * m / 2**63 * 2.0**(e-16383)
def dstr(a):
    # delphi ansistring: len at a-4
    try:
        n = struct.unpack('<i', rd(a-4, 4))[0]
        if 0 < n < 200:
            s = rd(a, n)
            if all(32 <= x < 127 or x in (13,10,9) or x >= 0xa0 for x in s): return s.decode('latin1')
    except Exception: pass
    return None
md = Cs(CS_ARCH_X86, CS_MODE_32); md.detail = True
def dis(start, end=None, maxi=4000):
    a = start; n = 0
    for ins in md.disasm(rd(start, 20000), start):
        note = ''
        for op in ins.operands:
            if op.type == X86_OP_MEM and op.mem.base == 0 and op.mem.index == 0:
                t = op.mem.disp & 0xffffffff
                if t in imps: note += ' ; ' + imps[t]
                elif c0 <= t < c1 and ins.mnemonic.startswith('f'):
                    if op.size == 4: note += ' ; %g' % struct.unpack('<f', rd(t,4))
                    elif op.size == 8: note += ' ; %g' % struct.unpack('<d', rd(t,8))
                    elif op.size == 10: note += ' ; %.12g' % ext(rd(t,10))
                    elif op.size == 2: note += ' ; i16 %d' % struct.unpack('<h', rd(t,2))
                else: note += ' ; [%x]' % t
            if op.type == X86_OP_IMM:
                t = op.imm & 0xffffffff
                if ins.mnemonic == 'call':
                    if t in meths: note += ' ; ' + meths[t]
                elif c0 <= t < c1:
                    s = dstr(t)
                    if s is not None: note += ' ; "%s"' % s.replace('\r','\\r')
                    elif t in meths: note += ' ; &' + meths[t]
        print('%x: %-7s %s%s' % (ins.address, ins.mnemonic, ins.op_str, note))
        n += 1
        if end and ins.address >= end: break
        if not end and ins.mnemonic == 'ret': break
        if n >= maxi: break
if __name__ == '__main__':
    dis(int(sys.argv[1],16), int(sys.argv[2],16) if len(sys.argv) > 2 else None)
