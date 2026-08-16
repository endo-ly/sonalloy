# Sonalloy AI Instrument Authoring and Render Diagnostics

- **Repository:** `endo-ly/sonalloy`
- **Canonical product requirements:** `docs/CONCEPT.md`
- **Primary implementation areas:** Instrument Definition, parameter/modulation contract, compiler, runtime modulation evaluation, `instrument inspect`, offline render diagnostics, agent skill
- **Purpose:** Give an implementation agent a complete, decision-free plan for making Sonalloy instrument definitions numerically meaningful to AI and making rendered behavior observable without reading runtime source code or creating ad-hoc analysis scripts.
- **Delivery model:** Implement as one coherent change set. The Definition change, Inspect expansion, and Render diagnostics share the same parameter contract and should land together.

---

## 0. Position of this plan

Sonalloy already has the core pieces required for AI-driven instrument creation: a JSON Instrument Definition, a compiled parameter catalog, modulation sources/routes, deterministic offline rendering, machine-readable inspection, and review-time WAV metrics. The missing piece is a complete authoring feedback loop.

Today an agent can write valid JSON, but some numbers are indirect enough that the agent must inspect source code to understand their physical effect. After rendering, the agent receives a WAV but has little structured information about what the runtime actually did. This encourages external Python scripts, duplicate formulas, approximate pitch trackers, and source-code inspection during normal sound design.

This plan changes that workflow to:

```text
intent
  ↓
write Definition using meaningful numeric units
  ↓
validate
  ↓
inspect exact compiled contract and route effect
  ↓
render
  ├─ audio analysis
  └─ selected parameter trace
  ↓
refine Definition
  ↓
final human listening review
```

The goal is not automatic aesthetic judgment. Sonalloy should report facts that it can know exactly or measure deterministically: parameter units, modulation depth, source ranges, effective values, clamp behavior, level, spectrum, stereo behavior, continuity, and timing. Human listening remains the final authority for whether a sound is musically good.

### 0.1 Implementation priority

When implementation choices conflict, use this order:

1. `docs/CONCEPT.md`
2. The contracts fixed by this plan
3. One source of truth for parameter and modulation math
4. Realtime safety of the normal runtime path
5. Deterministic, block-partition-independent behavior
6. Machine readability for AI and other frontends
7. Simplicity and maintainability

Do not add abstractions for hypothetical future features. Do not preserve the old Definition syntax through aliases, fallback parsing, migrations, or deprecated fields.

### 0.2 Permanent naming

Do not put temporary phase names, migration terminology, or this plan's implementation sequence into runtime type names, diagnostics, code comments, CLI output, or canonical product documentation. Permanent names should describe the product concepts only.

---

# 1. Current baseline and the exact problem

## 1.1 Existing strengths

The current code already has:

- `InstrumentDefinition` with `schema_version`
- `ParameterDescriptor` with stable ID, owner, unit, scale, min/max/default, and smoothing duration
- `ParameterScale::Linear` and `ParameterScale::Log2`
- compiled `ParameterHandle` lookup before entering the audio path
- built-in and user-defined modulation sources
- compiled modulation routes
- runtime route evaluation and final target clamping
- `instrument inspect --json`
- `RenderedAudio` held in memory before WAV writing
- `realfft` / `rustfft` dependencies already in `sonalloy-core`
- review-time WAV metrics in `review/generate/measure_wav.py`

The implementation therefore does not need a new DSP subsystem.

## 1.2 Main authoring defect: route `amount`

The current Definition stores:

```json
{
  "source": "vibrato",
  "target": "layer.body.tuning",
  "amount": 0.02,
  "curve": "linear"
}
```

`amount` is a fraction of the target's complete parameter range. For linear parameters the runtime uses:

```text
delta = curved_source × amount × (max - min)
```

For Log2 parameters it uses:

```text
log2_delta = curved_source × amount × log2(max / min)
effective = base × 2 ^ sum(log2_delta)
```

This is efficient internally, but it is a poor authoring representation. An agent that wants a 20-cent vibrato must know the tuning parameter's entire `-1200..+1200` range and calculate `20 / 2400`. An agent that sees cutoff `amount: 0.2` must know that the value operates in a Log2 domain rather than as 20% of Hertz.

The Definition should express author intent directly.

## 1.3 Secondary authoring defect: normalized algorithm-strength values

Fields such as these are structurally understandable but numerically under-explained:

- `waveshaping.amount`
- `phase_distortion.amount`
- `wavefold.amount`
- `feedback.amount`
- `drive.amount`
- `reverb.decay`
- `reverb.damping`
- `granular.randomness`
- `additive.inharmonicity`
- `operators[].modulation_amount`

The JSON names identify the feature, so the schema does not need broad renaming. The missing part is a precise numeric contract: endpoints, neutral points, and the implemented mapping where the algorithm has one.

## 1.4 Observation defect after rendering

`instrument inspect` reports compiled structure, but an agent cannot directly answer questions such as:

- How many cents can this route move tuning?
- Does this envelope raise cutoff by two octaves or by an arbitrary normalized amount?
- Did the route hit the target clamp?
- What was the actual source value at 80 ms?
- What was the final cutoff/tuning/pan after all routes were summed?
- What were the output peak, RMS, DC, stereo correlation, spectral centroid, and discontinuity metrics?

The runtime already owns most of the exact values. The renderer already owns the complete output audio. Those facts must be exposed instead of reconstructed externally.

---

# 2. Scope

## 2.1 Included

### Definition contract

- bump Instrument Definition schema to version 2
- replace route `amount` with explicit modulation `depth`
- define an explicit modulation unit contract
- convert Event Sequence `parameter_change` from normalized authoring values to native parameter values
- document exact semantics of ambiguous normalized generator/processor fields
- update all repository Definitions, presets, fixtures, review packages, examples, tests, and the agent skill

### Parameter/modulation core

- derive the modulation unit from each `ParameterDescriptor`
- compile authoring depth into the runtime modulation domain
- simplify runtime modulation formulas around direct depth
- centralize shared modulation evaluation math used by voice targets, global targets, Inspect calculations, and Trace calculations

### `instrument inspect`

- expose each parameter's native unit and modulation unit
- expose maximum meaningful modulation depth
- expose source value range and polarity
- expose each route's explicit depth
- expose the route's static effect range
- expose the target's reachable range from its Definition default, including whether the range can clamp

### `render note`, `render events`, `render midi`

- optional deterministic audio analysis
- optional selected-parameter runtime trace
- machine-readable JSON output for both
- human-readable summary output

### Review/agent workflow

- reuse the product analysis contract where review generation already controls the render call
- update `.agents/skills/create-instrument/SKILL.md` so agents use Inspect/Analysis/Trace before inventing external measurement scripts

## 2.2 Explicitly outside this change

- aesthetic scoring such as “good”, “warm”, “professional”, “80s-like”, etc.
- neural audio embeddings or learned audio classifiers
- arbitrary internal DSP-state tracing
- per-layer or per-note stem WAV export
- a general-purpose audio editor or visualization UI
- a generic pitch tracker that claims a fundamental for arbitrary/polyphonic/noisy audio
- realtime audio device support
- realtime MIDI device support
- plugin APIs
- MSEG, additional modulation sources, or new DSP generators/processors
- changing the fixed audio routing topology
- preserving Definition schema version 1
- automatic Definition migration

---

# 3. Instrument Definition schema version 2

## 3.1 Version policy

Change:

```rust
pub const CURRENT_SCHEMA_VERSION: u32 = 2;
```

Repository-owned Definitions must be updated in the same change.

Rules:

- version 1 is rejected as unsupported
- do not add `serde(alias = "amount")`
- do not add an untagged legacy route variant
- do not add a migration command
- do not keep a second runtime formula for old routes
- tests for removed v1 behavior are unnecessary; keep one explicit rejection test for unsupported schema version as part of the existing schema-version contract

## 3.2 Replace `amount` with explicit `depth`

Replace:

```rust
pub struct ModulationRouteDefinition {
    pub source: String,
    pub target: String,
    pub amount: f32,
    pub curve: ModulationCurve,
}
```

with:

```rust
pub struct ModulationRouteDefinition {
    pub source: String,
    pub target: String,
    pub depth: ModulationDepthDefinition,
    pub curve: ModulationCurve,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulationDepthDefinition {
    pub value: f32,
    pub unit: ModulationUnit,
}
```

The JSON contract is:

```json
{
  "source": "vibrato",
  "target": "layer.body.tuning",
  "depth": {
    "value": 20.0,
    "unit": "cents"
  },
  "curve": "linear"
}
```

For an envelope that opens a filter by two octaves:

```json
{
  "source": "filter_env",
  "target": "voice.processor.tone.cutoff",
  "depth": {
    "value": 2.0,
    "unit": "octaves"
  },
  "curve": "smooth_step"
}
```

For velocity reducing gain by 9 dB at source value 1:

```json
{
  "source": "velocity",
  "target": "layer.body.gain",
  "depth": {
    "value": -9.0,
    "unit": "decibels"
  },
  "curve": "linear"
}
```

Negative values preserve direction. Bipolar sources naturally apply both positive and negative excursions around the base. Unipolar sources move from the base in one direction determined by the depth sign.

## 3.3 `ModulationUnit`

Add a public serialized enum near the parameter contract:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationUnit {
    Decibels,
    Pan,
    Cents,
    Hertz,
    Seconds,
    PerSecond,
    Index,
    DecibelsPerOctave,
    Normalized,
    Octaves,
}
```

Do not overload `ParameterUnit`. `ParameterUnit` describes the native value stored by the target. `ModulationUnit` describes a signed change applied by a route.

### Mapping rule

`ParameterDescriptor` must expose:

```rust
pub fn modulation_unit(&self) -> ModulationUnit
```

Use the following fixed mapping:

| Parameter unit | Scale | Modulation unit |
|---|---|---|
| Decibels | Linear | Decibels |
| Pan | Linear | Pan |
| Cents | Linear | Cents |
| Hertz | Linear | Hertz |
| Hertz | Log2 | Octaves |
| Ratio | Log2 | Octaves |
| Seconds | Linear | Seconds |
| Seconds | Log2 | Octaves |
| PerSecond | Linear | PerSecond |
| PerSecond | Log2 | Octaves |
| Index | Linear | Index |
| DecibelsPerOctave | Linear | DecibelsPerOctave |
| Normalized | Linear | Normalized |

Any combination not present in the table is a programming error in parameter registration and must be covered by unit tests.

For Log2 parameters, `octaves` is a generic base-2 logarithmic depth. `+1` doubles the native value and `-1` halves it. For a frequency this is literally one octave; for seconds, rates, or ratios it is the same mathematically explicit doubling/halving domain.

## 3.4 Maximum modulation depth

Add:

```rust
pub fn max_modulation_depth(&self) -> f32
```

Rules:

```text
Linear: max - min
Log2:   log2(max / min)
```

This preserves the expressive range that old `amount = ±1` provided while presenting it in meaningful units.

Compiler validation after target resolution must require:

```text
depth.value is finite
abs(depth.value) <= descriptor.max_modulation_depth()
depth.unit == descriptor.modulation_unit()
```

Use target-aware compiler diagnostics because Definition-only validation does not yet know the resolved target descriptor.

Add diagnostics:

```text
ROUTE_DEPTH_INVALID
ROUTE_DEPTH_UNIT_INVALID
```

Diagnostic examples:

```text
modulation.routes[2].depth.value
modulation.routes[2].depth.unit
```

Diagnostic detail should include the expected unit and allowed magnitude when useful.

## 3.5 Compiled route contract

Replace compiled `amount` with direct domain depth:

```rust
pub struct CompiledRoute {
    pub source: CompiledSourceRef,
    pub target: ParameterHandle,
    pub depth: f32,
    pub curve: ModulationCurve,
}
```

The unit is not required in the audio path because the target descriptor/scale establishes the evaluation domain at compile time. Do not perform string or enum-unit matching in the audio loop.

## 3.6 Runtime formula

After curve shaping:

### Linear target

```text
contribution = curved_source × depth
effective_unclamped = base + sum(contribution)
effective = clamp(effective_unclamped, min, max)
```

### Log2 target

`depth` is stored in octaves:

```text
octave_contribution = curved_source × depth
effective_unclamped = base × 2 ^ sum(octave_contribution)
effective = clamp(effective_unclamped, min, max)
```

The target's total parameter range is no longer multiplied into each route at runtime.

Examples that must hold exactly before clamping:

```text
Tuning base 0 cents, bipolar LFO depth 20 cents:
source +1 → +20 cents
source  0 →   0 cents
source -1 → -20 cents

Cutoff base 1000 Hz, unipolar envelope depth +2 octaves:
source 0   → 1000 Hz
source 0.5 → 2000 Hz
source 1   → 4000 Hz

Grain size base 80 ms, unipolar envelope depth -1 octave:
source 0 → 80 ms
source 1 → 40 ms
```

## 3.7 Event Sequence parameter values

The authoring problem also exists in `render events`, where `parameter_change` currently stores a normalized `0..1` value.

Change Event Sequence JSON from:

```json
{
  "absolute_frame": 12000,
  "type": "parameter_change",
  "parameter": "voice.processor.tone.cutoff",
  "normalized": 0.35
}
```

to:

```json
{
  "absolute_frame": 12000,
  "type": "parameter_change",
  "parameter": "voice.processor.tone.cutoff",
  "native_value": 2400.0
}
```

`native_value` is expressed in the parameter's `ParameterUnit`, which is available through `instrument inspect`.

CLI compilation of Event Sequence must:

1. resolve the parameter ID
2. obtain its `ParameterDescriptor`
3. validate `native_value` with `descriptor.normalize(native_value)`
4. store the normalized result in the existing `ProcessEventKind::ParameterChange`

The Core event remains normalized. This keeps frontend/runtime control transport independent from authoring representation.

Do not keep the old `normalized` Event Sequence field.

---

# 4. Make ambiguous numeric fields explicit enough for AI

## 4.1 General rule

Do not rename every normalized field. A value such as `morph: 0.25`, `mix: 0.5`, or `stereo_correlation: 0.8` is reasonable when the endpoints are explicit.

For each normalized or algorithm-specific field, canonical documentation and the agent skill must state at least one of these contracts:

1. explicit endpoints (`0 = A`, `1 = B`)
2. a neutral point (`0.5 = unchanged`)
3. exact implemented mapping when the number drives an algorithm rather than a simple interpolation

Avoid vague documentation such as “higher means stronger” when the implementation has a precise formula.

## 4.2 Required semantic clarifications

Update `docs/instrument-definition.md` and `.agents/skills/create-instrument/SKILL.md` with a compact table covering at minimum:

### Oscillator complex controls

- `waveshaping.amount`
  - `0` is bypass
  - current shape coefficient is `1 + amount × 3`
  - wet shaper is normalized `tanh(shape × x) / tanh(shape)`
  - final output crossfades from dry to that wet value by `amount`

- `phase_distortion.amount`
  - `0` is identity
  - phase breakpoint is `0.5 - amount × 0.45`
  - `1` moves the breakpoint to `0.05`

- `wavefold.amount`
  - `0` is bypass
  - DaisySP wavefolder drive is `1 + amount × 7`
  - wet amount passed to the wavefolder is the same `amount`

- `feedback.amount`
  - `0` disables feedback
  - feedback phase contribution uses the previous sample and the current implemented mapping
  - document the exact mapping from runtime rather than describing it as a generic percentage

### Drive

- `drive.amount`
  - `0` is identity
  - shape coefficient is `amount × 4`
  - wet signal is normalized tanh saturation
- `drive.mix`
  - `0` dry
  - `1` wet
  - intermediate values are the implemented linear dry/wet interpolation

### Positional / interpolation values

- `morph`: identify A and B endpoints
- `position`: identify beginning/end of the relevant source domain
- `stereo_correlation`: `0` independent, `1` identical
- `pan_spread` / unison spread: identify center and maximum configured spread
- `freeze`: identify free traversal and fully frozen behavior
- `formant.throat`: keep the existing neutral-point explanation (`0.5` is unchanged bandwidth)

### Operator modulation

Keep `operators[].modulation_amount`, but define it by mode using the actual runtime formulas:

- Phase: modulator signal × amount contributes to the summed modulation; the current runtime turns that summed modulation into the implemented phase offset. State the exact factor.
- Frequency: summed modulation scales instantaneous operator frequency through the current runtime formula; do not call it a canonical FM index if the implementation is not mathematically that definition.
- Amplitude: each incoming modulator contributes `1 + output × amount` to the amplitude multiplier before the runtime clamp.
- Ring: amount crossfades the current carrier signal toward the carrier×modulator product.

The goal is that an agent can choose a reasonable starting number without opening `runtime/generator/operator.rs`.

## 4.3 No duplicate semantic source of truth in code

Do not introduce a large natural-language description registry into the audio runtime solely to duplicate documentation. Numeric metadata belongs in the parameter contract and Inspect. Algorithm explanations belong in canonical docs and the agent skill.

---

# 5. Centralize modulation evaluation

## 5.1 Reason

Voice target evaluation and global target evaluation currently repeat the same base-denormalization, route iteration, curve shaping, range-domain accumulation, and clamp behavior. Trace and Inspect must not add a third and fourth formula.

Refactor the math before adding observation features.

## 5.2 Pure evaluation helper

Place the shared math in `crates/sonalloy-core/src/runtime/modulation.rs` or another existing modulation-owned module. Keep it small and data-oriented.

Conceptual API:

```rust
pub(crate) struct RouteContribution {
    pub(crate) source_raw: f32,
    pub(crate) source_shaped: f32,
    pub(crate) domain_delta: f32,
}

pub(crate) struct EvaluatedParameterValue {
    pub(crate) base: f32,
    pub(crate) domain_sum: f32,
    pub(crate) unclamped: f32,
    pub(crate) final_value: f32,
    pub(crate) clamped: bool,
}
```

The realtime path does not need to allocate or retain `RouteContribution` values. Implement the core formula as pure scalar helpers that both runtime and diagnostics can call.

Required helpers should cover:

- curve shaping
- one route's domain contribution: `shaped_source × depth`
- combining the domain sum with a base value according to `ParameterScale`
- final clamp and finite check

For example:

```rust
fn route_domain_delta(source: f32, depth: f32, curve: ModulationCurve) -> f32;

fn apply_domain_sum(
    descriptor: &ParameterDescriptor,
    base: f32,
    domain_sum: f32,
) -> Result<EvaluatedParameterValue, ProcessError>;
```

Voice and global evaluators should differ only in how they resolve a route's source value.

## 5.3 Realtime constraints

Normal `InstrumentRuntime::process` must continue to:

- allocate nothing after `prepare`
- perform no string lookup
- perform no JSON serialization
- perform no logging for each sample/span
- keep route order deterministic

Diagnostics must be optional offline work built around the same scalar math, not a reason to add `Vec::push`, maps, or formatted strings to the audio path.

---

# 6. `instrument inspect` expansion

## 6.1 Purpose

Inspect should answer “what does this Definition mean after compilation?” without requiring source-code reading.

Keep both current human-readable output and `--json`. JSON is the canonical machine interface.

## 6.2 Parameter report

Extend each parameter entry from the current fields to:

```json
{
  "id": "layer.body.tuning",
  "owner": { "layer": { "definition_index": 0 } },
  "unit": "cents",
  "scale": "linear",
  "min": -1200.0,
  "max": 1200.0,
  "default": 0.0,
  "smoothing_seconds": 0.005,
  "modulation": {
    "unit": "cents",
    "max_abs_depth": 2400.0
  }
}
```

For cutoff:

```json
{
  "id": "voice.processor.tone.cutoff",
  "unit": "hertz",
  "scale": "log2",
  "min": 20.0,
  "max": 20000.0,
  "default": 9500.0,
  "modulation": {
    "unit": "octaves",
    "max_abs_depth": 9.965784
  }
}
```

Do not duplicate min/max/default under multiple nested objects.

## 6.3 Source report

Every source included in compiled routes must report its numeric domain.

Add:

```json
{
  "id": "vibrato",
  "scope": "voice",
  "kind": "lfo",
  "value_range": {
    "min": -1.0,
    "max": 1.0,
    "polarity": "bipolar"
  },
  "waveform": "sine",
  "rate_hz": 5.0,
  "phase": 0.0
}
```

Fixed source ranges:

| Source | Min | Max | Polarity |
|---|---:|---:|---|
| Velocity | 0 | 1 | unipolar |
| Key Tracking | -1 | 1 | bipolar |
| LFO | -1 | 1 | bipolar |
| Modulation Envelope | 0 | 1 | unipolar |
| Random | -1 | 1 | bipolar |
| Pitch Bend | -1 | 1 | bipolar |
| Mod Wheel | 0 | 1 | unipolar |
| Aftertouch | 0 | 1 | unipolar |

Use a small serialized enum for polarity rather than free text.

## 6.4 Route report

Replace Inspect's `amount` output with the Definition-level depth:

```json
{
  "source": "vibrato",
  "target": "layer.body.tuning",
  "curve": "linear",
  "depth": {
    "value": 20.0,
    "unit": "cents"
  },
  "source_range": {
    "min": -1.0,
    "max": 1.0
  },
  "effect": {
    "kind": "additive",
    "min_delta": -20.0,
    "max_delta": 20.0,
    "unit": "cents"
  }
}
```

For Log2 targets:

```json
{
  "source": "filter_env",
  "target": "voice.processor.tone.cutoff",
  "curve": "smooth_step",
  "depth": {
    "value": 2.0,
    "unit": "octaves"
  },
  "source_range": {
    "min": 0.0,
    "max": 1.0
  },
  "effect": {
    "kind": "multiplicative",
    "min_octaves": 0.0,
    "max_octaves": 2.0,
    "min_factor": 1.0,
    "max_factor": 4.0
  }
}
```

`SmoothStep` preserves the endpoint range, so static route range calculation can use the source endpoints.

## 6.5 Reachable range from the Definition default

Add one target-level summary per parameter that has routes. Name it `modulated_range_from_default` in the parameter entry, not a separate duplicated catalog.

Linear example:

```json
"modulated_range_from_default": {
  "unclamped_min": -20.0,
  "unclamped_max": 20.0,
  "effective_min": -20.0,
  "effective_max": 20.0,
  "may_clamp": false
}
```

Log2 example with cutoff default 9500 Hz and +2-octave unipolar envelope:

```json
"modulated_range_from_default": {
  "unclamped_min": 9500.0,
  "unclamped_max": 38000.0,
  "effective_min": 9500.0,
  "effective_max": 20000.0,
  "may_clamp": true
}
```

The range is a deterministic bound assuming each source may independently reach its declared endpoint. State this in `docs/cli.md`; do not present it as a prediction of a specific performance.

## 6.6 Human-readable Inspect

Human output should add compact information instead of dumping raw JSON structure. Example:

```text
parameter layer.body.tuning
  base: 0 cents
  range: -1200..1200 cents
  modulation: cents, max depth ±2400

route vibrato -> layer.body.tuning
  source: bipolar -1..1
  depth: 20 cents
  effect: -20..20 cents
```

For Log2 targets, print the factor:

```text
route filter_env -> voice.processor.tone.cutoff
  source: unipolar 0..1
  depth: +2 octaves
  effect: ×1..×4
  default reachable: 9500..20000 Hz (clamped; unclamped max 38000 Hz)
```

---

# 7. Render audio analysis

## 7.1 Core placement

Add:

```text
crates/sonalloy-core/src/analysis.rs
```

This module analyzes `RenderedAudio` and has no file-format responsibility. It should be usable by CLI and tests.

The repository already depends on `realfft` / `rustfft`; do not add another FFT dependency.

Public conceptual API:

```rust
pub struct AudioAnalysisOptions {
    pub reference_frequency_hz: Option<f32>,
}

pub struct AudioAnalysis { ... }

pub fn analyze_rendered_audio(
    audio: &RenderedAudio,
    options: AudioAnalysisOptions,
) -> Result<AudioAnalysis, AudioAnalysisError>;
```

Keep types serializable so CLI does not need to duplicate the report structure.

## 7.2 CLI options

Add these options to `render note`, `render events`, and `render midi`:

```text
--analyze
```

`--json` remains the switch that serializes the complete success report.

When `--analyze` is absent, do not perform FFT analysis or build analysis structures.

## 7.3 Analysis metrics

Return this shape conceptually:

```json
{
  "sample_rate": 48000,
  "channels": 2,
  "frames": 48001,
  "duration_seconds": 1.0000208,
  "finite": true,
  "level": {
    "peak": 0.652,
    "peak_dbfs": -3.714,
    "rms": 0.181,
    "rms_dbfs": -14.849,
    "crest_factor_db": 11.135,
    "over_full_scale": false
  },
  "dc": {
    "left": 0.00002,
    "right": 0.00001
  },
  "activity": {
    "threshold_dbfs": -80.0,
    "first_frame": 2,
    "peak_frame": 1440,
    "last_frame": 45010
  },
  "continuity": {
    "large_delta_threshold": 0.25,
    "max_adjacent_frame_delta": 0.021,
    "large_delta_count": 0,
    "first_large_delta_frames": []
  },
  "stereo": {
    "correlation": 0.63
  },
  "spectrum": {
    "fft_size": 4096,
    "hop_size": 2048,
    "spectral_centroid_hz": 1820.0,
    "peaks": [
      { "frequency_hz": 261.7, "relative_power": 1.0 }
    ],
    "reference_frequency_hz": 261.6256,
    "harmonic_energy_ratio": 0.84
  }
}
```

### Level

Compute on all interleaved channel samples unless a field is explicitly per-channel.

- `peak`: maximum absolute sample
- `rms`: root mean square
- dBFS: `20 * log10(value)`
- when linear value is zero, serialize dBFS as `null`; never emit `-inf` or NaN in JSON
- `crest_factor_db = peak_dbfs - rms_dbfs` when both exist
- `over_full_scale = peak > 1.0`; use this wording because float WAV is not physically clipped merely because it exceeds 1.0

### DC

Report per-channel arithmetic mean. The current output contract is stereo; keep the analysis structure capable of the channel count present in `RenderedAudio` rather than hard-coding left/right internally. CLI may serialize stereo-friendly names if desired, but a small array is preferable for core generality.

### Activity

Use a fixed documented threshold of `-80 dBFS` for initial product behavior.

Report:

- first frame whose per-frame maximum absolute channel value crosses the threshold
- frame of overall peak
- last frame crossing threshold

Use `null` for first/last/peak when audio is silent.

This is a signal-activity measure, not an ADSR detector. Do not label it `attack_seconds` or `release_seconds`.

### Continuity

Reuse the current review convention:

- max adjacent-frame delta across channels
- threshold `0.25`
- number of frames above threshold
- first up to 16 candidate frame indices

### Stereo correlation

Use normalized Pearson-style zero-mean correlation between left and right channels.

- `+1`: strongly same-direction correlated
- `0`: uncorrelated
- `-1`: opposite polarity
- `null` when the denominator is zero (silence/constant signal)

### Spectrum

Use deterministic STFT power averaging rather than the current review script's slow direct DFT.

Fixed initial settings:

```text
fft_size = min(4096, largest supported power-of-two <= frames)
hop_size = fft_size / 2
window = Hann
channel input = average of channels for spectral summary
spectrum aggregation = mean power by bin across analysis windows
```

For very short audio where a useful FFT cannot be formed, return an empty peak list and `null` spectral metrics rather than fabricating values.

Report:

- spectral centroid from averaged power
- eight strongest local-maximum bins, ordered by power descending
- relative power normalized to the strongest reported peak

### Harmonic reference

Do not implement a generic “fundamental frequency detector” in this change.

For `render note`, provide the equal-tempered frequency of the requested MIDI note as `reference_frequency_hz`:

```text
440 × 2 ^ ((note - 69) / 12)
```

Use that only to compute a harmonic-energy ratio around integer multiples with a documented tolerance. Name it a reference frequency, not a detected fundamental.

For `render events` and `render midi`, leave `reference_frequency_hz` and harmonic ratio `null` unless a future explicit option supplies one. Do not guess a fundamental from a polyphonic phrase.

## 7.4 Analysis timing

Run analysis after latency correction so metrics describe the WAV that is actually written to disk.

The order for CLI render becomes:

```text
compile
  ↓
render extended latency-aware request
  ↓
remove reported latency prefix
  ↓
optional analyze
  ↓
write WAV
  ↓
print report
```

Analysis failure must fail the render command rather than silently omit requested diagnostics.

---

# 8. Runtime parameter trace

## 8.1 Purpose and boundary

Trace reports selected author-visible Dynamic Parameters. It does not dump arbitrary oscillator phase, filter state, delay buffers, FFT frames, grains, or internal native handles.

Valid trace targets are existing `ParameterDescriptor` IDs.

## 8.2 CLI contract

Add repeatable options to all three instrument render commands:

```text
--trace <PARAMETER_ID>
--trace-every-frames <N>
```

Rules:

- `--trace` may be supplied multiple times
- default `--trace-every-frames` is `480`
- `N` must be greater than zero
- unknown trace parameter IDs are CLI input errors before rendering
- duplicate IDs are de-duplicated while preserving first-request order
- `--trace-every-frames` without `--trace` is an input error
- trace is optional; normal renders pay no trace collection cost

JSON is the primary interface. Human mode may print a compact table, but do not suppress the functionality outside `--json`.

## 8.3 Trace sampling semantics

Trace points represent runtime state **after processing up to the reported absolute frame**.

Collect:

1. a baseline point at frame 0 before performance events
2. periodic points at `N`, `2N`, `3N`, ...
3. one post-event point as soon as at least one frame has been processed after each event boundary when that point is not already represented
4. the final rendered frame

Keep points sorted and de-duplicate identical frame positions.

Latency correction must also be reflected in trace frame numbers. The final public trace timeline starts at the same frame 0 as the corrected WAV. Internal pre-roll/latency frames must not appear as negative user frames.

## 8.4 Do not make trace the source of audio changes

Trace-enabled rendering may split process blocks at additional observation boundaries, but it must use the same runtime and event ordering.

Add a regression test that renders the same request with trace disabled and enabled and verifies audio equivalence within the repository's existing block-partition tolerance. If trace changes audible output, fix partition dependence rather than adding a special trace-only DSP path.

## 8.5 Runtime snapshot access

Add non-allocating current-value accessors required to build snapshots outside the hot sample loop:

- `Smoother::current()`
- current external control values
- current voice assignment: voice index, note ID, note number, velocity, voice state
- current value for each used voice source
- whether a Definition layer is currently active for that voice

For modulation envelopes, expose the current envelope value from `AdsrRuntime` without advancing it. For LFO, derive the current value from its current phase without advancing phase.

Do not make these broad public mutation APIs. Keep them at the narrowest visibility required by Core render diagnostics.

## 8.6 Trace evaluation uses shared modulation math

At each observation point, evaluate the selected target from:

```text
current base Parameter state
+ current route source values
+ route curve
+ compiled direct depth
+ target scale
+ clamp
```

Use the same scalar helpers introduced in section 5.

Do not call DSP generator/processors to reconstruct values.

## 8.7 Voice and global scope

For target owners associated with a voice (`Layer`, `LayerGenerator`, `LayerProcessor`, `VoiceProcessor`), produce one observation per active voice for which the target is meaningful.

Each observation includes:

```json
"voice": {
  "index": 0,
  "note_id": 1,
  "note_number": 60,
  "velocity": 100,
  "state": "active"
}
```

For `GlobalProcessor` targets, `voice` is `null` and one instrument-level observation is produced.

Layer-owned targets should be omitted for a voice while that layer is inactive. Do not report fabricated zero values for an inactive layer.

## 8.8 Trace record shape

Linear target example:

```json
{
  "frame": 4800,
  "seconds": 0.1,
  "parameter": "layer.body.tuning",
  "unit": "cents",
  "voice": {
    "index": 0,
    "note_id": 1,
    "note_number": 60,
    "velocity": 100,
    "state": "active"
  },
  "base": 0.0,
  "routes": [
    {
      "source": "pitch_env",
      "raw": 0.61,
      "shaped": 0.61,
      "depth": {
        "value": -480.0,
        "unit": "cents"
      },
      "contribution": {
        "value": -292.8,
        "unit": "cents",
        "factor": null
      }
    }
  ],
  "before_clamp": -292.8,
  "final": -292.8,
  "clamped": false
}
```

Log2 target example:

```json
{
  "frame": 4800,
  "seconds": 0.1,
  "parameter": "voice.processor.tone.cutoff",
  "unit": "hertz",
  "voice": { "index": 0, "note_id": 1, "note_number": 60, "velocity": 100, "state": "active" },
  "base": 1000.0,
  "routes": [
    {
      "source": "filter_env",
      "raw": 0.5,
      "shaped": 0.5,
      "depth": {
        "value": 2.0,
        "unit": "octaves"
      },
      "contribution": {
        "value": 1.0,
        "unit": "octaves",
        "factor": 2.0
      }
    }
  ],
  "before_clamp": 2000.0,
  "final": 2000.0,
  "clamped": false
}
```

For multiple routes, preserve Definition route order in the `routes` array. This mirrors the deterministic runtime addition order.

## 8.9 Trace size guard

Trace is diagnostic output and can grow quickly with polyphony.

Before execution, calculate an upper bound from:

```text
requested trace target count
× prepared polyphony
× periodic observation count
```

Reject obviously excessive requests with a CLI input diagnostic instead of risking unbounded memory use. Use an initial hard cap of **100,000 trace observations**. Event-boundary observations count toward the final runtime collection; if the cap is reached unexpectedly due to dense events, stop with an explicit trace-size error rather than truncating silently.

Add a dedicated diagnostic or CLI message such as:

```text
TRACE_LIMIT_EXCEEDED
```

The cap is a product safety limit, not a fallback path.

---

# 9. Render success report and CLI structure

## 9.1 Success JSON

Extend the current render `SuccessReport` with optional fields:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
analysis: Option<AudioAnalysis>,

#[serde(skip_serializing_if = "Option::is_none")]
trace: Option<RenderTraceReport>,
```

Existing metadata remains:

- status
- sample rate
- channel count
- frames
- reported latency
- output path
- backend version
- diagnostics

Do not create a second top-level JSON format for diagnostic renders.

## 9.2 Human output

With `--analyze`, print a compact summary after normal render success:

```text
analysis
  peak: -3.71 dBFS
  rms: -14.85 dBFS
  centroid: 1820 Hz
  stereo correlation: 0.63
  activity: frames 2..45010 at -80 dBFS threshold
  large discontinuities: 0
```

With `--trace`, print one compact table per selected parameter. Human formatting is secondary; preserve all detail in JSON.

## 9.3 Keep `main.rs` changes focused

The current CLI entry point is already large. Do not perform a broad unrelated refactor in this change. If the new report serialization/formatting would materially inflate it, add exactly one focused module such as:

```text
crates/sonalloy-cli/src/report.rs
```

and keep command argument parsing/execution in `main.rs`.

Do not move unrelated MIDI, init, validation, or existing generator inspection code merely for style.

---

# 10. Repository Definition migration

## 10.1 Update all owned JSON

Use repository-wide search, not a hand-maintained file list.

Update every Instrument Definition in:

- `presets/`
- `review/**/definitions/`
- `testdata/`
- crate test fixtures
- documentation examples
- generated expected JSON where Definition structure is asserted

For every file:

1. set `schema_version` to `2`
2. replace route `amount` with `depth`
3. convert the old numeric intent into the equivalent direct depth using the target descriptor
4. preserve the previous sound as closely as possible before any deliberate retuning

Conversion formulas for repository migration only:

```text
old Linear amount → new depth
old_amount × (max - min)

old Log2 amount → new depth in octaves
old_amount × log2(max / min)
```

Do not keep these formulas as runtime compatibility logic. A temporary one-off development script may be used during implementation and deleted before merge if it has no continuing product value.

## 10.2 Event Sequence files

Replace every repository event:

```json
"normalized": X
```

with equivalent:

```json
"native_value": Y
```

Use each target descriptor's current `denormalize(X)` mapping to compute `Y`.

Delete any one-off migration helper before completion unless it is intentionally promoted to a permanent supported tool, which this plan does not require.

## 10.3 Preset sound preservation

Definition syntax changes should not accidentally retune presets.

For repository presets/review Definitions that only undergo equivalent route conversion:

- render before/after from the same commit baseline while developing
- compare WAVs under the existing deterministic tolerance
- investigate any audible or material numeric difference

If a preset needs deliberate adjustment because the old route semantics were actually misleading, treat that as an explicit sound-design decision and re-review the affected WAV. Do not hide a sound change inside the schema conversion.

---

# 11. Review metric integration

## 11.1 Product analysis becomes the primary common metric contract

The new Core `AudioAnalysis` owns the common deterministic metrics used by product render diagnostics:

- finite
- peak/RMS
- DC
- continuity
- stereo correlation
- spectral centroid / peaks
- optional harmonic reference ratio

Where review generation invokes Sonalloy itself to create a WAV, update that invocation to request `--analyze --json` and reuse the returned analysis instead of recalculating the same common metrics independently.

## 11.2 Keep review-specific operations where they belong

`review/generate/measure_wav.py` may continue to own operations that are specifically about comparing files or package fixtures, such as:

- sample-by-sample WAV comparison
- known event-boundary differences
- package-specific aggregation

Remove or stop using duplicated basic metric implementations once the same metric is consumed from the Core report. Avoid two different definitions of “RMS”, “spectral centroid”, or “large discontinuity” in active review generation.

Do not add a general `analyze-wav` command in this change solely to preserve the Python script's current calling pattern.

---

# 12. Agent skill and canonical documentation

## 12.1 `.agents/skills/create-instrument/SKILL.md`

Change the standard workflow to:

```text
init → edit → validate → inspect → render/analyze/trace → refine
```

The skill must teach:

- write route depth in target-meaningful units
- use Inspect to verify unit, source polarity, route effect, and clamp range before rendering
- use `render --analyze` for output-level/spectral/stereo/continuity facts
- use `--trace` for questions about modulation movement and final effective parameter values
- use human listening for aesthetic quality

Add a strong operational rule:

> If Sonalloy Inspect, Analysis, or Trace already exposes the required fact, use that product interface. Do not read runtime source code or create an external Python signal-analysis implementation merely to reconstruct the same fact.

This rule does not ban external tools for genuinely unsupported research or one-off human analysis.

## 12.2 `docs/instrument-definition.md`

Update:

- schema version
- Modulation Route JSON
- `depth` units and formulas
- source polarity table
- Event Sequence references if present
- exact numeric semantics table from section 4
- examples across linear and Log2 targets

Do not repeat full CLI output examples here; link responsibility to `docs/cli.md`.

## 12.3 `docs/runtime-processing.md`

Update the runtime modulation formula to direct depth and explain:

- linear domain summation
- Log2 octave-domain summation
- Definition route order
- clamp after summation
- smoothing applies to the base parameter state as currently designed

Trace CLI formatting belongs in `docs/cli.md`, not runtime documentation.

## 12.4 `docs/cli.md`

Document:

- expanded Inspect parameter/source/route contracts
- `native_value` in Event Sequence
- `--analyze`
- `--trace`
- `--trace-every-frames`
- trace observation semantics
- analysis metric definitions and null behavior
- JSON examples

## 12.5 README

Keep the README change small. Update the AI-first usage story to show that the intended loop includes Inspect and optional render diagnostics. Do not duplicate the detailed metric reference.

---

# 13. Detailed implementation sequence

Implement in this order so each step leaves one source of truth for the next step.

## Step 1 — Introduce the new modulation authoring contract

Files expected to change:

```text
crates/sonalloy-core/src/definition.rs
crates/sonalloy-core/src/parameter.rs
crates/sonalloy-core/src/diagnostics.rs
crates/sonalloy-core/src/compiler.rs
crates/sonalloy-core/src/generator_parameters.rs   # only if helper imports/contracts require it
```

Tasks:

1. set schema version 2
2. add `ModulationUnit`
3. add `ModulationDepthDefinition`
4. replace route `amount` with `depth`
5. add `ParameterDescriptor::modulation_unit()`
6. add `ParameterDescriptor::max_modulation_depth()`
7. change compile validation to target-aware depth/unit validation
8. replace `CompiledRoute.amount` with `depth`
9. add/rename route-depth diagnostics

At the end of Step 1, compilation may still fail in runtime code because it still references `amount`; proceed directly to Step 2 before broad fixture migration.

## Step 2 — Replace runtime range-fraction math with direct depth

Files:

```text
crates/sonalloy-core/src/runtime/modulation.rs
crates/sonalloy-core/src/runtime/voice.rs
crates/sonalloy-core/src/runtime/instrument.rs
```

Tasks:

1. centralize curve and scalar route evaluation
2. remove repeated `(max - min)` / `log2(max/min)` multiplication from voice/global runtime evaluators
3. evaluate direct linear or octave depth
4. keep route order unchanged
5. keep final clamp unchanged
6. keep all process-time structures preallocated
7. verify normal audio-thread allocation tests still pass

Do not add trace yet. First make the normal runtime's new contract correct and tested.

## Step 3 — Change Event Sequence authoring values

Files:

```text
crates/sonalloy-cli/src/main.rs
crates/sonalloy-cli/tests/cli.rs
```

Tasks:

1. replace Event Sequence `normalized` with `native_value`
2. resolve descriptor and call `normalize(native_value)` during CLI event compilation
3. return field-path diagnostics for non-finite/out-of-range values
4. keep Core `ProcessEventKind::ParameterChange.normalized`

## Step 4 — Migrate repository Definitions and establish sound-equivalent baseline

Use repository-wide search.

Tasks:

1. update schema version
2. convert route amounts using the formulas in section 10
3. convert Event Sequence values
4. update compile/unit/CLI fixtures
5. run validation across all presets and review Definitions
6. render key review fixtures and verify no unintended sound change

Do this before Inspect work so subsequent Inspect tests only need the final schema.

## Step 5 — Expand Inspect

Files likely involved:

```text
crates/sonalloy-cli/src/main.rs
crates/sonalloy-cli/tests/cli.rs
crates/sonalloy-core/src/parameter.rs
crates/sonalloy-core/src/compiler.rs   # only if stable source metadata is missing
```

Tasks:

1. parameter modulation metadata
2. source range/polarity metadata
3. route depth and route effect
4. reachable range from default
5. human-readable formatting
6. JSON integration tests

Static Inspect calculations must call shared contract helpers rather than reimplement runtime formulas from source text.

## Step 6 — Add Core audio analysis

Files:

```text
crates/sonalloy-core/src/analysis.rs        # new
crates/sonalloy-core/src/lib.rs
crates/sonalloy-core/Cargo.toml             # no new dependency expected
```

Tasks:

1. report types and errors
2. level/DC/activity metrics
3. continuity metrics
4. stereo correlation
5. deterministic STFT summary using existing FFT dependencies
6. optional reference-harmonic metric
7. unit tests with generated deterministic signals

Keep WAV decoding/writing out of this module.

## Step 7 — Wire `--analyze` into all instrument render commands

Files:

```text
crates/sonalloy-cli/src/main.rs
crates/sonalloy-cli/src/report.rs            # optional focused new module
crates/sonalloy-cli/tests/cli.rs
```

Tasks:

1. add CLI args
2. analyze after latency correction
3. use MIDI note reference only for `render note`
4. add optional `analysis` to success JSON
5. human summary

Verify `--analyze` does not change WAV bytes.

## Step 8 — Add current-state runtime snapshot support

Files likely involved:

```text
crates/sonalloy-core/src/runtime/smoothing.rs
crates/sonalloy-core/src/runtime/adsr.rs
crates/sonalloy-core/src/runtime/voice.rs
crates/sonalloy-core/src/runtime/instrument.rs
crates/sonalloy-core/src/trace.rs             # new
crates/sonalloy-core/src/lib.rs
```

Tasks:

1. read-only current smoother/envelope/source values
2. read-only active voice identity/state
3. selected-parameter snapshot calculation using shared modulation helpers
4. trace report structs
5. no mutation during snapshot

Keep visibility narrow. Do not expose internal mutable runtime state as a public debugging API.

## Step 9 — Integrate trace collection into offline rendering

Files:

```text
crates/sonalloy-core/src/render.rs
crates/sonalloy-core/src/trace.rs
crates/sonalloy-cli/src/main.rs
crates/sonalloy-cli/tests/cli.rs
```

Tasks:

1. add trace request/options
2. resolve IDs before render
3. insert deterministic trace observation boundaries
4. collect baseline/periodic/post-event/final points
5. latency-correct public frame positions
6. enforce 100,000-observation limit
7. serialize optional `trace` in success report
8. add compact human output

Do not make ordinary `render_instrument*` callers pay for trace structures when trace is disabled.

## Step 10 — Integrate review generation and documentation

Files:

```text
review/generate/measure_wav.py
review/generate/**                       # only callers that can consume CLI analysis
.agents/skills/create-instrument/SKILL.md
docs/instrument-definition.md
docs/runtime-processing.md
docs/cli.md
docs/testing-and-sound-review.md
README.md
```

Tasks:

1. consume product analysis where review renders already call CLI
2. retain review-specific comparison helpers only
3. update agent workflow
4. update canonical contracts without duplicating the same explanation across documents

---

# 14. Test plan

## 14.1 Definition and compiler tests

Add or update tests for:

### Schema

- schema version 2 succeeds
- unsupported schema version is rejected
- route `amount` is not accepted in v2
- route `depth` rejects unknown fields

### Unit matching

Representative target cases:

| Target | Expected modulation unit |
|---|---|
| layer gain | decibels |
| layer pan | pan |
| layer tuning | cents |
| filter cutoff | octaves |
| filter resonance | normalized |
| granular grain size | octaves |
| granular density | octaves |
| granular pitch | cents |
| spectral shift | hertz |
| additive spectral tilt | decibels_per_octave |
| operator ratio | octaves |
| operator modulation amount | index or normalized according to the parameter descriptor created for the selected mode |

Test:

- correct unit accepted
- incorrect unit rejected with route depth unit diagnostic
- depth at positive/negative limit accepted
- just outside limit rejected
- non-finite depth rejected

## 14.2 Runtime modulation tests

Use simple fixed sources and deterministic event sequences.

### Linear direct depth

- tuning base 0, source +1, depth +20 cents → +20
- source -1 → -20
- unipolar envelope 0.5, depth -12 dB → -6 dB contribution

### Log2 direct depth

- cutoff 1000 Hz, source 1, +2 octaves → 4000 Hz
- cutoff 1000 Hz, source 0.5, +2 octaves → 2000 Hz
- grain size 0.08 s, source 1, -1 octave → 0.04 s

### Multiple routes

- linear routes add in Definition order
- Log2 octave contributions sum before exponentiation
- positive and negative routes cancel as expected
- final clamp occurs after route sum
- `clamped` diagnostic calculation matches final runtime value

### Curve

Keep existing Linear/SmoothStep behavior and test direct-depth values through both curves.

### Partitioning

Render the same Dynamic Parameter instrument with block sizes:

```text
32
64
257
1024
```

Compare under the existing repository tolerance.

## 14.3 Event Sequence tests

- native cutoff value converts to correct normalized Core event
- native tuning/gain/pan values convert correctly
- out-of-range native value produces field-path diagnostic
- NaN/Infinity cannot enter through JSON
- removed `normalized` field is rejected

## 14.4 Inspect tests

Integration tests should assert selected fields, not freeze the entire huge JSON as one brittle snapshot.

Required cases:

1. tuning parameter reports cents modulation unit
2. cutoff reports octaves modulation unit and max depth
3. bipolar LFO route reports `-depth..+depth`
4. unipolar envelope +2-octave route reports factor `1..4`
5. negative unipolar Log2 depth reports factor below one
6. multiple routes produce correct reachable range from default
7. reachable range marks `may_clamp` when unclamped bound exceeds target range
8. source polarity/range is correct for every built-in/user source kind

## 14.5 Audio analysis unit tests

Generate samples in memory; do not round-trip through WAV unless testing CLI I/O.

### Silence

- finite true
- peak/rms zero
- dBFS null
- activity frames null
- stereo correlation null
- spectrum empty/null as defined

### Sine

Use known 440 Hz or a bin-centered frequency.

Verify:

- peak near expected amplitude
- RMS near amplitude / sqrt(2)
- DC near zero
- spectral peak near expected frequency
- centroid near expected frequency for a pure tone

### DC signal

- correct DC mean
- correlation behavior defined

### Stereo

- identical channels → correlation near +1
- inverted channels → near -1
- deterministic orthogonal/different signals → near expected low correlation

### Continuity

Construct one artificial discontinuity and verify frame index/count/max delta.

### Activity

Known silent prefix/body/suffix produces exact threshold-crossing frames.

## 14.6 Render analysis CLI tests

For each render command:

- no `--analyze` → `analysis` omitted
- `--analyze --json` → expected structure present
- WAV content is identical with analysis off/on
- render note includes reference frequency
- events/midi do not invent reference frequency

## 14.7 Trace unit/integration tests

### Exact movement

- envelope → tuning in cents
- LFO → pan
- velocity → gain
- random → pan with deterministic note ID/seed
- envelope → Log2 cutoff in octaves

For each record verify:

- raw source
- shaped source
- depth
- contribution
- base
- before clamp
- final
- clamped flag

### Polyphony

Render overlapping notes and verify:

- separate observations contain correct note IDs/voice indices
- velocity/random source values differ where expected
- same target parameter can report different final values per voice

### Global target

Verify one observation with `voice: null`.

### Inactive layer

Verify layer target is omitted for a voice while the layer is inactive.

### Timing

- periodic frames are stable across render block sizes
- post-event observation occurs after parameter/control/note events
- final point exists
- latency-corrected frame 0 aligns with public WAV timeline

### Audio invariance

Render trace off/on and compare WAV output under the existing block-partition tolerance for:

```text
44.1 kHz
48 kHz
96 kHz
```

with representative block sizes.

### Trace limit

- estimated request over the limit is rejected before render when possible
- dense event case cannot silently truncate

## 14.8 Existing quality gates

Run the repository's standard gates:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run native/sanitizer/fault-injection checks required by the existing CI configuration.

Run repository review generation affected by changed Definitions and inspect/metric contracts.

---

# 15. Acceptance scenarios

The implementation is complete only when these end-to-end scenarios work from the released CLI surface.

## Scenario A — Agent creates a vibrato without parameter-range math

Definition:

```json
{
  "source": "vibrato",
  "target": "layer.body.tuning",
  "depth": { "value": 18.0, "unit": "cents" },
  "curve": "linear"
}
```

Expected:

- validation succeeds
- Inspect says source is bipolar `-1..1`
- Inspect says effect is `-18..+18 cents`
- Trace shows final tuning moving within that range
- no source-code reading is required

## Scenario B — Agent creates a filter envelope in octaves

Definition:

```json
{
  "source": "filter_env",
  "target": "voice.processor.tone.cutoff",
  "depth": { "value": 2.0, "unit": "octaves" },
  "curve": "smooth_step"
}
```

Base cutoff: `1200 Hz`.

Expected:

- Inspect identifies Log2 target
- Inspect reports factor `×1..×4`
- default reachable bound is `1200..4800 Hz` unless another route/clamp modifies the bound
- Trace reports raw envelope, shaped envelope, octave contribution, factor, and final Hz

## Scenario C — Agent finds clamp before listening

Base cutoff: `9500 Hz`, +2-octave unipolar route.

Expected Inspect:

```text
unclamped max = 38000 Hz
effective max = 20000 Hz
may_clamp = true
```

Agent can redesign base/depth before rendering.

## Scenario D — Agent validates rendered output without Python

Command:

```bash
sonalloy render note instrument.json \
  --note 60 --velocity 100 --gate 0.6 --tail 0.6 \
  --analyze \
  --trace layer.body.tuning \
  --trace voice.processor.tone.cutoff \
  --json \
  --output out.wav
```

Expected one JSON report contains:

- ordinary render metadata
- common audio metrics
- selected parameter traces
- diagnostics

The agent can answer:

- output level
- audible signal duration by threshold
- stereo correlation
- spectral centroid/peaks
- large discontinuities
- actual tuning movement
- actual cutoff movement
- clamp occurrences

without writing an external analysis program.

## Scenario E — Human review remains meaningful

Analysis/Trace can establish that the intended mechanism worked and that output has no obvious numeric defect. The skill still instructs the agent/user to perform human listening for timbre, musical suitability, character, balance, and subjective quality.

---

# 16. Final completion criteria

All of the following must be true.

### Definition authoring

- [x] Instrument schema version is 2.
- [x] `modulation.routes[].amount` is removed.
- [x] Routes use explicit `{ value, unit }` depth.
- [x] Direct-depth formulas are the only runtime modulation formulas.
- [x] Event Sequence `parameter_change` uses `native_value`.
- [x] No compatibility alias, old parser branch, or migration path remains.
- [x] All repository Definitions/events use the new contract.

### Numeric semantics

- [x] Every dynamic parameter can report native unit, scale, modulation unit, and max depth.
- [x] Ambiguous normalized fields have endpoint/neutral/formula semantics in canonical docs and the agent skill.
- [x] Operator modulation amount is explained per mode using actual runtime behavior.

### Inspect

- [x] Parameter report includes modulation unit/depth limit.
- [x] Source report includes numeric range and polarity.
- [x] Route report includes direct depth and static effect.
- [x] Modulated reachable range from default is reported with clamp information.
- [x] Human and JSON output agree semantically.

### Render analysis

- [x] `render note/events/midi --analyze` works.
- [x] Analysis uses corrected output audio.
- [x] No generic arbitrary-audio fundamental claim is made.
- [x] Analysis reports finite/level/DC/activity/continuity/stereo/spectrum facts.
- [x] Analysis does not change WAV output.

### Trace

- [x] Repeatable `--trace <parameter>` works for all Dynamic Parameter owner categories.
- [x] Trace reports raw/shaped source, direct depth, contribution, base, unclamped, final, and clamped state.
- [x] Per-voice identity is present where required.
- [x] Trace observation frames are deterministic.
- [x] Trace limit prevents unbounded diagnostic output.
- [x] Trace does not introduce process-time allocations in the normal runtime path.
- [x] Trace-enabled audio matches normal rendering within existing partition tolerance.

### Code quality

- [x] Voice/global/Inspect/Trace do not maintain separate modulation formulas.
- [x] No dead compatibility code remains.
- [x] No one-off migration script remains unless it has an explicit permanent product purpose.
- [x] Existing realtime allocation guarantees remain intact.
- [x] `cargo fmt`, `clippy -D warnings`, workspace tests, and required native CI checks pass.

### Documentation and agent workflow

- [x] `docs/instrument-definition.md` reflects the final v2 contract.
- [x] `docs/runtime-processing.md` has the direct-depth runtime formula.
- [x] `docs/cli.md` documents Inspect/Analysis/Trace and Event Sequence native values.
- [x] `.agents/skills/create-instrument/SKILL.md` uses the complete authoring feedback loop.
- [x] README remains concise and points users to the canonical documents.

---

# 17. Expected result

After this change, Sonalloy's AI-first claim should hold across the complete sound-design loop rather than only at JSON generation time.

An agent should be able to:

```text
understand a parameter
→ choose a value in an explicit unit
→ understand a modulation route before rendering
→ render the instrument
→ inspect output metrics
→ inspect selected runtime parameter movement
→ revise the Definition
```

The agent may still use human listening for artistic judgment, but it should no longer need to inspect Rust DSP implementation or generate custom Python merely to determine what a Sonalloy parameter means or whether a declared modulation actually executed.
