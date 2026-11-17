# IG_miniVNA: findings from reading the original exe

Date: 2026-11-17. Produced by static reading of the UPX-unpacked `IG_miniVNA.exe` (version 2008, IW1GEU). Nothing was executed. Key constants (180/1023, 60/1023, 20.16, the N+2 count, the 0.0006-day timeout) were spot-checked a second time against the disassembly, and the level and phase formulas were checked against sweeps recorded from the real device (`docs/captures/`). The working files in the scratch directory named below are temporary and may no longer exist.

IG_miniVNA static analysis report (unpacked.exe, image base 0x400000)

Everything below was read from the disassembly unless marked INFERRED or NOT DETERMINED. Nothing was executed.

Working files, all in /tmp/claude-1000/-home-audric-dev-0-RF-miniVNA/691892a2-8841-4d0c-be7c-eb0798fba921/scratchpad:
- dis2.py: improved disassembler (follows jump targets past embedded string data, resolves the global pointer table, names form fields). `python3 dis2.py <addr>...`
- sweep.py, show.sh, xref.py, fields.py: helpers. names.txt, globals.json, formfields.txt: the names I assigned.
- core.txt (0x498d74-0x4a380c), main.txt (0x4a380c-0x4ab2c0), disp.txt (0x49dd5c), gen.txt, calform.txt: annotated listings.

Function map (core unit)
- 0x49bca0 OpenPort; 0x49bd68 ClosePort
- 0x49cd18 ReadParams (edit boxes to globals)
- 0x49bbc4 CalcStep
- 0x49bd94 Sweep(start Hz, stop Hz: doubles on stack; ax = N; dl = mode)
- 0x49c4b4 ParkAndClose (not "apply calibration")
- 0x49c588 SendGene(freqHz, coef)
- 0x49c6e0 Convert (raw readings to all physical and derived arrays, including the calibration offset)
- 0x49d104 SetupMode (Antenna / Loss / Length / Transmission)
- 0x49dd5c Display (drawing, marker fields, BW/Q, length, loss)
- 0x4a201c places M1/M2 on the band edges after a band preset

BpUneMesureClick order: OpenPort, Sleep(100), ReadParams, Sweep, then on success ParkAndClose, Convert, Display.

Globals (addresses are the variables; code reaches them through a pointer table at 0x4b13xx-0x4b1axx)
- 0x4acda8 dword fMin = 100000
- 0x4acdac dword fMax = 180000000 (ini "DDS Hz")
- 0x4acdb0 byte VNA1 flag
- 0x4acdb4 double ddsCoef = 10.73741824
- 0x4acdbc / 0x4acdc0 dword startHz / stopHz, as displayed (they include the extender multiplier)
- 0x4acdcc word N (default 500)
- 0x4acdd0 single stepHz
- 0x4acdd4 byte mult (1, 4, 8, 16)
- 0x4acdd8 byte "reload cal offsets" flag
- 0x4acde8 byte mode (0 reflection, 1 transmission)
- 0x4acdf8 byte phase180 (1 = 180° mode, the default)
- 0x4acdfc double graph RL offset ("Decalage")
- 0x4ace1c double velocity factor = 0.66
- 0x4ace24 byte UI mode (0 antenna, 1 cable loss, 2 cable length, 3 transmission)
- 0x4b388c / 0x4b388e / 0x4b3890 word M1 index, M2 index, SWR-min index
- Arrays indexed 0..N:
  - words: rawA 0x4b3894, rawB 0x4b406c, calOffR 0x4c6648, calOffT 0x4c6e20
  - doubles: freq 0x4b4844, SWR 0x4b679c, |Z| 0x4b86f4, RL 0x4ba64c, phase 0x4bc5a4, Rs 0x4be4fc, Xs 0x4c0454, L 0x4c23ac, C 0x4c4304
  - bytes: sign 0x4c625c
- Calibration table 0x4b28dc (4008 bytes)

---
1. Raw readings to physical values (Convert, 0x49c6e0; certain)

The first 16-bit reading of each point (rawA) is phase; the second (rawB) is level. Parsing is at 0x49c17a-0x49c1e4.

For i = 0..N:
```
freq[i]  = startHz + i*stepHz                          (0x49c783)
phase[i] = rawA[i] * 0.175953079178886   (= 180/1023)  (0x49c7b4)
RL[i]    = (rawB[i] + calOff[i]) * 0.0586510263929619 - 20.16   (0.05865... = 60/1023)
           calOff = calOffR in reflection (0x49c7d9), calOffT in transmission (0x49c806);
           signed 16-bit, added to the raw count before scaling
if VNA1 flag: RL[i] -= 10                              (0x49c83b)
```
- The same formula serves both modes. In transmission the RL array is simply labelled "T.L." / "Transmission Loss(dB)".
- The value is a positive loss in dB.
- At the end of the per-point work, `if RL[i] < 0 then RL[i] = 0` (0x49cadc). rho and everything derived from it are computed before this clamp, from the unclamped value.

Phase sign (0x49caf0-0x49ccb3). The detector only gives 0..180°. The sign is synthesised from the slope of phase against frequency:
```
a[i] = ArcTan2(im, re)            (equals phase in radians, since rho > 0)
for i >= 2: s[i] = 1 if a[i] < a[i-1]; 0 if a[i] > a[i-1]; s[i-1] if equal
            (i = 1 only initialises prev)
de-glitch, i = 1..N-1: if s[i] differs from both neighbours and the neighbours agree, s[i] = s[i-1]
s[0] = s[1]; s[N] = s[N-1]
if phase180 flag == 0 (360° mode): where s[i] == 0, phase[i] = -phase[i] and Xs[i] = -Xs[i]
```
So rising phase with frequency gives negative phase and negative Xs; falling phase gives positive. In 180° mode (the default) phase and Xs stay unsigned.

Cross-check with the real readings (nothing connected, reflection):
- Phase: 11 gives 1.94°, 127 gives 22.35°.
- Level with no calibration (calOff = 0): 364 gives 1.19 dB, 380 gives 2.13 dB.
- Level after a reflection calibration taken in that same open state (calOff = 344 - rawB_cal): about 0.016 dB at every point, so SWR is about 1085.
- The 0 dB point of the scale is count 343.7.

---
2. Derived values (Convert, 0x49c846-0x49cadc; certain)

```
rho  = 1 / 10^(RL/20)                    (RL after cal offset and VNA1, before the clamp)
SWR  = |(1 + rho) / (1 - rho)|           (0x49c871; no guard for rho = 1, no upper clamp)
re   = cos(phase°) * rho ;  im = sin(phase°) * rho     (degrees to radians: divide by 57.2957795130823)
D    = (1 - re)^2 + im^2
|Xs| = |2*im / D| * 50 + 2^-100          (0x49c8be-0x49c943; the 2^-100 is Power(2,-100), a divide-by-zero guard)
Rs   = |(1 - re^2 - im^2) / D| * 50      (0x49c946-0x49c9ef; note the abs)
L    = 1e6  * |Xs| / (2*pi*freq)   µH    (0x49c9ff; 0 if Xs == 0)
C    = 1e12 / (2*pi*|Xs|*freq)     pF    (0x49ca4d)
|Z|  = sqrt(Xs^2 + Rs^2)                 (0x49ca90)
```
- 2*pi is the constant 6.28318530717959. freq is the displayed frequency in Hz, so it includes the extender multiplier.
- L and C are both computed from |Xs| before the sign step.

"XL µH" field (Display, 0x4a0939-0x4a0b22). Filled only when both marker checkboxes TraceM1 and TraceM2 are on; otherwise the label is "---" and the fields are empty.
- If C[M1] > C[M2]: labels "| Xs | +j" and "Xl to µH"; shows L for M1 and M2.
- Otherwise: labels "| Xs | -j" and "Xc to pF"; shows C[M1], C[M2].
- Quirk: the µH branch reads `L[M - 46]` (operand `[aL + M*8 - 0x170]`, at 0x4a09da and 0x4a0a26), not L[M]. The pF branch reads C[M]. For M < 46 it reads the tail of the Xs array. INFERRED: an original bug; a rewrite should use L[M].

Marker fields SWR, |Z|, RL, phase, Rs, Xs are array[M] formatted with 2 decimals (0x4a0346-0x4a04f1); frequency is freq/1e6 with 6 decimals.

SWR-min marker (0x4a0f41): index of the smallest SWR over i = 1..N, starting from a threshold of 200; index 0 if nothing is below 200.

---
3. Calibration

Table at 0x4b28dc, 4008 bytes (0xfa8):
```
+0      dword  magic 0x01CB2AE8 (decimal 30092008)
+4      byte   reflection done
+5      byte   transmission done
+8      R[1..1000]  smallint
+0x7d8  T[1..1000]  smallint
```
Saved LZ-compressed to <app dir>\IG_miniVNA.cal by BpVnaOk3Click (0x48f7dc). Loaded in FormCreate (0x4a55b5-0x4a5707): if the file is missing, the message "Please Calibrate your miniVNA in first" is shown; if the magic is wrong, the table is zeroed.

Acquisition (form TFormCalibrage; the menu picks which step is shown, then Ok3 saves):
- Transmission, BpVnaOk1Click 0x48f4c4: Sweep(fMin, fMax, N = 1000, mode 1). The form text is "Reliez au plus court DUT et DET" (through connection).
- Reflection, BpVnaOk2Click 0x48f64c: Sweep(fMin, fMax, N = 1000, mode 0). INFERRED from the string table: the instruction is "Retirez toute connection entre DUT et DET" (open port).
- In both, for i = 1..1000: `table[i] = 344 - rawB[i]`, or `511 - rawB[i]` if the VNA1 flag is set (0x48f592 / 0x48f711).
- Level only. Phase is never calibrated anywhere.
- Point 0 is not used. Calibration point i is at fMin + i*round((fMax - fMin)/1000), which is 100 kHz + i*179.9 kHz by default.
- The calibration sweep goes through the ordinary Sweep routine, so it is also divided by the current extender multiplier and honours the firmware-bug option.

Application: nearest point, no interpolation. This is inside Sweep (0x49c21d-0x49c39b) and runs only when the reload flag 0x4acdd8 is set. The flag is set at startup, when the start, stop or samples edit boxes change, after saving a calibration, and after loading a .VNA file.
```
g    = fMax / 1000                         (180000 Hz by default)
pos  = (start' / mult) / g                 (start' is the start AFTER the firmware-bug subtraction, i.e. start - step when the option is on)
inc  = ((stop / mult) / g - pos) / N
for i = 1..N: calOffR[i] = R[round(pos)]; calOffT[i] = T[round(pos)]; pos += inc
calOffR[0] = calOffR[1]; calOffT[0] = calOffT[1]
```
- The per-sweep offsets start at pos for i = 1, so they are shifted by about one sample relative to freq[i].
- The lookup grid is f/180 kHz, while the calibration points sit at 100 kHz + i*179.9 kHz. That is a small built-in mismatch.
- Convert then adds calOff to rawB as in section 1.

DDS calibration (TFormDDS; FormShow 0x491e1c, helper 0x491d78, OK 0x492010):
- It puts the device in generator mode at an editable frequency (default 10 MHz), using a trial coefficient that starts as ddsCoef.
- Eight buttons add or subtract 0.001, 0.0001, 0.00001, 0.000001 to the trial coefficient and re-send.
- OK stores the trial value in ddsCoef (the Hz to DDS-word factor) and writes it to IG_miniVNA.ini, section "Configure", key "Coeficient DDS", as coef*1e9. Startup reads it back and divides by 1e9 (0x4a3ae6).
- It changes nothing else.

---
4. Sweep command (Sweep, 0x49bd94; certain)

```
stepHz = Round((stop - start) / N)        integer Hz, stored as a single (CalcStep 0x49bbc4); also shown as "(Step : n Hz)"
if "Compenser le bug du firmware" is checked: start = start - stepHz        (0x49be62)
FlushIn; FlushOut; ReadAvail (discarded); Sleep(1)
send "1\r" if mode != 0 else "0\r"; Sleep(10)
send str(Round(start / mult * ddsCoef)) + "\r"; Sleep(10)
send str(N + 2) + "\r"; Sleep(10)          always N+2, with or without the bug option
send str(Round(stepHz / mult * ddsCoef)) + "\r"; Sleep(10)
t0 = Now
loop: buf += ReadAvail, until len(buf) >= (N+2)*4
      if Now - t0 > 0.0006 day (51.84 s): MessageDlg "No receipt of the data since the material ! (1)",
         stop the loop timer, return false
```
- Round is the x87 fistp (round half to even). Numbers are sent as plain decimal integers.
- The wait is a busy loop with no Sleep. There are no retries.
- The progress label computes len/(N+2)*100 capped at 100, so it reaches 100% at a quarter of the data (cosmetic bug).

Parsing, 1-based byte offsets, for i = 0..N+1:
```
off = 4*i + 1 (+4 if the bug option is on)
rawA[i] = b[off] | b[off+1] << 8
rawB[i] = b[off+2] | b[off+3] << 8
if bug option: rawA[0] = rawA[1]; rawB[0] = rawB[1]
```
No 10-bit masking is applied.

"Compensate firmware step bug" (menu item at form+0x554; ini key "Compensate firmware Step Bug", default "1" = on):
- The sweep starts one step low and the first returned point is dropped.
- Displayed point 0 is then overwritten with point 1.
- Net effect: the device's first two points are never shown.
- With the option off, raw[i] is device point i and nothing is dropped.
- NOT DETERMINED: what the firmware bug actually is. Only the workaround is in the code.

Extender x4 / x8 / x16 (0x4a941c, 0x4a95e4, 0x4a97b4; "Non" 0x4a9260):
- Sets mult to 4, 8 or 16 and the edit boxes to 4-720, 8-1440 or 16-2880 MHz; changes the title text; enables or disables band-preset menu items.
- All displayed and stored frequencies (freq[], L, C, CSV) are the multiplied ones.
- Only the DDS words (start and step) and the calibration lookup are divided by mult.
- ReadParams limits become fMin*mult to fMax*mult.
- The Enable Freq Ext menu item (0x4aae8c) only shows or hides the multiplier submenu.

After a single sweep, ParkAndClose (0x49c4b4) sends "0\r", "10\r", "1\r", "0\r" with Sleep(1) between them (mode 0, start word 10, count 1, step 0), then closes the port without reading the 4-byte reply.

Continuous mode (BpMesureBoucleClick 0x4a713c, TimerBoucleTimer 0x4a64f0):
- The port stays open. ReadParams runs once. The first sweep is immediate.
- Each timer tick does: disable timer, Sweep, Convert, Display, re-enable timer. ParkAndClose is sent only on stop.
- No Interval is set in the DFM, so INFERRED: the TTimer default of 1000 ms.
- On failure: "Error Read Data to comport ! (10)." and the port is closed.

Port: component TAfComPort with InBufSize and OutBufSize 8192. NOT DETERMINED: where baud and framing are set (not in the main form's DFM).

N180 / N360 (0x4a99f0 / 0x4a9a10): set the phase180 flag to 1 / 0, then re-run Convert and Display. See the sign logic in section 1. The Phase menu is only made visible when the ini has F4CLB=OUI (0x4a3b44).

VNA11 (0x4aad6c; ini key "VNA1"):
- Toggles the VNA1 flag.
- On: fMin = 1 MHz, fMax = 65 MHz, edit boxes "1" and "65", RL gets -10 dB, calibration uses 511 - raw.
- Off: fMin = 100 kHz, fMax = 180 MHz, edit boxes "1" and "180".
- Because fMax changes, the calibration grid (fMax/1000) changes too.

ReadParams (0x49cd18):
- Start and stop are MHz text times 1e6, rounded.
- Start is accepted only if it is >= fMin*mult - 0.1, <= fMax*mult and <= stop; stop likewise and >= start. Otherwise the previous text is restored.
- An empty start becomes str(mult); an empty stop becomes str(180*mult).
- Samples are clamped to 10..1000.

---
5. Cable length and loss

Length (Display 0x4a0c8b, UI mode 2; also EditVeloChange 0x4aaa7c):
```
L(m) = 3e8 / (|freq[M1] - freq[M2]| + 1e-7) / 2 * velo - 0.045      (2 decimals)
```
velo defaults to 0.66. EditVeloChange stores the new velo and recomputes.

"L (m) -> Velo." (BpCoefLongClick 0x4aacc8 opens FormSaisie in mode 4; its OK handler is at 0x494e56):
```
velo = (Lknown + 0.045) / (3e8 / (|f1 - f2| + 1e-7) / 2)
```
Written to EditVelo with 2 decimals, which triggers EditVeloChange.

"Mesure Automatique" (BpMesureAutoClick 0x4aa500):
1. Set start "1", stop "180", samples "500"; uncheck M1 and M2.
2. Two passes. Each pass: Sleep(200), click BpUneMesure, Sleep(50), then scan with index i starting at 0. For k = 1..3:
   - advance i until 85 < |phase[i]| < 95 (loop bound i < N-2); call that index hit;
   - then advance until |phase[i]| < 84 or > 96 (leaves the band);
   - if either scan reaches N-2, EditLong shows "!!!" and the routine aborts.
3. Pass 1: start box = freq[hit, k=1]/1e6 - 1; stop box = freq[hit, k=3]/1e6 + 1 (3 decimals). This zooms onto the first to third 90° crossings.
4. Pass 2: M1 = hit for k=1; M2 = hit for k=3.
5. Refine each marker: best = |phase[M]| (not |phase - 90|; original quirk); for j = 1..10, if ||phase[M+j]| - 90| < best, take off = j and update best; M += off.
6. Display then computes the length from M1 and M2. The first to third 90° crossing is one full phase period.

Cable loss (Display 0x4a0d23, UI mode 1, reflection sweep). It uses only the two marker points, not the whole sweep:
```
a = RL[M1]/2; b = RL[M2]/2
Mini = min(a, b); Maxi = max(a, b); Moyenne = (a + b)/2      (2 decimals)
```
SetupMode puts M1 = round(N/3) and M2 = round(N*0.66) on every mode change (0x49dc24).

---
6. Bandwidth and Q

"Du marqueur 1 a 2" panel (Display 0x4a0b81-0x4a0c86; only when both M1 and M2 are checked, otherwise blank):
```
BW = |freq[M1] - freq[M2]|                       (Hz, 0 decimals)
Q  = ((freq[M1] + freq[M2]) / 2) / (BW + 0.0001)  (2 decimals)
```
There is no -3 dB search. The user places the markers.

BpCalculeClick (0x4aae30) is not a BW/Q routine. It checks TraceM1, redraws, and opens TFormCalcule, an LC resonance calculator:
- BpEgaleClick (0x498618): `result = 1 / (3.948e-05 * fM1_MHz^2 * value)`. With the combo on "µH" (index 0) the input is µH and the result is shown as pF; otherwise the input is pF and the result is µH. Default value "15".
- BpM1SwrClick (0x4989a8): M1 = index of min SWR over 1..N (threshold 200).
- BpM1dBMaxClick (0x498aec): M1 = index of max RL over 1..N (starting from -10).
- BpM1dBminClick (0x498c30): not read; INFERRED: index of min RL.

---
7. Generator mode

SendGene (0x49c588), called from the generator form (0x492848) and from the DDS form (0x491d78):
```
FlushIn; FlushOut; Sleep(1)
"0\r"; Sleep(10)
str(Round(freqHz * ddsCoef)) + "\r"; Sleep(10)
"1\r"; Sleep(1)
"0\r"; Sleep(1)
```
- This is mode 0, start word, count 1, step 0. The 4-byte reply is not read.
- It is re-sent on every frequency change (digit +/- buttons from 1 Hz to 1e9 Hz).
- Default frequency 10 MHz, clamped to fMin*mult..fMax*mult.
- Quirk: the word is not divided by mult, although the limits are scaled by it.
- Closing the form (BpFermerClick 0x492b0c) calls ParkAndClose.

---
8. File formats

CSV (MSauverCSVClick 0x4a81dc):
- Separator is always ",". DecimalSeparator is forced to "." during the export and restored afterwards.
- Header line: `"Frequency(Hz)","Return Loss(dB)","Phase(deg)"`. In transmission mode the middle column is `"Transmission Loss(dB)"`.
- Rows i = 0..N: FloatToStr(freq[i]) (integer Hz), RL[i] fixed 2 decimals, phase[i] fixed 2 decimals.
- Phase is signed only in 360° mode.

.VNA (MSauverVnaClick 0x4a7904, MChargerVnaClick 0x4a7c2c). An 8040-byte (0x1f68) record, LZ-compressed:
```
+0x00   dword  magic 0x01CB2AE8 (load rejects anything else: "miniVNA file incompatible !")
+0x04   dword  startHz
+0x08   dword  stopHz
+0x0C   word   N
+0x0E   byte   mode
+0x10   word   M1 index (0xFFFF if the M1 box is unchecked)
+0x12   word   M2 index (same)
+0x14   bytes  trace checkboxes: SWR, Z, RL, Phase, Rs, Xs
+0x1A   byte   Echelle30
+0x1C   single RL graph offset
+0x20   rawA[0..1000]     words
+0x7F2  rawB[0..1000]
+0xFC4  calOffR[0..1000]
+0x1796 calOffT[0..1000]
```
- Only entries 0..N are filled.
- The file stores raw readings plus the per-point calibration offsets; Convert is re-run on load.

.cal: the 4008-byte table in section 3, compressed the same way.

Compression container (Tlzrw1 component, the "!HZL" header): NOT DETERMINED. I did not read the Compress/Decompress code at 0x478d48 / 0x478d50.

---
9. Things a rewrite would get wrong

- No averaging or smoothing of readings anywhere. One reading per point.
- The count sent is N+2 and (N+2)*4 bytes are awaited; N+1 points (0..N) are used. With the bug option on (the default), two device points are effectively discarded (section 4).
- stepHz is an integer, Round((stop - start)/N), so freq[N] can differ slightly from stop.
- The level calibration is an additive count offset (344 - raw_cal, or 511 - raw_cal for VNA1), looked up by nearest 1/1000-of-fMax bin, about one sample off, and refreshed only when the sweep parameters change.
- Phase is never calibrated.
- RL is clamped to >= 0 only after rho, SWR and Z have been computed from the unclamped value. A negative RL gives rho > 1, and the abs() calls hide it in SWR and Rs.
- SWR has no upper clamp. |Xs| always has 2^-100 added. Rs is an absolute value.
- The phase and Xs sign are a slope heuristic with a 1-sample de-glitch, applied only in 360° mode. In the default 180° mode Xs is always >= 0 and the plot uses |phase|.
- L or C in the marker panel is chosen by comparing C at M1 and M2, not by sign, and the L read is offset by 46 samples (section 2).
- The "Decalage" RL offset (0x4acdfc, accepted between -60 and 60, otherwise reset to 0) is added to the plotted RL trace and to the axis labels (0x49e138, 0x49f81b). The marker read-outs and the CSV use the unshifted RL. It is stored in the .VNA file.
- Limits: samples 10..1000; start/stop within fMin*mult..fMax*mult; receive timeout 51.84 s; no retry.
- Number formatting in the fields uses ffNumber (thousands separators) with 2 decimals.
- Ini file IG_miniVNA.ini, section "Configure": keys "Coeficient DDS" (coef*1e9), "Compensate firmware Step Bug", "DDS Hz" (fMax), "VNA1", "F4CLB".
