use std::{fs::File, path::Path};
use oddio::Source;

const DURATION_SECS: u32 = 6;
const RATE: u32 = 44100;
const FRAME_SIZE: usize = 512;
const SPEED: f32 = 50.0;

fn main() {
    let boop = oddio::Samples::from_iter(
        RATE,
        // Generate a simple sine wave
        (0..RATE * DURATION_SECS).map(|i| {
            let t = i as f32 / RATE as f32;
            (t * 500.0 * 2.0 * std::f32::consts::PI).sin() * 80.0
        }),
    );
    let mut boop = oddio::SamplesSource::new(boop, 0.0);
    let mut boop_state = oddio::State::new([-SPEED, 10.0, 0.0].into());

    let mut samples = vec![[0.0; 2]; (RATE * DURATION_SECS) as usize];
    for i in 0..(samples.len() / FRAME_SIZE) {
        let t = ((i + 1) * FRAME_SIZE) as f64 / RATE as f64;
        let mut mixer = oddio::Mixer {
            samples: &mut samples[i as usize * FRAME_SIZE..(i as usize + 1) * FRAME_SIZE],
            rate: RATE,
        };
        mixer.mix(oddio::Input {
            source: &boop,
            state: &mut boop_state,
            position_wrt_listener: [-SPEED + SPEED * t as f32, 10.0, 0.0].into(),
        });
        boop.advance(FRAME_SIZE as f32);
    }

    let track = wav::BitDepth::Sixteen(
        samples
            .into_iter()
            .flat_map(|ch| (0..2).map(move |i| ch[i]))
            .map(|x| (x * i16::MAX as f32) as i16)
            .collect(),
    );

    let mut out_file = File::create(Path::new("a.wav")).unwrap();
    wav::write(wav::Header::new(1, 2, RATE, 16), track, &mut out_file).unwrap();
}
