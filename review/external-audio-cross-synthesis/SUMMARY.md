# External Audio Cross Synthesis Review

## Automated Review

- 7 fixture definitions were validated and rendered through the CLI external-audio path.
- Generated inputs are deterministic PCM16 WAV files at 48 kHz and 96 kHz. Their frame counts and SHA-256 values are recorded in metrics.json.
- The package records product Analysis and Inspect JSON, Envelope Follower Trace JSON, Full Cross Synthesis block-size differences, Reset comparison data, External alignment, and runtime resource metrics.
- The automated checks cover Envelope Follower, External Sidechain, Vocoder mono/stereo behavior, Envelope Transfer, Spectral Morph startup and parameter stages, a combined chain, and a 96 kHz Full Cross Synthesis render.

## Human Review

未試聴。音質、Speech-like articulation、Sidechainの聴感、Stereo定位、Morphの連続性は、人間が同じ再生条件で試聴して確認してください。
