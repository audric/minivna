import pefile, struct, sys, json, re, os
from capstone import *
from capstone.x86 import *
D=os.path.dirname(os.path.abspath(__file__))+'/exe/'
pe = pefile.PE(D+'unpacked.exe'); base = pe.OPTIONAL_HEADER.ImageBase
img = pe.get_memory_mapped_image()
code = [s for s in pe.sections if s.Name.startswith(b'CODE')][0]
c0 = base + code.VirtualAddress; c1 = c0 + code.Misc_VirtualSize
meths = {int(k,16): v for k, v in json.load(open(D+'meths.json')).items()}
names = dict(meths)
try:
    for l in open(D+'../names.txt'):
        l=l.split()
        if len(l)>=2: names[int(l[0],16)]=l[1]
except Exception: pass
imps = {}
try: G={int(k,16):v for k,v in json.load(open(D+'../globals.json')).items()}
except Exception: G={}
F={}
try:
    for l in open(D+'../formfields.txt'):
        k,v=l.split(); F[int(k,16)]=v
except Exception: pass
for e in pe.DIRECTORY_ENTRY_IMPORT:
    for i in e.imports: imps[i.address] = (i.name or b'?').decode()
def rd(a, n): return img[a-base:a-base+n]
def u32(a): return struct.unpack('<I', rd(a,4))[0]
def ext(b):
    m, se = struct.unpack('<QH', b); s = -1 if se & 0x8000 else 1; e = se & 0x7fff
    if e == 0 and m == 0: return 0.0
    return s * m / 2**63 * 2.0**(e-16383)
def dstr(a):
    try:
        n = struct.unpack('<i', rd(a-4, 4))[0]
        if 0 < n < 300 and u32(a-8)==0xffffffff:
            s = rd(a, n)
            return s.decode('latin1')
    except Exception: pass
    return None
md = Cs(CS_ARCH_X86, CS_MODE_32); md.detail = True
def thunk(t):
    # jmp [imp]
    b=rd(t,6)
    if b[:2]==b'\xff\x25':
        a=u32(t+2)
        if a in imps: return imps[a]
    return None
def note_for(ins):
    note=''
    for op in ins.operands:
        if op.type == X86_OP_MEM and op.mem.base == 0 and op.mem.index == 0:
            t = op.mem.disp & 0xffffffff
            if t in imps: note += ' ; ' + imps[t]
            elif c0 <= t < c1 and ins.mnemonic.startswith('f'):
                if op.size == 4: note += ' ; %g' % struct.unpack('<f', rd(t,4))
                elif op.size == 8: note += ' ; %.15g' % struct.unpack('<d', rd(t,8))
                elif op.size == 10: note += ' ; %.15g' % ext(rd(t,10))
                elif op.size == 2: note += ' ; i16 %d' % struct.unpack('<h', rd(t,2))
            elif 0x4ac000 <= t < 0x4b2000:
                v=u32(t)
                note += ' ; =>%s' % G.get(v,'%x'%v)
            else: note += ' ; g_%s' % G.get(t,'%x'%t)
        if op.type == X86_OP_IMM:
            t = op.imm & 0xffffffff
            if ins.mnemonic == 'call':
                if t in names: note += ' ; ' + names[t]
                else:
                    th=thunk(t)
                    if th: note += ' ; ' + th
            elif c0 <= t < c1:
                s = dstr(t)
                if s is not None: note += ' ; "%s"' % s.replace('\r','\\r').replace('\n','\\n')
                elif t in names: note += ' ; &' + names[t]
    m=re.search(r'\[e[a-z]x \+ (0x[2-5][0-9a-f]{2})\]', ins.op_str)
    if m and int(m.group(1),16) in F and int(m.group(1),16)>=0x2c4: note += ' ; .'+F[int(m.group(1),16)]
    return note
def func(start, out=sys.stdout, limit=0x4000):
    a=start; maxt=start; calls=[]
    while a < start+limit:
        ins=next(md.disasm(rd(a,16),a),None)
        if ins is None:
            out.write('%x: ??\n'%a); break
        out.write('%x: %-7s %s%s\n' % (ins.address, ins.mnemonic, ins.op_str, note_for(ins)))
        if ins.mnemonic=='call' and ins.operands[0].type==X86_OP_IMM: calls.append(ins.operands[0].imm)
        if ins.mnemonic.startswith('j') and ins.operands[0].type==X86_OP_IMM:
            t=ins.operands[0].imm
            if start<=t<start+limit: maxt=max(maxt,t)
        if ins.mnemonic=='push' and ins.operands[0].type==X86_OP_IMM:
            t=ins.operands[0].imm&0xffffffff
            if a<t<start+limit and dstr(t) is None: maxt=max(maxt,t)
        if ins.mnemonic=='jmp' and ins.operands[0].type==X86_OP_MEM and ins.operands[0].mem.index!=0 and ins.operands[0].mem.scale==4:
            tb=ins.operands[0].mem.disp&0xffffffff
            # jump table directly after
            k=tb; ents=[]
            while True:
                v=u32(k)
                if not (start<=v<start+limit): break
                if k>=min(ents+[1<<32]): break
                ents.append(v); k+=4
            out.write('   jumptable: '+' '.join('%x'%e for e in ents)+'\n')
            if ents: maxt=max(maxt,max(ents))
            if tb==a+ins.size:
                a=k; continue
        a+=ins.size
        if ins.mnemonic in('ret','retn') and a>maxt: break
        if ins.mnemonic=='jmp' and a>maxt and ins.operands[0].type==X86_OP_IMM: break
    return a, calls
if __name__ == '__main__':
    for s in sys.argv[1:]:
        e,_=func(int(s,16))
        print('---- end %x'%e)
