# Spectral resynthesis review package

The package contains the two reference Definitions, their source assets, inspect output, MIDI and absolute-frame event fixtures, technical renders, regression renders for the existing Generator families, and machine measurements.

Machine checks performed by the generator:

- Spectral A/B preparation, Stereo output, FFT 2048, Morph, and all five Spectral control parameters.
- Spectral plus Additive, Sample, and Noise with Layer, Voice, and Global Processor chains and Modulation routes.
- MIDI render, absolute-frame parameter changes, 16-voice rendering, one-voice stealing, supported block sizes, supported sample rates, Fresh Runtime reproducibility, and the reported latency impulse position.
- Existing Oscillator, Noise, Sample, Granular, Wave Sequence, Wavetable, Operator Modulation, Additive, and Formant reference renders.
- Identity SNR / error / correlation, transition and spectral-flux measurements, Shift and Pitch spectrum estimates, Morph boundary measurements, and high-note near-Nyquist energy are recorded in metrics.json.
- Release performance measurements for 1, 4, 8, and 16 Stereo voices with FFT 2048 and Morph enabled. Performance audio is kept outside the package.

Human listening checklist:

- [ ] Spectral note has a stable, clearly differentiated stereo image.
- [ ] Position and Freeze remain stable when held.
- [ ] Blur changes temporal definition without clicks.
- [ ] Shift changes pitch without changing scan duration.
- [ ] Morph moves smoothly between A and B.
- [ ] Hybrid layers remain distinguishable and the processor chain remains controlled.
- [ ] MIDI timing, velocity, and external controls are musically usable.
