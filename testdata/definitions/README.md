# Definition Fixtures

- `valid-basic-poly-synth.json` is a valid Definition fixture.
- `invalid-schema-version.json` is rejected by validation because its schema version is newer than the current schema.

Programmatic range（MIDI Key / Velocityの0〜127上限を含む）、duplicate-ID、unknown-field、enabled Layer位置、`NaN` / `Infinity` cases are covered by the Core unit tests because JSON cannot represent the latter two values.
