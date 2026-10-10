# Sonalloy テスト体系の整理・CI最適化 実装指示書

## 1. 目的と基本方針

### 対象

- Repository: https://github.com/endo-ly/sonalloy
- Base: `main`
- 調査時点のHEAD: `785f321697c56770e264b9652fe47a5742ca1abe`
- 実装単位: **1 PRで全対象を完了する**

Sonalloyのテスト体系を見直し、実際の不具合検出に寄与するテストを残しながら、重複、過剰なケース分け、内部実装への依存、無意味な保証を積極的に削除・統合する。

同時に、GitHub Actions、CMake、Node.js、PowerShellによる検証を整理し、CIの実行時間と保守負担を削減する。

### 判断基準

各テストについて、次の問いを判断の中心に置く。

**「このテストを削除した場合、どのような現実的な不具合を見逃すのか」**

削除によって見逃す不具合が具体的に説明できないもの、他のテストで同じ不具合を検出できるものは、積極的に削除する。

実行時間、不安定さ、修正負担、テストの複雑さに対して、保証の価値が小さいものも整理対象とする。

特に以下を重視する。

- 同じ入力・期待値・失敗条件を異なる階層で繰り返しているテストは、原則として一か所へ集約する。
- 内部の計算式、構造体、配列順序を確認するだけのテストは、外部から観測できる動作で代替できるか検討する。
- 正常に動作することを既存の統合テストで保証できる場合、単純な正常系テストを追加で維持しない。
- 同じ異常系について、値やメッセージを少し変えただけの大量のケースを維持しない。
- 旧仕様や廃止形式を拒否するテストを整理する。
- テスト件数、カバレッジ、既存テスト行数の維持を目的にしない。
- 削除したテストの代わりに、同じ内容のテストを別ファイルへ機械的に移動しない。

### 実装上の制約

1. **本書の対象をすべて1 PRで対応する。** 実装量を理由としたスコープ縮小、別PRへの先送りは禁止。
2. テスト対象のプロダクションコードは、原則として変更しない。テストの共通化に不可欠な軽微な修正は許容する。
3. テストを通すためのフォールバック、互換分岐、実装の劣化は禁止。
4. テスト専用の過剰な抽象化、巨大な共通テスト基盤、設定ファイルの追加を避ける。
5. 削除後に未使用となるテスト補助関数、Fixture、Import、依存パッケージ、CI設定も整理する。
6. テストの保証を移す場合は、移動先の既存テストを拡張することを優先する。
7. テスト以外の機能変更、仕様変更、プリセット改変、フォーマット変更は行わない。
8. 不具合を発見した場合、テスト削除によって隠さず、原因を明確にして対応する。
9. `AGENTS.md`と既存のコーディング規約に従う。
10. 作業前に既存の差分を確認し、ユーザーの変更を破棄しない。

## 2. CI・ビルド検証の整理

対象：

- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`
- `CMakeLists.txt`
- `tests/cmake_cli_consumer.cmake`
- `tests/cmake_consumer.cpp`
- `scripts/verify-msvc-capi-consumer.ps1`
- `scripts/builtin-bundle.test.mjs`

### 2.1 GitHub Actionsの役割分担

現状、Windows、Linux、macOSの3環境で、共通するRustの検証が広範囲に繰り返されている。

以下の構成を基本として再編する。

| 環境 | 検証内容 |
|---|---|
| Linux | Workspace全体のFormat、Clippy、全Rustテスト、Node.js Bundleテスト、CMake連携 |
| Windows | Windows固有のCLI・C API・DSP連携テスト、CMake、MSVC Debug/Release連携 |
| macOS | macOS向けビルド・CMake連携・代表的な実行確認 |
| Linux Native Fault Injection | Native例外処理・異常系テスト |
| Linux Native Sanitizer | AddressSanitizer / UndefinedBehaviorSanitizer |
| Audit | 依存ライブラリの脆弱性検査 |

具体的な変更：

1. `cargo fmt --all -- --check`をLinuxのみに集約する。
2. `cargo clippy --workspace --all-targets -- -D warnings`をLinuxのみに集約する。
3. `cargo test --workspace`をLinuxで全実行する。
4. Windowsでは、C API・CLI・DSPなど、Windows固有の不具合を検出するために必要なテストを実行する。Coreの全テストを無条件に再実行する構成は整理する。
5. macOSでは、CMake経由のビルドと代表的なCLI・C API動作を検証する。Linuxと同じ全Rustテストの再実行を削減する。
6. LinuxのNode.js Bundleテストは維持する。
7. `cargo audit`は独立したJobとして維持する。
8. Native Fault InjectionとSanitizerは維持する。

Windows・macOSに残す検証は、**それぞれのOS固有の実装、リンク、実行環境に起因する不具合を検出できるもの**を選ぶ。

Linuxの全テストと完全に同じシナリオを繰り返す必要はない。

ただし、WindowsのMSVC ABI、macOSのC++ランタイム連携、OS固有の音声デバイス周辺など、実際に環境差がある箇所の検証を失わないこと。

### 2.2 Releaseビルドの重複削除

現在のCIでは、3環境で`cargo build --workspace --release`を実行している。

別途、`release.yml`では各配布ターゲット向けのReleaseビルドを行っている。

対応：

- 通常CIの全Workspace Releaseビルドを削減する。
- WindowsのMSVC Release C API連携検証に必要なReleaseビルドは維持する。
- LinuxとmacOSのReleaseバイナリ生成は、リリースワークフロー側へ集約する。
- `release.yml`のバイナリ生成、アーカイブ作成、Checksum生成、公開処理は維持する。
- 通常CIでしか検出できないRelease構成固有の問題がある場合、その保証に必要な最小のビルドだけを残す。

Releaseワークフローの配布対象OSとアーキテクチャは変更しない。

### 2.3 CMakeの不要なプリセット生成

`CMakeLists.txt`の次の依存関係を修正する。

```cmake
sonalloy_stage_builtin_bundle(
    TARGET sonalloy-builtin-bundle-stage
    DESTINATION "${CMAKE_CURRENT_BINARY_DIR}/test-builtin-resources"
)
add_dependencies(sonalloy-cmake-consumer sonalloy-builtin-bundle-stage)
```

CMake Consumerの実際の検査内容は、C APIのCapability問い合わせとCLIの`--version`実行である。

これらにプリセット一式のステージングは不要。

対応：

- CMake Consumerから`sonalloy-builtin-bundle-stage`への依存を削除する。
- Consumerテスト専用のBundle生成を削除する。
- 公開CMake関数`sonalloy_stage_builtin_bundle`は維持する。
- Bundle生成の検証責務を`scripts/builtin-bundle.test.mjs`へ集約する。
- CMake経由でC++ Consumerがリンク・実行できることは引き続き検証する。

`tests/cmake_cli_consumer.cmake`の`--version`検査も確認し、CLI自身の起動確認との重複を整理する。

CMake経由で生成されたCLIが実行可能であるという独自の保証が残る場合は、その確認だけを維持する。

### 2.4 Windows MSVC連携

`scripts/verify-msvc-capi-consumer.ps1`を対象に、CMake Consumerとの重複を確認する。

現行のDebug `/MDd`とRelease `/MD`では、実際に異なるランタイム構成を扱うため、両方のABI互換性を保証する。

以下の条件を満たすよう整理する。

- Debug / Releaseの両構成が検証される。
- C++から公開ヘッダーをIncludeできる。
- Static Libraryが正しくリンクできる。
- Consumerを実行できる。
- RustとC++のMSVCランタイム構成が一致する。

CMake ConsumerとPowerShellで完全に重複するビルドや検査があれば、片方へ集約する。

ただし、MSVCランタイムのDebug/Release差異の保証は維持する。

### 2.5 Nativeテスト

以下のJobは維持する。

- `linux-native-fault-injection`
- `linux-native-sanitizer`

Native FFIの例外境界やメモリ破壊を検出する独自の価値がある。

通常のRustテストと重複する部分は削減してよいが、Fault InjectionとSanitizerの実行自体を削除してはならない。

## 3. Scripts・Bundleテスト

対象：

`scripts/builtin-bundle.test.mjs`

現行の4テストを整理する。

### 正常系

`staged built-in bundle contains source definitions and required resources`

このテストには、プリセット定義のメタデータをフィールド単位で比較する大量のAssertionがある。

対応：

- Source側のPreset ID一覧と、生成したManifestのID一覧が一致することを確認する。
- Manifestに重複IDがないことを確認する。
- 代表的な定義JSONが正しくコピーされることを確認する。
- 必須のLicense、Notice、共有Assetが含まれることを確認する。
- Manifestの基本構造と参照先が有効であることを確認する。
- `name`、`author`、`description`、`category`、`tags`、`preview`などを個別に何十回も比較する処理を整理する。
- メタデータ変換処理をテストコード内でもう一度実装しない。

### 異常系

以下は実際のデータ欠落・バンドル破損を防ぐため、維持する。

- `staging rejects a preset with missing required metadata`
- `staging rejects an unexpected preset directory instead of omitting it`
- `staging rejects duplicate preset IDs`

共通する一時ディレクトリ生成、License作成、後片付けが重複する場合は整理する。

補助関数を増やしすぎず、同じセットアップを繰り返す部分に限定する。

## 4. Rustテストの削除

この章では、既存の統合テストによる保証、または次章での統合を前提として、個別のテストを削除する。

### 4.1 定義・スキーマ

対象：

- `crates/sonalloy-core/src/definition/generator.rs`
- `crates/sonalloy-core/src/definition/processor.rs`
- `crates/sonalloy-core/src/definition/modulation.rs`
- `crates/sonalloy-core/src/definition/metadata.rs`
- `crates/sonalloy-core/src/definition/mod.rs`

| テスト | 対応 |
|---|---|
| `legacy_string_waveform_is_rejected` | 削除 |
| `sample_schema_rejects_legacy_direct_fields` | 削除 |
| `schema_rejects_the_old_delay_shape` | 削除 |
| `routes_require_depth_and_reject_legacy_amount_fields` | 旧amount形式の拒否だけを削除し、現行depthの契約検査は残す |
| `valid_schema_v6_metadata_has_no_diagnostics` | 削除 |
| `valid_definition_has_no_diagnostics` | 削除 |
| `validation_accepts_multiple_enabled_layers` | 削除 |

旧形式の拒否に特化したFixture、補助関数、Importも整理する。

現在のスキーマにおける未知フィールド拒否、必須項目、公開診断コードなどの保証は維持する。

**スキーマバージョンの更新は行わない。**

### 4.2 CLI

対象：

- `crates/sonalloy-cli/src/command/mod.rs`
- `crates/sonalloy-cli/tests/dev_cli.rs`
- `crates/sonalloy-cli/tests/realtime_cli.rs`
- `crates/sonalloy-cli/tests/render_cli.rs`
- `crates/sonalloy-cli/tests/instrument_cli.rs`
- `crates/sonalloy-cli/tests/demo_cli.rs`
- `crates/sonalloy-cli/src/command/update.rs`

| テスト | 対応 |
|---|---|
| `every_command_and_argument_has_help` | 削除。再帰的なHelp存在検査の補助関数も削除 |
| `clap_rejects_zero_block_size_before_rendering` | 数値引数制約テストへ統合して削除 |
| `play_rejects_zero_buffer_before_accessing_devices` | CLI引数解析テストへ統合して削除 |
| `render_note_rejects_midi_values_above_127` | 数値引数制約テストへ統合して削除 |
| `invalid_definition_returns_exit_code_one` | 不完全JSONの診断コード検査へ終了コード確認を移して削除 |
| `output_directory_used_as_file_returns_wav_error` | ファイル出力エラーの代表ケースに集約して削除 |
| `render_demo_measures_mp3_without_mastering` | 既存Demoテストへの統合を検討し、独立テストを削除 |
| `up_to_date_compares_tagged_latest_release` | 更新判定のより上位のテストに集約できる場合は統合して削除。単独テストの保証価値も評価する |

特にCLIでは、同じClap制約を単体テストと実CLIプロセスの両方で網羅する構造を解消する。

ただし、CLIの終了コード、JSON診断コード、副作用の有無など、実プロセスでしか確認できない内容は維持する。

### 4.3 内部計算・補助関数

| ファイル | テスト | 対応 |
|---|---|---|
| `runtime/voice.rs` | `vector_weight_scales_linear_gain_after_db_conversion` | 実際の音量制御テストで代替できる場合は削除 |
| `runtime/sample.rs` | `root_note_and_octave_ratios_are_exact` | Sampleの実再生速度・音程検査へ集約 |
| `runtime/modulation.rs` | `value_span_uses_one_shared_ramp_formula` | Parameter Rampの分割独立性テストへ集約 |
| `runtime/generator/additive.rs` | `inharmonicity_keeps_the_fundamental_at_one` | Additiveの実描画テストへ集約 |
| `runtime/generator/granular.rs` | `random_value_is_stable_for_the_same_grain_identity` | GranularのSeed再現性検査へ集約 |
| `runtime/processor/reverb.rs` | `output_taps_read_their_declared_delay_lines` | 内部Delay Lineの構造検査を削減し、実音声結果による保証を優先 |
| `runtime/sine.rs` | `zero_frame_process_is_a_noop` | Coreのゼロフレーム処理検査へ統合 |
| `render.rs` | `duration_zero_produces_empty_stereo` | Rendering境界条件の既存テストと比較して削除 |
| `demo/mod.rs` | `stereo_mix_one_frame_fade_silences_the_final_frame` | Fadeの終端検査へ統合 |
| `pattern/mod.rs` | `default_pattern_is_one_bar_long` | CLIのPattern Init検査へ統合 |

この表の条件付き候補も全件処理する。

実際に保証が重複していることを確認したものは削除し、独立した重要な保証が存在する場合は既存テストへ集約する。

独立テストとして残さざるを得ない場合は、そのテストが検出する具体的な不具合と、他テストでは代替できない理由を最終報告に記載する。

## 5. Rustテストの統合

### 5.1 CLI・MIDI

| 対象 | 統合元 | 統合方針 |
|---|---|---|
| `command/mod.rs` | `numeric_cli_constraints_are_enforced_by_clap` / `play_syntax_constraints_are_enforced_by_clap` | CLI数値・構文制約の共通検査へ整理 |
| `command/realtime.rs` | `macro_cc_parser_resolves_mappings_and_rejects_reserved_or_duplicate_controls` / `macro_cc_parser_checks_format_range_and_reserved_controls` | Macro CCの正常・異常値を一つのまとまりで検査 |
| `instrument_cli.rs` | `basic_generators_validate_and_inspect_all_generator_modes` / `generator_definitions_validate_and_inspect` | GeneratorのValidate/Inspectを整理 |
| `instrument_cli.rs` | `spectral_validate_and_inspect_reports_prepared_asset` / `spectral_reference_and_hybrid_support_inspect` | Spectralの単体・HybridのInspect検査を集約 |
| `demo_cli.rs` | `demo_validate_and_inspect_use_relative_references_and_longest_pattern` / `demo_uses_longest_pattern_for_timeline_and_conductor` | 最長Patternの時間軸検査を集約。Conductor固有の検査は維持 |
| `midi/pattern.rs` + `pattern_cli.rs` | `midi_events_reject_overlapping_notes_with_the_same_pitch` / `pattern_export_rejects_overlapping_same_pitch_notes` | 同一PitchのNote重複拒否をCLI側に集約 |
| `midi/render.rs` + `render_cli.rs` | `control_only_midi_is_rejected` / `render_midi_rejects_control_only_input` | ControlのみのMIDI拒否をCLI側に集約 |
| `midi/pattern.rs` + `pattern_cli.rs` | `midi_parameter_events_are_counted_in_one_error` / `pattern_import_preserves_supported_controls_and_export_rejects_parameters` | 内部エラー件数の検査を削減し、公開診断を確認 |
| `demo/mod.rs` | `stereo_mix_fade_reaches_zero_at_the_final_frame` / `stereo_mix_one_frame_fade_silences_the_final_frame` | Fadeの終端検査を統合 |

CLI起動回数やFixture生成回数の削減を優先する。

一つのテストに大量の独立した責務を詰め込まず、同じ操作・同じ入力系統に属する検査をまとめる。

MIDI変換では、Tick、Tempo、Control Event、Note On/Offの順序など、実際のMIDI解釈に影響する保証を残す。

### 5.2 定義・コンパイル・Parameter Catalog

| 対象 | 統合元 | 方針 |
|---|---|---|
| `definition/metadata.rs` | `category_rejects_empty_and_outer_whitespace` / `tags_reject_too_many_empty_and_case_insensitive_duplicates` | メタデータの無効値検査を整理 |
| `definition/metadata.rs` | `preview_rejects_tempo_and_time_signature_values` / `preview_rejects_note_count_velocity_timing_and_range_values` / `preview_rejects_music_longer_than_ten_seconds` | Previewの無効値検査を集約 |
| `definition/generator.rs` | `sample_zone_mapping_and_round_robin_ranges_are_validated` / `sample_zone_midi_fields_have_explicit_bounds` | Sample Zoneの範囲・対応関係の検査を統合 |
| `definition/generator.rs` | `oscillator_and_noise_ranges_are_validated` / `wavetable_definition_ranges_are_validated` | GeneratorのParameter範囲検査を整理 |
| `compiler/modulation.rs` | `routes_resolve_to_dense_targets_and_preserve_same_target_order` / `route_depth_units_and_limits_are_checked_against_the_target_descriptor` | Route解決とParameter契約をまとめる |
| `parameter/catalog.rs` | `catalog_order_is_definition_order_then_processor_scope`、`generator_parameters_follow_layer_parameters_and_preserve_metadata`、`complex_generator_parameters_preserve_catalog_metadata`、`operator_parameters_follow_topology_and_mode_contract` | 内部Catalog構造の検査を整理し、公開Parameter ID・単位・範囲を優先 |
| `compiler/generator.rs` + `core_process.rs` | `disabled_layers_stay_in_the_catalog_but_not_the_runtime_layers` / `hybrid_compiles_two_layers_and_prepares_the_sample`など | 無効Layerのコンパイル内部構造より、実際に発音対象から除外される動作を優先 |

Parameter Catalogについては、SonalloyのCLIおよびC ABIを通じて外部から利用するParameter ID、単位、値域は重要な契約である。

これらの保証は残す。

一方、内部配列のIndexやCompiled構造体の細部など、実装変更によって頻繁に修正が必要になる検査を減らす。

現行のDefinition SchemaおよびParameter IDを変更してはならない。

### 5.3 Runtimeの状態遷移

対象：

- `crates/sonalloy-core/src/runtime/instrument/event.rs`
- `crates/sonalloy-core/src/runtime/instrument/mod.rs`
- `crates/sonalloy-core/tests/core_process.rs`

#### Sustain

以下を整理する。

- `sustain_defers_release_until_the_pedal_is_lifted`
- `sustain_does_not_release_a_key_that_is_still_down`
- `repeated_sustain_state_changes_are_idempotent`
- `reset_clears_sustain_state`

同一Runtimeを使った状態遷移として統合する。

確認する内容は、Key Down、Sustain On、Note Off、Sustain Off、Resetの実際の発音状態である。

#### Choke Group

以下を整理する。

- `choke_stops_only_selected_group_layers_and_does_not_revive_on_release`
- `newer_choke_hit_cancels_older_pending_layers`
- `choke_group_note_fades_out_only_voices_of_the_same_group`
- `choke_reuses_choked_voice_before_stealing_an_unrelated_voice`
- `choke_does_not_replace_a_pending_note_from_another_group`

同じFixtureやVoiceセットアップを繰り返す部分を集約する。

Choke対象の選択、フェード処理、Voice再利用、Pending Noteとの関係について、異なる不具合を検出するAssertionは維持する。

#### Voice Stealing

以下を整理する。

- `steal_starts_pending_note_when_release_finishes_before_fade`
- `steal_fade_completes_across_multiple_blocks`
- `pending_note_off_cancels_note_before_steal_completion`
- `pending_note_off_is_held_by_sustain_until_steal_completes`
- `reset_discards_pending_note_during_steal`
- `steal_fade_continues_parameter_lfo_and_envelope_processing`
- `steal_completion_inside_a_control_span_starts_the_pending_note`

Voice Stealingの共通Fixture、Event生成、期待状態の確認を整理する。

内部状態の変化を細かくなぞる大量のテストより、音声出力とNote Lifecycleの結果を重視する。

ただし、Pending Note、Sustain、Fade中の発音開始タイミングは実際の挙動に影響するため、その保証は残す。

#### Reset・イベント時間軸

以下を整理する。

- `note_timing_is_independent_of_block_size`
- `absolute_event_timing_is_stable_across_block_sizes`
- `reset_restarts_the_same_render`
- `hybrid_layers_share_one_voice_and_render_finite_audio`などの重複する正常描画検査

同じイベント列に対するブロック分割独立性を共通の代表的なシナリオへ集約する。

Resetについては共通Runtimeの基本契約を代表ケースで確認し、各Generator固有の履歴、Seed、内部バッファを初期化する処理は、その音源固有のテストとして残す。

### 5.4 Sample・Noise・音源

| 対象 | 統合元 | 方針 |
|---|---|---|
| `runtime/sample.rs` | `sample_runtime_finishes_without_out_of_bounds_reads` / `sample_runtime_plays_only_the_compiled_region` | 再生領域・終了時の境界検査をまとめる |
| `runtime/generator/noise.rs` + `core_process.rs` | `correlation_endpoints_and_reset_are_deterministic` / `block_partition_does_not_change_the_noise_sequence` / `noise_is_stereo_deterministic_and_block_size_independent` | NoiseのSeed、Correlation、ブロック独立性の重複を整理 |
| `physical_modal.rs` | `every_supported_modal_mode_count_compiles_and_renders` | Mode Countの6値全描画を縮小 |
| `physical_modal.rs` | `two_layer_render_is_finite_at_supported_sample_rates` / `constant_render_is_bit_exact_across_block_sizes_and_reset_repeats` | 有限値、Sample Rate、Block Sizeの検査を整理 |
| `spectral.rs` | `spectral_identity_resynthesis_matches_the_source_after_reported_latency` / `spectral_morph_of_identical_sources_is_identity_resynthesis` | Identity Resynthesisの重複を整理 |
| `processor_expansion.rs` | `modulation_effect_state_is_independent_of_block_size` / `processor_parameter_changes_and_global_modulation_are_block_size_independent` | Processorの動的変更とブロック独立性を統合 |

Physical ModalのMode Countについては、まず`4 / 12 / 24`など、最小・中間・最大の代表値を使用する。

内部で特定のMode Countだけが異なる処理経路に入る場合、その経路を通る値も残す。

Spectralでは、STFTの再合成精度、遅延、Morph、Freeze、Position、Block Size独立性のように、それぞれ実際の音声結果に独立して影響する保証を維持する。

単純に「finiteである」ことだけを何度も繰り返す検査を縮小する。

### 5.5 Processor・DSP

以下の恒等性テストを整理する。

| 対象ファイル | テスト |
|---|---|
| `runtime/processor/bitcrusher.rs` | `mix_zero_is_an_identity` |
| `runtime/processor/drive.rs` | `amount_zero_is_identity` |
| `runtime/processor/resonator.rs` | `mix_zero_is_an_identity` |
| `runtime/processor/eq.rs` | `zero_gain_is_an_identity` |
| `runtime/processor/compressor.rs` | `ratio_one_without_makeup_is_an_identity` |

既存のProcessor統合テストで同じ入力・出力の恒等性が保証できる場合は、個別テストを削除する。

一つの小規模な共通テストで複数Processorを扱える場合は統合する。

ただし、テストを統合するためだけの大規模なProcessor抽象化は追加しない。

DSP側では次を整理する。

| 対象 | 対応 |
|---|---|
| `triangle_square_and_pulse_generation_are_finite` | ほかの波形描画検査と集約 |
| `signal_frequency_and_saw_generation_are_verified` | Sawの周波数精度を残し、単純な有限値・範囲検査の重複を削減 |
| `backend_reports_capabilities` | 固定バージョン文字列の検査を削減。外部に必要なCapability保証は維持 |
| `backend_reports_fixed_version_and_latency` | 固定バージョン検査を削減し、Latency契約を中心に残す |

対象ファイル：

- `crates/sonalloy-dsp-sys/tests/ffi.rs`
- `crates/sonalloy-dsp-sys/src/stretch.rs`

## 6. 維持する重要な保証

本整理では、次の保証を優先的に維持する。

| 分野 | 維持する保証 |
|---|---|
| Realtime | Prepare後のAudio ThreadでHeap Allocationが発生しない |
| Note Lifecycle | Note On、Note Off、Sustain、Voice Stealing、Choke Groupが正常に動作 |
| Runtime State | Reset、再発音、Runtime Update、Generation切替が正しく動作 |
| 時間軸 | Block Size変更、Tempo変更、Event Offsetによって不正なタイミングにならない |
| 音声品質 | 音程、スペクトル、出力レベル、左右チャンネル、遅延、音声の連続性 |
| Parameter | 公開Parameter ID、単位、範囲、Modulation、Rampの動作 |
| Sample / Asset | Asset不在、Hash不一致、範囲外アクセス、Loop・Stretchの正常動作 |
| CLI | 公開コマンドの主要正常系、終了コード、診断コード、成果物生成 |
| C ABI | Compile、Prepare、Activate、Process、Update、Destroy、Handle所有権 |
| Native | メモリ安全性、Native Exception、Fault Injection、Sanitizer |
| Built-in Preset | 全プリセットのParse、Validate、Compile、Render、非無音出力 |

### Realtime Allocation

以下のような検査は維持する。

- `realtime_render_cases_do_not_allocate_after_prepare`
- `publish_process_and_reclaim_take_do_not_allocate`
- `granular_runtime_process_path_does_not_allocate`
- `spectral_render_does_not_allocate_after_prepare`
- `wave_sequence_process_path_does_not_allocate`
- Audio CallbackのAllocation検査

実際に異なるRealtime処理経路を検証している場合は、それぞれ維持する。

### Built-in Preset

`crates/sonalloy-core/tests/builtin_presets.rs`

対象：

`every_builtin_preset_metadata_validates_and_renders`

対応：

- 存在する全Built-in Presetの検証は維持する。
- `paths.len() >= 60`という最低件数のAssertionを削除する。
- すべてのPresetがParse、Validate、Compileできることを維持する。
- すべてのPresetが正常にRenderでき、非有限値や意図しない無音にならないことを確認する。
- 代表プリセットの固定件数に依存しない。

プリセット数の維持を目的にしたテストを削除し、実際に存在するプリセットの利用可能性を保証する。

## 7. テスト構造の整理

削除・統合に伴い、テストコード自体の構造も整える。

### FixtureとHelper

各テストで使われるFixture、補助関数、共通処理を確認する。

- 削除したテスト専用のFixtureは削除する。
- 使用箇所が消えたHelperは削除する。
- 同じJSON定義やMIDIイベントを何度も構築する処理は、自然に共有できる場合のみ共通化する。
- 少数行の単純な準備処理を共有するだけのために、新しい抽象化を増やさない。
- テスト専用の大規模なBuilderやDSLを新設しない。
- 共通化によりテスト本体が読みづらくなる場合は、簡潔な直接記述を優先する。
- テストからしか使用されず、整理後に不要となったプロダクション側の公開APIや補助関数も確認する。

### Assertion

以下のような検査は積極的に削減する。

- エラーになることだけを複数箇所で確認する。
- `is_finite()`だけを同じ処理経路で何度も確認する。
- 内部配列の長さや順序を実装に合わせて検査する。
- 構造体のフィールドを実装と同じ順番で一つずつ比較する。
- モックが設定した戻り値を、そのままモック経由で確認する。
- Rustの型システムやSerdeなどが保証する性質を、追加の意味を持たない形で確認する。
- 大量の境界値を検査しているが、それぞれの値が同じ処理経路に入る。

一方、公開APIの具体的な動作、ユーザーが観測する出力、リアルタイム処理の制約などは残す。

### テストデータ

`testdata/`以下も参照関係を確認する。

- 削除したテストからしか参照されないテストデータは削除する。
- 既存の音声レビューやドキュメントで利用しているデータは維持する。
- `testdata/generate/`以下の生成スクリプトは、出力先Fixtureが不要になった場合に整理する。
- 必要なテストデータを残すためだけの無意味なテストは追加しない。

## 8. 検証

テスト整理が目的であるため、検証自体も必要最小限の構成とする。

### Rust

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### Node.js

```bash
node --test scripts/builtin-bundle.test.mjs
```

### CMake・Native

変更後のGitHub Actionsで、以下の検証が成功することを確認する。

- Windows / Linux / macOSのCMake Consumer
- Windows MSVC Debug / Release C API Consumer
- Linux Native Fault Injection
- Linux Native Sanitizer
- Releaseワークフローで使用するビルド構成

同じテストを手元で何度も繰り返す必要はない。

必要に応じて変更箇所のテストを実行し、最終的にCI全体の成功を確認する。

### 変更範囲

テストの削除・統合後に以下を確認する。

1. 公開API、JSON Schema、Parameter IDが変更されていない。
2. プロダクションコードにテスト通過用の分岐が追加されていない。
3. 旧形式への互換処理が追加されていない。
4. 不要となったTest HelperとFixtureが残っていない。
5. CIに重複したビルド・テストが残っていない。
6. 必要なNativeとRealtimeの保証が残っている。
7. 新しい大量のテストが追加され、削除効果を相殺していない。

### CI実行時間

GitHub Actionsから変更前後のJob所要時間を比較する。

少なくとも以下を確認する。

- CI全体の完了時間
- Linux Jobの実行時間
- Windows Jobの実行時間
- macOS Jobの実行時間
- Native Fault Injection / Sanitizerの実行時間

計測値が取得できる場合は実測を記載する。

テスト件数やコード行数は成果として報告してよいが、削減数を増やすために必要な検査を削除しない。

## 9. 完了条件

以下をすべて満たしたら完了とする。

- [ ] GitHub ActionsのOS間の重複検証を整理した。
- [ ] 不要なWorkspace Releaseビルドを削減した。
- [ ] CMake Consumerから不要なPreset Stage依存を削除した。
- [ ] MSVC Debug/ReleaseのABI保証を維持した。
- [ ] Fault Injection / Sanitizerを維持した。
- [ ] Node.js Bundleテストの過剰なフィールド比較を整理した。
- [ ] 第4章の削除候補をすべて処理した。
- [ ] 第5章の統合候補をすべて処理した。
- [ ] RuntimeのSustain、Choke、Voice Stealingのテストを整理した。
- [ ] 各Generator・Processorの重複する正常系検査を整理した。
- [ ] Parameter Catalogの公開契約を維持した。
- [ ] Realtime Allocationの保証を維持した。
- [ ] 全Built-in Presetの実描画検査を維持した。
- [ ] Preset最低件数のAssertionを削除した。
- [ ] 不要なFixture、Helper、Importを削除した。
- [ ] `testdata/`内の未使用データを確認した。
- [ ] 未使用のDev Dependencyがあれば削除した。
- [ ] RustのFormat、Clippy、Testが成功した。
- [ ] 必要なCI Jobが成功した。
- [ ] ソースコードの意図しない機能変更がないことを確認した。

## 10. 実装報告・PR

**本作業は1 PRですべて完了させる。**

レビューしやすいよう、以下の論理的なまとまりでコミットを分けてよい。

1. CLI・定義・Schemaのテスト削除
2. Runtime・Generator・Processorのテスト統合
3. Scripts・CMake・CIの整理
4. 未使用Fixtureと補助コードの削除

コミットは分けても、PRは分割しない。

### 最終報告

完了時には次の情報を報告する。

| 項目 | 内容 |
|---|---|
| 削除 | 削除したテスト名・対象ファイル |
| 統合 | 統合元と統合先、変更したAssertion |
| 維持 | 当初候補だったが残したテストと、現実的な不具合に基づく理由 |
| CI | 削減したJob内処理、ビルド、テスト、依存関係 |
| Fixture | 削除した未使用データ・Helper |
| 実測 | 取得できた変更前後のCI実行時間 |
| 検証 | 実行したテストと結果 |
| PR | URL、コミット、変更ファイル数 |

条件付き候補を含め、**本書に記載したすべての対象について、最終的にどう処理したかを確認できること**。

指示対象に未対応の項目を残したまま完了報告しない。

### 自己レビュー

PR作成前に、実装者自身が以下の観点で確認する。

- 同じ保証を行うテストが複数階層に残っていないか。
- 削除したテストの保証が、本当にほかのテストで代替されているか。
- 必要性の低いテストを「念のため」という理由で残していないか。
- テスト統合の結果、巨大で読みづらいテスト関数が生まれていないか。
- 共通化のために複雑な仕組みを導入していないか。
- テストと無関係な実装修正が混入していないか。
- CIの最適化によって、OS固有の重要な検査まで消していないか。
- テストを削除したことで、実際の不具合を見逃す範囲が不当に広がっていないか。

**コードベース全体として、テストが軽量で、役割が明確で、今後の音源追加や機能変更に伴う保守負担が小さい状態にすること。**

これを最終的な完成条件とする。