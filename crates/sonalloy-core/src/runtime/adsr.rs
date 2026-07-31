use crate::compiler::CompiledAdsr;

/// Runtime state of one ADSR envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdsrState {
    /// No note is sounding.
    Idle,
    /// Rising toward full level.
    Attack,
    /// Falling toward sustain level.
    Decay,
    /// Holding the configured sustain level.
    Sustain,
    /// Falling to silence after Note Off.
    Release,
}

/// Sample-accurate ADSR state machine.
#[derive(Debug, Clone)]
pub(crate) struct AdsrRuntime {
    config: CompiledAdsr,
    state: AdsrState,
    level: f32,
    start_level: f32,
    elapsed: usize,
}

impl AdsrRuntime {
    pub(crate) fn new(config: CompiledAdsr) -> Self {
        Self {
            config,
            state: AdsrState::Idle,
            level: 0.0,
            start_level: 0.0,
            elapsed: 0,
        }
    }

    pub(crate) fn note_on(&mut self) {
        self.state = AdsrState::Attack;
        self.level = 0.0;
        self.start_level = 0.0;
        self.elapsed = 0;
        self.skip_zero_duration_segments();
    }

    pub(crate) fn note_off(&mut self) {
        if matches!(self.state, AdsrState::Idle | AdsrState::Release) {
            return;
        }
        self.state = AdsrState::Release;
        self.start_level = self.level.clamp(0.0, 1.0);
        self.elapsed = 0;
        if self.config.release_samples == 0 {
            self.reset();
        }
    }

    pub(crate) fn reset(&mut self) {
        self.state = AdsrState::Idle;
        self.level = 0.0;
        self.start_level = 0.0;
        self.elapsed = 0;
    }

    pub(crate) fn next_sample(&mut self) -> f32 {
        loop {
            match self.state {
                AdsrState::Idle => return 0.0,
                AdsrState::Attack => {
                    if self.config.attack_samples == 0 {
                        self.state = AdsrState::Decay;
                        self.level = 1.0;
                        self.elapsed = 0;
                        continue;
                    }
                    let progress = progress(self.elapsed, self.config.attack_samples);
                    self.level = exponential_rise(progress);
                    self.elapsed = self.elapsed.saturating_add(1);
                    if self.elapsed >= self.config.attack_samples {
                        self.state = AdsrState::Decay;
                        self.level = 1.0;
                        self.elapsed = 0;
                        self.skip_zero_duration_segments();
                    }
                    return self.level;
                }
                AdsrState::Decay => {
                    if self.config.decay_samples == 0 {
                        self.state = AdsrState::Sustain;
                        self.level = self.config.sustain_level;
                        self.elapsed = 0;
                        continue;
                    }
                    let progress = progress(self.elapsed, self.config.decay_samples);
                    self.level = exponential_fall(1.0, self.config.sustain_level, progress);
                    self.elapsed = self.elapsed.saturating_add(1);
                    if self.elapsed >= self.config.decay_samples {
                        self.state = AdsrState::Sustain;
                        self.level = self.config.sustain_level;
                        self.elapsed = 0;
                    }
                    return self.level;
                }
                AdsrState::Sustain => return self.config.sustain_level,
                AdsrState::Release => {
                    if self.config.release_samples == 0 {
                        self.reset();
                        continue;
                    }
                    let progress = progress(self.elapsed, self.config.release_samples);
                    self.level = exponential_fall(self.start_level, 0.0, progress);
                    self.elapsed = self.elapsed.saturating_add(1);
                    if self.elapsed >= self.config.release_samples {
                        self.reset();
                    }
                    return self.level;
                }
            }
        }
    }

    pub(crate) fn is_idle(&self) -> bool {
        self.state == AdsrState::Idle
    }

    pub(crate) fn frames_until_idle(&self) -> Option<usize> {
        match self.state {
            AdsrState::Idle => Some(0),
            AdsrState::Release => Some(self.config.release_samples.saturating_sub(self.elapsed)),
            AdsrState::Attack | AdsrState::Decay | AdsrState::Sustain => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn state(&self) -> AdsrState {
        self.state
    }
}

fn progress(elapsed: usize, duration: usize) -> f32 {
    if duration == 0 {
        1.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        let progress = elapsed as f32 / duration as f32;
        progress.clamp(0.0, 1.0)
    }
}

fn exponential_rise(progress: f32) -> f32 {
    let endpoint = 1.0 - (-5.0_f32).exp();
    (1.0 - (-5.0 * progress).exp()) / endpoint
}

fn exponential_fall(start: f32, target: f32, progress: f32) -> f32 {
    let shape = (-5.0 * progress).exp();
    target + (start - target) * (shape - (-5.0_f32).exp()) / (1.0 - (-5.0_f32).exp())
}

impl AdsrRuntime {
    fn skip_zero_duration_segments(&mut self) {
        loop {
            match self.state {
                AdsrState::Attack if self.config.attack_samples == 0 => {
                    self.state = AdsrState::Decay;
                    self.level = 1.0;
                }
                AdsrState::Decay if self.config.decay_samples == 0 => {
                    self.state = AdsrState::Sustain;
                    self.level = self.config.sustain_level;
                }
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(attack: usize, decay: usize, sustain: f32, release: usize) -> AdsrRuntime {
        AdsrRuntime::new(CompiledAdsr {
            attack_samples: attack,
            decay_samples: decay,
            sustain_level: sustain,
            release_samples: release,
        })
    }

    #[test]
    fn zero_duration_segments_advance_without_looping() {
        let mut adsr = envelope(0, 0, 0.5, 0);
        adsr.note_on();
        assert_eq!(adsr.state(), AdsrState::Sustain);
        assert!((adsr.next_sample() - 0.5).abs() < 1.0e-6);
        adsr.note_off();
        assert!(adsr.is_idle());
    }

    #[test]
    fn note_off_during_attack_releases_from_current_level() {
        let mut adsr = envelope(100, 0, 1.0, 10);
        adsr.note_on();
        for _ in 0..50 {
            let _ = adsr.next_sample();
        }
        let before_release = adsr.next_sample();
        adsr.note_off();
        let after_release = adsr.next_sample();
        assert!(after_release <= before_release);
        assert!(after_release > 0.0);
    }

    #[test]
    fn release_reaches_idle_at_the_configured_duration() {
        let mut adsr = envelope(0, 0, 1.0, 4);
        adsr.note_on();
        let _ = adsr.next_sample();
        adsr.note_off();
        for _ in 0..4 {
            let _ = adsr.next_sample();
        }
        assert!(adsr.is_idle());
    }

    #[test]
    fn frames_until_idle_tracks_release_remaining() {
        let mut adsr = envelope(0, 0, 1.0, 4);
        adsr.note_on();
        let _ = adsr.next_sample();
        assert_eq!(adsr.frames_until_idle(), None);
        adsr.note_off();
        assert_eq!(adsr.frames_until_idle(), Some(4));
        let _ = adsr.next_sample();
        assert_eq!(adsr.frames_until_idle(), Some(3));
    }
}
