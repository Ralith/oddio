const DURATION_SECS: u32 = 3;
const RATE: u32 = 44100;
const BLOCK_SIZE: usize = 512;
const SPEED: f32 = 50.0;

fn main() {
    let boop = oddio::Frames::from_iter(
        RATE,
        // Generate a simple sine wave
        (0..RATE * DURATION_SECS).map(|i| {
            let t = i as f32 / RATE as f32;
            (t * 500.0 * 2.0 * core::f32::consts::PI).sin() * 80.0
        }),
    );
    let (mut scene_handle, mut scene) = oddio::SpatialScene::new();
    scene_handle.play(
        oddio::FramesSignal::from(boop),
        oddio::SpatialOptions {
            position: [-SPEED, 10.0, 0.0].into(),
            velocity: [SPEED, 0.0, 0.0].into(),
            radius: 0.1,
        },
    );

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create("offline.wav", spec).unwrap();

    for _ in 0..(RATE * DURATION_SECS / BLOCK_SIZE as u32) {
        let mut block = [[0.0; 2]; BLOCK_SIZE];
        oddio::run(&mut scene, RATE, &mut block);
        for &frame in &block {
            for &sample in &frame {
                writer
                    .write_sample((sample * i16::MAX as f32) as i16)
                    .unwrap();
            }
        }
    }

    writer.finalize().unwrap();
}
