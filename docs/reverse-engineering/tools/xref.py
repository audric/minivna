import sys,struct,bisect
sys.path.insert(0,'.')
from dis2 import *
data=bytes(img)
ks=sorted(names)
def owner(a):
    i=bisect.bisect_right(ks,a)-1
    return '%s+%x'%(names[ks[i]],a-ks[i]) if i>=0 else '?'
for arg in sys.argv[1:]:
    v=int(arg,16)
    # pointers to v in DATA table, plus v itself
    targets=[v]
    for a in range(0x4ac000,0x4b2000,4):
        if u32(a)==v: targets.append(a)
    out=[]
    for t in targets:
        p=struct.pack('<I',t); i=c0-base
        while True:
            i=data.find(p,i,c1-base)
            if i<0: break
            out.append(base+i); i+=1
    print(arg, G.get(v,''), ' '.join('%x(%s)'%(a,owner(a)) for a in sorted(out)))
