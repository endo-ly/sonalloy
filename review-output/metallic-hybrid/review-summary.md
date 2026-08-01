# Metallic Hybrid Sound Review

## Reference

- Definition: metallic-hybrid.json
- Source implementation commit: 9702f24ede1903cfaf00d6d2e771d51d74134123
- Asset: metal-hit.wav
- MIDI inputs: metallic-hybrid-phrase.mid, metallic-hybrid-pitch-range.mid, metallic-hybrid-velocity.mid
- Sample rate: 48,000 Hz
- Render block size: 257 frames
- Rendered output format: stereo 32-bit float WAV
- Source asset format: mono PCM16 WAV

## Automatic checks

- All generated samples are finite: pass
- Rendered peaks stay within the float WAV range: pass
- Rendered outputs have no adjacent-frame discontinuity candidates over 0.25: pass
- All rendered outputs remain reproducible across block sizes 64, 257, and 1024: pass
- Sample renders are finite and non-silent: pass
- Hybrid Mix differs from Oscillator-only: pass
- Missing-asset fallback remains non-silent: pass
- Inspect reports show expected Sample Layer state and only expected asset diagnostics: pass
- Source asset hashes match the copied Definitions: pass
- Maximum rendered-output absolute peak: 0.341414
- Maximum rendered-output adjacent-frame difference: 0.134538
- Source asset metrics are retained as 01-sample-source.wav in metrics.json
- Missing asset render retains the oscillator layer: 09-missing-asset-fallback.wav
- 02-sample-decoded-root.wav and 03-sample-pitch-range.wav are not loudness-matched to the source; they use the sample-only layer gain and include one-shot silence.
- 06-hybrid-mix.wav starts the sample and oscillator layers from one Note On and is intended to sound like one integrated instrument.

## Listening intent

| File | Intent | Expected result |
|---|---|---|
| 01-sample-source.wav | Present the source fixture without engine processing. | A short, bright metallic transient with a low body tail; this is the reference for the following sample renders. |
| 02-sample-decoded-root.wav | Check WAV decode, hash validation, 44.1 kHz to 48 kHz preparation, and root-note playback at C4. | The same character and pitch as the source, without a new click or an obvious resampling artifact. |
| 03-sample-pitch-range.wav | Check one-shot playback at C3, C4, and C5 using the C4 root note. | The attack moves one octave down and up while remaining recognizably the same sample and ending cleanly. |
| 04-oscillator-only.wav | Isolate the sine body used by the instrument. | A stable pitched tone with the intended envelope and no metallic attack layer. |
| 05-sample-only.wav | Isolate the sample layer across the musical phrase. | Each note has a distinct metallic attack and the layer does not contribute a sustained pitched body. |
| 06-hybrid-mix.wav | Compare the combined sample attack and oscillator body at the reference note. | The attack supplies brightness and definition while the body supplies pitch and sustain; the two layers sound like one instrument. |
| 07-velocity-response.wav | Check the four velocity levels from quiet to loud. | Louder notes become stronger and brighter in a continuous, musical way without clicks or disproportionate jumps. |
| 08-musical-phrase.wav | Evaluate the complete instrument in a short phrase with overlapping notes and varied velocities. | The hybrid remains clear, playable, and balanced through the phrase. |
| 09-missing-asset-fallback.wav | Confirm the body remains usable when the attack asset cannot be loaded. | The metallic attack disappears, but the oscillator body continues to render normally and the output stays clean. |

## Listening record

| Item | Result | Notes |
|---|---|---|
| Source and decoded root sound natural |  |  |
| Pitch range is usable |  |  |
| Sample ending is free of an audible click |  |  |
| Attack layer has a clear transient role |  |  |
| Body layer provides pitch and sustain |  |  |
| Solo layers combine into one instrument |  |  |
| Velocity response is natural |  |  |
| Musical phrase is usable |  |  |
| Hybrid value is clear |  |  |
| Instrument is ready for use |  |  |

## Files

The audio, copied Definitions, MIDI inputs, source asset, metrics, and this record are kept in this directory.
