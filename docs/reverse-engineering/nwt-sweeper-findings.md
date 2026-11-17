# The 35-4400 MHz sweeper ("NWT" protocol): what the unit showed

Found by talking to one unit on 2026-11-17: an aluminium-cased "35-4400 MHz spectrum
analyzer / sweep source / power meter" (LTDZ family), one detector input, one output.
Nothing here comes from documentation; each line was seen on the device. Driver:
`src/device/nwt.rs`. Hardware check: `cargo run --example nwtcheck /dev/ttyUSB0`.

## Connection

- USB chip: CH340 (vendor `1a86`, product `7523`), no serial number, Linux driver `ch341`.
- 57600 baud, 8 data bits, no parity, 1 stop bit. It sends nothing on its own.

## Commands

Every command starts with the byte `0x8F`, then one letter, then decimal digits (ASCII).
Frequencies are in units of 10 Hz.

| Command | Bytes after `0x8F` | Answer |
|---|---|---|
| Version | `v` | 1 byte: 119 |
| Sweep | `a` + start (9 digits) + step (8 digits) + number of points (4 digits) + 2 digits | 4 bytes per point |
| One reading | `m` | 4 bytes |
| Generator | `f` + frequency (9 digits) | none |

- The sweep command needs all 23 digits. With 21 (the form found in descriptions of the
  older NWT units) the box waits silently for the rest; the next bytes sent complete it.
- The last two digits are not understood. `00` and `20` gave the same readings.
- A point is the detector reading as a 16-bit number, low byte first, sent twice (the
  protocol has two channels, this unit has one input).
- Speed: about 1.3 ms per point; 1001 points in 1.2 to 1.5 s.
- **Steps of 10 MHz or more are not honoured.** The unit ignores the part of the step
  from 10 MHz up: asked for 43.65 MHz it steps by 3.65 MHz, asked for 11 MHz by 1 MHz,
  asked for 10, 20 or 30 MHz it stays on the start frequency. Largest usable step:
  9.99999 MHz. The driver therefore sweeps a coarser request several times finer and
  keeps every n-th point (`nwt::thinning`).
- Readings with nothing connected: 30 to 62 counts, rising to about 148 between roughly
  125 and 193 MHz. That rise stays at the same frequency for every step size and start
  frequency tried, which is how the step limit above was found and the fix checked.
- Unlike the miniVNA it was never seen to hang. The program still never writes to it
  while a sweep is being sent: a stop reads the rest of the sweep away first.

## Not yet known

- **dB per count.** The program uses 0.1 dB per count until the detector's slope is
  measured with an attenuator of known value (`nwt_db_per_count` in `calibration.toml`).
  dB values are approximate until then.
- **The reading with output joined to input.** Needed as the 0 dB reference
  (Configure, Calibrate Reference). Until it is measured, 600 counts is assumed.
- Whether frequencies really are in units of 10 Hz: start and step are consistent with
  each other (the rise above sits at the same place for every start and step), but the
  absolute scale is to be confirmed with a filter of known frequency or a receiver.
- The generator command is accepted (no answer, the box stays responsive); the output
  was not checked with a receiver.
