# Testdata Generation

`testdata/`のFixture（MIDI、Audio Asset）を決定的に再生成するScriptを置く。Review Packageの生成Script（`review/generate/`）からも、まずここでFixtureを最新化してから参照する。

| Script | 生成物 |
|---|---|
| `generate_midi_fixtures.py` | `testdata/midi/`の固定MIDI入力（Basic Poly Synth、Metallic Hybrid、Expressive Hybrid Lead） |
| `generate_metallic_hybrid_inputs.py` | `testdata/assets/metal-hit.wav`と`testdata/midi/metallic-hybrid-*.mid` |
| `generate_spectral_reference_assets.py` | `testdata/assets/spectral-reference-*.wav`（Stereo source、Latency impulse） |
| `generate_granular_textures.py` | `testdata/assets/stereo-texture.wav`と`testdata/assets/mono-texture.wav`（Granular Review入力の質感Source） |

```bash
python3 testdata/generate/generate_midi_fixtures.py
python3 testdata/generate/generate_metallic_hybrid_inputs.py
python3 testdata/generate/generate_spectral_reference_assets.py
python3 testdata/generate/generate_granular_textures.py
```

いずれも固定成分・固定Seedから生成するため、実行するたびに同じ結果になる。
