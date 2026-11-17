import pefile, struct, re, json
pe = pefile.PE('unpacked.exe'); base = pe.OPTIONAL_HEADER.ImageBase
code = [s for s in pe.sections if s.Name.startswith(b'CODE')][0]
d = code.get_data(); va0 = base + code.VirtualAddress
meths = {}
# method table entries: word len, dword addr, shortstring name
for m in re.finditer(rb'[A-Za-z_][A-Za-z0-9_]{3,40}', d):
    p = m.start()
    if p < 7: continue
    n = d[p-1]
    if n != len(m.group()): continue
    addr = struct.unpack_from('<I', d, p-5)[0]; ln = struct.unpack_from('<H', d, p-7)[0]
    if ln == n + 7 and va0 <= addr < va0 + len(d):
        meths[addr] = m.group().decode()
json.dump({hex(k): v for k, v in sorted(meths.items())}, open('meths.json', 'w'), indent=0)
print(len(meths))
for k, v in sorted(meths.items()): print(hex(k), v)
