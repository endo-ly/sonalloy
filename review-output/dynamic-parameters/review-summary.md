# Dynamic Parameter Review

## Inputs

- Moving Hybrid Pad
- Expressive Hybrid Lead
- Event Sequence with Parameter Change, Pitch Bend, Mod Wheel, Aftertouch, Random Pan, and Resonance Control
- MIDI fixture with Pitch Bend, CC1, Channel Aftertouch, and Key Tracking notes

## Human listening items

- `01-parameter-cutoff.wav`: Parameter Changeの位置、Smoothing、Click、変化量
- `02-lfo-filter.wav`: LFOの周期、位相、滑らかさ、Block Size差
- `03-envelope-pitch.wav`: Attack、Decay、Release、Pitchの連続性
- `04-random-pan.wav`: Noteごとの左右差、同じ入力での再現、極端な偏り
- `05-external-controls.wav`: Pitch Bend、Mod Wheel、Aftertouchの反映とSmoothing
- `06-voice-stealing.wav`: Steal Fade、Pending Note、LFO / Envelope初期化、Click
- `07-musical-phrase.wav`: 4〜8小節相当の音色としての使いやすさ
- `08-key-tracking.wav`: 低音から高音までのCutoff変化と音域の自然さ
- `09-resonance-control.wav`: Resonance変化の安定性、発散、Click

## 判定基準

- すべての音源で明確なClick、NaN、Infinity、異常な音量落ちがない
- LFOとEnvelopeが階段状や不連続に聞こえない
- Pitch BendでSampleとOscillatorの音程変化が一致する
- Random PanがNoteごとに変化し、同じ入力では再現する
- Key TrackingとResonanceが意図したTargetだけを変化させる
- Voice Stealing後に新しいNoteのSource Stateが前のVoiceから混ざらない
- Reference Instrumentを実際の音色として使用できる

自動測定値は同じディレクトリの`metrics.json`に保存する。音質の判定はこの記録だけでは完了しない。
