use symphonia::core::audio::{Channels, Position};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Mono,
    Stereo,
}

impl Layout {
    pub fn channels(&self) -> usize {
        match self {
            Layout::Mono => 1,
            Layout::Stereo => 2,
        }
    }

    pub fn channel_mask(&self) -> u64 {
        match self {
            Layout::Mono => Position::FRONT_CENTER.bits(),
            Layout::Stereo => {
                Position::FRONT_LEFT.bits() | Position::FRONT_RIGHT.bits()
            }
        }
    }
}

pub fn layout_of(channels: &Channels) -> Option<Layout> {
    match channels {
        Channels::Positioned(positions) => {
            let bits = positions.bits();
            if bits.count_ones() == 1 {
                Some(Layout::Mono)
            } else if bits
                == (Position::FRONT_LEFT.bits() | Position::FRONT_RIGHT.bits())
            {
                Some(Layout::Stereo)
            } else {
                None
            }
        }
        Channels::Discrete(1) => Some(Layout::Mono),
        Channels::Discrete(2) => Some(Layout::Stereo),
        _ => None,
    }
}

pub fn channel_index(channels: &Channels, position: Position) -> Option<usize> {
    match channels {
        Channels::Positioned(positions) => {
            let bits = positions.bits();
            (bits & position.bits() != 0)
                .then(|| (bits & (position.bits() - 1)).count_ones() as usize)
        }
        Channels::Discrete(count) => {
            let order = [
                Position::FRONT_LEFT,
                Position::FRONT_RIGHT,
                Position::FRONT_CENTER,
                Position::LFE1,
                Position::REAR_LEFT,
                Position::REAR_RIGHT,
                Position::SIDE_LEFT,
                Position::SIDE_RIGHT,
            ];
            order
                .iter()
                .position(|candidate| candidate.bits() == position.bits())
                .filter(|position| *position < *count as usize)
        }
        _ => None,
    }
}

const CENTRE_MIX: f32 = std::f32::consts::FRAC_1_SQRT_2;

pub fn mix_into(input: &[f32], from: &Channels, to: Layout, output: &mut Vec<f32>) {
    let from_count = from.count();
    assert!(from_count > 0, "mixer needs at least one input channel");
    assert!(
        input.len().is_multiple_of(from_count),
        "mixer input must hold whole frames"
    );
    let to_count = to.channels();
    if layout_of(from) == Some(to) {
        output.clear();
        output.extend_from_slice(input);
        return;
    }
    let frames = input.len() / from_count;
    output.clear();
    output.reserve(frames * to_count);
    if from_count == 1 {
        for frame in input.as_chunks::<1>().0 {
            for _ in 0..to_count {
                output.push(frame[0]);
            }
        }
        return;
    }
    let at = |frame: &[f32], position: Position| -> f32 {
        channel_index(from, position).map_or(0.0, |index| frame[index])
    };
    match to {
        Layout::Mono => {
            // Fold the stereo downmix to mono: front channels have half gain,
            // centre and surrounds retain the stereo mix coefficients. Other
            // positioned channels contribute at half gain; both LFE channels
            // are deliberately omitted. The sum may exceed full scale.
            for frame in input.chunks_exact(from_count) {
                let mut value = 0.0;
                for (index, sample) in frame.iter().enumerate() {
                    let mask = match from {
                        Channels::Positioned(positions) => (0..64)
                            .filter(|shift| positions.bits() & (1u64 << shift) != 0)
                            .nth(index).map_or(0, |shift| 1u64 << shift),
                        Channels::Discrete(_) => [
                            Position::FRONT_LEFT, Position::FRONT_RIGHT,
                            Position::FRONT_CENTER, Position::LFE1,
                            Position::REAR_LEFT, Position::REAR_RIGHT,
                            Position::SIDE_LEFT, Position::SIDE_RIGHT,
                        ].get(index).map_or(0, |position| position.bits()),
                        _ => 0,
                    };
                    if mask & (Position::LFE1.bits() | Position::LFE2.bits()) != 0 {
                        continue;
                    }
                    let gain = if mask == Position::FRONT_CENTER.bits() {
                        CENTRE_MIX
                    } else if mask & (Position::REAR_LEFT | Position::REAR_RIGHT
                        | Position::SIDE_LEFT | Position::SIDE_RIGHT).bits() != 0 {
                        0.5 * CENTRE_MIX
                    } else {
                        0.5
                    };
                    value += sample * gain;
                }
                output.push(value);
            }
        }
        Layout::Stereo => {
            for frame in input.chunks_exact(from_count) {
                let left = at(frame, Position::FRONT_LEFT)
                    + CENTRE_MIX * at(frame, Position::FRONT_CENTER)
                    + CENTRE_MIX * at(frame, Position::REAR_LEFT)
                    + CENTRE_MIX * at(frame, Position::SIDE_LEFT);
                let right = at(frame, Position::FRONT_RIGHT)
                    + CENTRE_MIX * at(frame, Position::FRONT_CENTER)
                    + CENTRE_MIX * at(frame, Position::REAR_RIGHT)
                    + CENTRE_MIX * at(frame, Position::SIDE_RIGHT);
                output.push(left);
                output.push(right);
            }
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn seven_one_and_side_five_one_downmix_by_position() {
        let side = Channels::Positioned(
            Position::FRONT_LEFT
                | Position::FRONT_RIGHT
                | Position::FRONT_CENTER
                | Position::LFE1
                | Position::SIDE_LEFT
                | Position::SIDE_RIGHT,
        );
        let mut output = Vec::new();
        mix_into(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0], &side, Layout::Stereo, &mut output);
        assert!((output[0] - CENTRE_MIX).abs() < 1e-6 && output[1] == 0.0, "{output:?}");

        let seven = Channels::Positioned(
            Position::FRONT_LEFT
                | Position::FRONT_RIGHT
                | Position::FRONT_CENTER
                | Position::LFE1
                | Position::REAR_LEFT
                | Position::REAR_RIGHT
                | Position::SIDE_LEFT
                | Position::SIDE_RIGHT,
        );
        mix_into(&[0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], &seven, Layout::Stereo, &mut output);
        assert_eq!(output, vec![0.0, 1.0]);
    }

    #[test]
    fn channel_index_follows_position_bit_order() {
        let channels = Channels::Positioned(
            Position::FRONT_LEFT | Position::FRONT_LEFT_CENTER | Position::SIDE_LEFT,
        );
        assert_eq!(channel_index(&channels, Position::FRONT_LEFT), Some(0));
        assert_eq!(channel_index(&channels, Position::FRONT_LEFT_CENTER), Some(1));
        assert_eq!(channel_index(&channels, Position::SIDE_LEFT), Some(2));
        assert_eq!(channel_index(&channels, Position::FRONT_RIGHT), None);
    }

    #[test]
    fn any_single_positioned_channel_is_mono() {
        assert_eq!(layout_of(&Channels::Positioned(Position::FRONT_LEFT)), Some(Layout::Mono));
        assert_eq!(layout_of(&Channels::Positioned(Position::FRONT_CENTER)), Some(Layout::Mono));
    }

    use super::*;

    fn positioned(bits: u64) -> Channels {
        Position::from_bits(bits).map(Channels::Positioned).unwrap()
    }

    fn stereo() -> Channels {
        positioned(Position::FRONT_LEFT.bits() | Position::FRONT_RIGHT.bits())
    }

    fn mono() -> Channels {
        positioned(Position::FRONT_CENTER.bits())
    }

    fn five_one() -> Channels {
        positioned(
            Position::FRONT_LEFT.bits()
                | Position::FRONT_RIGHT.bits()
                | Position::FRONT_CENTER.bits()
                | Position::LFE1.bits()
                | Position::REAR_LEFT.bits()
                | Position::REAR_RIGHT.bits(),
        )
    }

    #[test]
    fn passthrough_is_bit_exact() {
        let input = vec![0.1f32, -0.2, 0.3, -0.4];
        let mut output = Vec::new();
        mix_into(&input, &stereo(), Layout::Stereo, &mut output);
        assert_eq!(output, input);
    }

    #[test]
    fn mono_fans_out_to_stereo() {
        let mut output = Vec::new();
        mix_into(&[0.5, -0.5], &mono(), Layout::Stereo, &mut output);
        assert_eq!(output, vec![0.5, 0.5, -0.5, -0.5]);
    }

    #[test]
    fn stereo_averages_to_mono() {
        let mut output = Vec::new();
        mix_into(&[1.0, -1.0, 0.6, 0.2], &stereo(), Layout::Mono, &mut output);
        assert_eq!(output, vec![0.0, 0.4]);
    }

    #[test]
    fn five_one_downmixes_with_centre_and_surround() {
        let mut frame = [0.0f32; 6];
        frame[0] = 1.0;
        frame[1] = 0.5;
        frame[2] = 1.0;
        frame[4] = 1.0;
        frame[5] = 0.5;
        let mut output = Vec::new();
        mix_into(&frame, &five_one(), Layout::Stereo, &mut output);
        assert_eq!(output.len(), 2);
        let left = 1.0 + CENTRE_MIX + CENTRE_MIX;
        let right = 0.5 + CENTRE_MIX + 0.5 * CENTRE_MIX;
        assert!((output[0] - left).abs() < 1e-6);
        assert!((output[1] - right).abs() < 1e-6);
    }

    #[test]
    fn mono_output_keeps_surround_and_centre_after_track_switch() {
        let mut output = Vec::new();
        mix_into(&[0.25], &mono(), Layout::Mono, &mut output);
        assert_eq!(output, vec![0.25]);
        for (channel, expected) in [0.5, 0.5, CENTRE_MIX, 0.0,
            0.5 * CENTRE_MIX, 0.5 * CENTRE_MIX].into_iter().enumerate() {
            let mut impulse = [0.0; 6];
            impulse[channel] = 1.0;
            mix_into(&impulse, &five_one(), Layout::Mono, &mut output);
            assert!((output[0] - expected).abs() < 1e-6, "channel {channel}: {output:?}");
        }
    }

    #[test]
    fn lfe_is_dropped_in_stereo_downmix() {
        let mut frame = [0.0f32; 6];
        frame[3] = 1.0;
        let mut output = Vec::new();
        mix_into(&frame, &five_one(), Layout::Stereo, &mut output);
        assert_eq!(output, vec![0.0, 0.0]);
    }

    #[test]
    fn discrete_channels_map_canonically() {
        assert_eq!(layout_of(&Channels::Discrete(2)), Some(Layout::Stereo));
        let mut output = Vec::new();
        mix_into(
            &[1.0, 0.0],
            &Channels::Discrete(2),
            Layout::Mono,
            &mut output,
        );
        assert_eq!(output, vec![0.5]);
    }

    #[test]
    fn silence_stays_silent() {
        let mut output = vec![9.0];
        mix_into(&[0.0; 12], &five_one(), Layout::Stereo, &mut output);
        assert_eq!(output, vec![0.0, 0.0, 0.0, 0.0]);
    }
}
