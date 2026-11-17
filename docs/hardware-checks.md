# Checks on the real device

Device: original mRS miniVNA (not Pro) on `/dev/ttyUSB0`, 2026-11-17.
Programs: `examples/calibrate_open.rs`, `examples/hwcheck.rs`, `examples/repeat.rs`.

## Done, with nothing connected to DUT and DET

| Check | Expected | Measured | Result |
|---|---|---|---|
| Reflection calibration, then a sweep 1–180 MHz | loss within ±0.2 dB of 0 | −0.04 to 0.07 dB before clamping, lowest SWR 233 | pass |
| Continuous sweeping for one minute | no error | 115 sweeps of 500 points, no error, loss stayed −0.04 to 0.07 dB | pass |
| Original's calibration command (negative start word `-857920`) against ours (`1073742`), points 2..1001 against 1..1000 | within ±2 counts | level: at most 1–2 counts in five runs. Phase: at most 1 count in four runs, 15 counts in one run | pass for level (the only value calibration uses); see note |
| Step-bug option off / on, 1–180 MHz, 500 points | off: point 0 is an outlier; on: none | off: phase 87, 12, 12, 12 …; on: phase 12, 12, 12, 12 … | pass |

Note on the one run with a 15-count phase difference: it was the first command after the port
had been closed by one minute of continuous sweeping and reopened. Four later runs did not
repeat it (two identical sweeps also differ by at most 1 count). Not explained. Phase is not
used by the calibration table.

## Stopping a sweep half way (2026-11-17, afternoon)

**The miniVNA must not be sent anything while it is still sending a sweep.** A first version
of "Stop" sent the parking command 0.4 s into a 1000-point sweep. The next sweep came back
garbled (levels 100..231 instead of 363..407), the one after got no answer, and the device
then ignored everything, including single filler fields, until its USB cable was unplugged
and plugged in again.

The fix (`Device::drain`): after a stop, the rest of the interrupted sweep is read away
before anything else is sent. With that, `examples/stoptest.rs` stopped a 1000-point sweep
three times; each stop took 1.34 s (the device finishes its sweep, the result is thrown
away) and each following 500-point sweep was normal (levels 363..407, phase 11..249).

| Check | Expected | Measured | Result |
|---|---|---|---|
| Shorted DUT port against open port | phase near 180 degrees at low frequency instead of 0 | short: phase 1023 falling to 684 counts, level 348..354; open: phase 12 rising to 132, level 365..380 | pass |
| Stop half way through a sweep, three times, then sweep | next sweep normal | see above | pass with `drain`; hangs the device without it |

## Still to do: each needs something connected

| Check | Expected | Result |
|---|---|---|
| 50-ohm load on DUT | SWR below 1.3 across HF, return loss above 18 dB | not done |
| Antenna or other known load, side by side with the original under CrossOver | within 0.5 dB and 2 degrees at three frequencies | not done |
| Open-ended cable of known length, Cable Length → "Find automatically" | within 5 % of the real length | not done |
| DUT joined to DET, Calibrate Transmission, sweep in Transmission mode | loss within ±0.3 dB of 0 | not done |

## Not checked by hand

Menus, dialogs, mouse and keyboard actions in the window were not clicked through: there was
no pointer automation in the build session. Their logic is covered by unit tests, and the
window was checked by screen capture with the simulated and the real device.
