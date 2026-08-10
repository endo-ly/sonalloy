# Example audio assets

`digital-motion.wav` is the source for the wavetable reference instrument.

`spectral-reference-a.wav` and `spectral-reference-b.wav` are deterministic stereo sources containing harmonic, moving-harmonic, and noise-texture material for the Spectral reference instruments. `spectral-reference-impulse.wav` is a deterministic stereo impulse source for latency verification. They are regenerated with:

```sh
python3 scripts/review/generate_spectral_reference_assets.py
```
