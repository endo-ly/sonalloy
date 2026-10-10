use super::{AssetReference, GeneratorDefinition, InstrumentDefinition, ProcessorDefinition};

impl InstrumentDefinition {
    /// Visit every asset reference in definition order, including disabled layers.
    /// The callback receives the reference's field path and may rewrite the reference.
    ///
    /// # Errors
    /// Returns the first error produced by the callback.
    pub fn try_for_each_asset_mut<E>(
        &mut self,
        mut visit: impl FnMut(&str, &mut AssetReference) -> Result<(), E>,
    ) -> Result<(), E> {
        for (index, layer) in self.layers.iter_mut().enumerate() {
            let path = format!("layers[{index}].generator");
            match &mut layer.generator {
                GeneratorDefinition::Sample(value) => {
                    for (index, zone) in value.zones.iter_mut().enumerate() {
                        visit(
                            &format!("{path}.sample.zones[{index}].asset"),
                            &mut zone.asset,
                        )?;
                    }
                }
                GeneratorDefinition::Granular(value) => {
                    visit(&format!("{path}.granular.asset"), &mut value.asset)?;
                }
                GeneratorDefinition::WaveSequence(value) => {
                    for (index, step) in value.steps.iter_mut().enumerate() {
                        visit(
                            &format!("{path}.wave_sequence.steps[{index}].asset"),
                            &mut step.asset,
                        )?;
                    }
                }
                GeneratorDefinition::Wavetable(value) => {
                    visit(&format!("{path}.wavetable.asset"), &mut value.asset)?;
                }
                GeneratorDefinition::Spectral(value) => {
                    visit(&format!("{path}.spectral.asset_a"), &mut value.asset_a)?;
                    if let Some(asset) = &mut value.asset_b {
                        visit(&format!("{path}.spectral.asset_b"), asset)?;
                    }
                }
                GeneratorDefinition::Oscillator(_)
                | GeneratorDefinition::Noise(_)
                | GeneratorDefinition::PhysicalString(_)
                | GeneratorDefinition::Modal(_)
                | GeneratorDefinition::Additive(_)
                | GeneratorDefinition::Formant(_)
                | GeneratorDefinition::OperatorModulation(_) => {}
            }
            visit_processors(
                &mut layer.processors,
                &format!("layers[{index}].processors"),
                &mut visit,
            )?;
        }
        visit_processors(&mut self.voice_processors, "voice_processors", &mut visit)?;
        visit_processors(&mut self.global_processors, "global_processors", &mut visit)
    }
}

fn visit_processors<E>(
    processors: &mut [ProcessorDefinition],
    path: &str,
    visit: &mut impl FnMut(&str, &mut AssetReference) -> Result<(), E>,
) -> Result<(), E> {
    for (index, processor) in processors.iter_mut().enumerate() {
        match processor {
            ProcessorDefinition::Convolution(value) => {
                visit(&format!("{path}[{index}].ir"), &mut value.ir)?;
            }
            ProcessorDefinition::Filter(_)
            | ProcessorDefinition::LadderFilter(_)
            | ProcessorDefinition::Drive(_)
            | ProcessorDefinition::Eq(_)
            | ProcessorDefinition::Formant(_)
            | ProcessorDefinition::Resonator(_)
            | ProcessorDefinition::Bitcrusher(_)
            | ProcessorDefinition::Chorus(_)
            | ProcessorDefinition::Flanger(_)
            | ProcessorDefinition::Phaser(_)
            | ProcessorDefinition::FrequencyShifter(_)
            | ProcessorDefinition::Delay(_)
            | ProcessorDefinition::Reverb(_)
            | ProcessorDefinition::Gate(_)
            | ProcessorDefinition::Vocoder(_)
            | ProcessorDefinition::EnvelopeTransfer(_)
            | ProcessorDefinition::SpectralMorph(_)
            | ProcessorDefinition::TransientShaper(_)
            | ProcessorDefinition::Compressor(_)
            | ProcessorDefinition::Limiter(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::ConvolutionProcessorDefinition;

    #[test]
    fn asset_visitor_rewrites_sample_zones_and_all_processor_placements() {
        let mut definition: InstrumentDefinition = serde_json::from_str(include_str!(
            "../../../../testdata/instruments/mapped-sample-instrument.json"
        ))
        .unwrap();
        let convolution = ProcessorDefinition::Convolution(ConvolutionProcessorDefinition {
            id: "room".to_owned(),
            ir: AssetReference {
                path: "room.wav".to_owned(),
                sha256: None,
            },
            gain_db: 0.0,
            mix: 0.5,
        });
        definition.layers[0].enabled = false;
        definition.layers[0].processors.push(convolution.clone());
        definition.voice_processors.push(convolution.clone());
        definition.global_processors.push(convolution);
        let mut fields = Vec::new();

        definition
            .try_for_each_asset_mut(|field, reference| {
                fields.push(field.to_owned());
                reference.path = format!("assets/{}.wav", fields.len());
                Ok::<_, ()>(())
            })
            .unwrap();

        assert_eq!(fields.len(), 8);
        assert_eq!(
            &fields[5..],
            [
                "layers[0].processors[0].ir",
                "voice_processors[0].ir",
                "global_processors[0].ir"
            ]
        );
        let mut paths = Vec::new();
        definition
            .try_for_each_asset_mut(|_, reference| {
                paths.push(reference.path.clone());
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(
            paths,
            (1..=8)
                .map(|index| format!("assets/{index}.wav"))
                .collect::<Vec<_>>()
        );
    }
}
