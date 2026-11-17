import sys
sys.path.insert(0,'.')
from dis2 import *
import struct
def walk_back(a):
    # a = address of a name-length byte of some entry; walk entries forward & backward heuristically: find start by scanning backward
    # entry: dword off, word idx, byte len, name
    # go backward: previous entry ends at a-6
    start=a-6
    while True:
        # find previous: search len byte position p such that p+1+len == start and name chars are identifiers
        found=None
        for n in range(1,60):
            p=start-1-n
            if rd(p,1)[0]==n and all(chr(c).isalnum() or c==95 for c in rd(p+1,n)):
                found=p; break
        if found is None: break
        start=found-6
    return start
def dump(a):
    p=walk_back(a); res={}
    while True:
        off,idx=struct.unpack('<IH',rd(p,6)); n=rd(p+6,1)[0]; nmb=rd(p+7,n)
        if n==0 or off>0x2000 or not all(chr(c).isalnum() or c==95 for c in nmb): break
        res[off]=nmb.decode(); p+=7+n
    return res
if __name__=='__main__':
    r=dump(int(sys.argv[1],16))
    print('  '.join('%x:%s'%(k,v) for k,v in sorted(r.items())))
