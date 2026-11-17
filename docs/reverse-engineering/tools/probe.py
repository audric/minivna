import serial, sys, time
R = 10.73741824
def sweep(mode, f0, n, step, timeout=3):
    s = serial.Serial('/dev/ttyUSB0', 115200, timeout=timeout)
    time.sleep(0.3); s.read(s.in_waiting or 0)
    cmd = "%d\r%d\r%d\r%d\r" % (mode, int(f0*R), n, int(step*R))
    t = time.time(); s.write(cmd.encode())
    data = b''
    while len(data) < 4*n:
        c = s.read(4*n - len(data))
        if not c: break
        data += c
    extra = s.read(64) if len(data) == 4*n else b''
    s.close()
    return cmd, data, extra, time.time()-t
if __name__ == '__main__':
    mode, f0, n, step = int(sys.argv[1]), float(sys.argv[2]), int(sys.argv[3]), float(sys.argv[4])
    cmd, d, extra, dt = sweep(mode, f0, n, step)
    print(repr(cmd), "got", len(d), "bytes in %.2fs" % dt, "extra", len(extra))
    for i in range(0, len(d) - 3, 4):
        a = d[i] | d[i+1] << 8; b = d[i+2] | d[i+3] << 8
        print("%10.4f MHz  raw %s  A=%4d B=%4d" % ((f0 + i//4*step)/1e6, d[i:i+4].hex(), a, b))
