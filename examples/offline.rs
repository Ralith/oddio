use std::{fs::File, path::Path};

const DURATION_SECS: u32 = 3;
const RATE: u32 = 44100;
const FRAME_SIZE: usize = 512;
const SPEED: f32 = 50.0;

fn main() {
    let boop = oddio::Frames::from_iter(
        RATE,
        // Generate a simple sine wave
        (0..RATE * DURATION_SECS).map(|i| {
            let t = i as f32 / RATE as f32;
            (t * 500.0 * 2.0 * std::f32::consts::PI).sin() * 80.0
        }),
    );
    let (mut scene_handle, scene) = oddio::spatial(RATE, 0.1);
    scene_handle.play(
        oddio::FramesSignal::from(boop),
        [-SPEED, 10.0, 0.0].into(),
        [SPEED, 0.0, 0.0].into(),
        1000.0,
    );

    let mut frames = vec![[0.0; 2]; (RATE * DURATION_SECS) as usize];
    for chunk in frames.chunks_mut(FRAME_SIZE) {
        oddio::run(&scene, RATE, chunk);
    }

    let track = wav::BitDepth::Sixteen(
        frames
            .into_iter()
            .flat_map(|ch| (0..2).map(move |i| ch[i]))
            .map(|x| (x * i16::MAX as f32) as i16)
            .collect(),
    );

    let mut out_file = File::create(Path::new("a.wav")).unwrap();
    wav::write(wav::Header::new(1, 2, RATE, 16), track, &mut out_file).unwrap();
}
