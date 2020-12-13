use std::{fs::File, path::Path};

const DURATION_SECS: u32 = 3;
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
    let boop = oddio::Spatial::new(
        oddio::SamplesSource::from(boop),
        [-SPEED, 10.0, 0.0].into(),
        [SPEED, 0.0, 0.0].into(),
    );

    let (mut remote, mut worker) = oddio::worker();
    remote.play(boop);

    let mut samples = vec![[0.0; 2]; (RATE * DURATION_SECS) as usize];
    for chunk in samples.chunks_mut(FRAME_SIZE) {
        oddio::run(&mut worker, RATE, chunk);
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
