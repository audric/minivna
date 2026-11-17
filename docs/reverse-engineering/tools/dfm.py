import sys, struct
class R:
    def __init__(s, d): s.d=d; s.p=4
    def b(s): v=s.d[s.p]; s.p+=1; return v
    def ss(s): n=s.b(); v=s.d[s.p:s.p+n].decode('latin1'); s.p+=n; return v
    def val(s):
        t=s.b(); d=s.d
        if t==0: return None
        if t==1:
            out=[]
            while d[s.p]!=0: out.append(s.val())
            s.p+=1; return out
        if t==2: v=struct.unpack_from('<b',d,s.p)[0]; s.p+=1; return v
        if t==3: v=struct.unpack_from('<h',d,s.p)[0]; s.p+=2; return v
        if t==4: v=struct.unpack_from('<i',d,s.p)[0]; s.p+=4; return v
        if t==5: s.p+=10; return '<ext>'
        if t in (6,7): return s.ss()
        if t==8: return False
        if t==9: return True
        if t==10: n=struct.unpack_from('<I',d,s.p)[0]; s.p+=4+n; return '<bin %d>'%n
        if t==11:
            out=[]
            while True:
                x=s.ss()
                if not x: break
                out.append(x)
            return '['+','.join(out)+']'
        if t in (12,20): n=struct.unpack_from('<I',d,s.p)[0]; s.p+=4; v=d[s.p:s.p+n].decode('latin1'); s.p+=n; return v
        if t==13: return 'nil'
        if t==14:
            out=[]
            while d[s.p]!=0:
                if d[s.p] in (2,3,4): s.val()
                s.p+=1  # list start
                item={}
                while d[s.p]!=0:
                    k=s.ss(); item[k]=s.val()
                s.p+=1; out.append(item)
            s.p+=1; return out
        if t==15: v=struct.unpack_from('<f',d,s.p)[0]; s.p+=4; return v
        if t==18: n=struct.unpack_from('<I',d,s.p)[0]; s.p+=4; v=d[s.p:s.p+2*n].decode('utf-16le'); s.p+=2*n; return v
        if t==19: v=struct.unpack_from('<q',d,s.p)[0]; s.p+=8; return v
        raise Exception('type %d at %d'%(t,s.p))
    def obj(s, ind=0):
        if s.d[s.p]&0xF0==0xF0:
            f=s.b()
            if f&2: s.val()
        cls=s.ss(); name=s.ss()
        print('  '*ind+'object %s: %s'%(name,cls))
        while s.d[s.p]!=0:
            k=s.ss(); v=s.val()
            if k in SKIP or k.startswith('Font.') or k.startswith('TitleFont'): continue
            print('  '*(ind+1)+'%s = %r'%(k,v))
        s.p+=1
        while s.d[s.p]!=0: s.obj(ind+1)
        s.p+=1
SKIP={'ParentFont','TabOrder','ParentColor','TextHeight','PixelsPerInch','OldCreateOrder','Glyph.Data','Picture.Data','Icon.Data','NumGlyphs','ParentShowHint','ShowHint','Layout','BevelOuter','BevelInner','Transparent','AutoSize','ParentBackground','Color','Cursor','Bitmap'}
R(open(sys.argv[1],'rb').read()).obj()
