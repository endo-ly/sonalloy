use crate::compiler::CompiledFormantProfile;
use crate::process::{ProcessError, ProcessorFailureKind};

pub(crate) fn profile_pair(
    profiles: &[CompiledFormantProfile],
    position: f32,
) -> Result<(&CompiledFormantProfile, &CompiledFormantProfile, f32), ProcessError> {
    if profiles.is_empty() || !position.is_finite() || !(0.0..=1.0).contains(&position) {
        return Err(invalid_state());
    }
    if profiles.len() == 1 {
        return Ok((&profiles[0], &profiles[0], 0.0));
    }
    #[allow(clippy::cast_precision_loss)]
    let scaled = position * (profiles.len() - 1) as f32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = (scaled.floor() as usize).min(profiles.len() - 2);
    #[allow(clippy::cast_precision_loss)]
    let mix = scaled - index as f32;
    Ok((&profiles[index], &profiles[index + 1], mix))
}

pub(crate) fn geometric_lerp(first: f32, second: f32, mix: f32) -> f32 {
    (first.ln() + (second.ln() - first.ln()) * mix).exp()
}

fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::InvalidState,
    }
}
