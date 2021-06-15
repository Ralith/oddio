const DURATION_SECS: u32 = 2;
const RATE: u32 = 44100;
const BLOCK_SIZE: usize = 512;

const QUIET_AMPLITUDE: f32 = 0.001;

fn main() {
    let mixer = oddio::Adapt::new(
        oddio::Mixer::new(),
        QUIET_AMPLITUDE / 2.0f32.sqrt(),
        oddio::AdaptOptions {
            tau: 0.1,
            max_gain: 1e6,
            low: 0.1 / 2.0f32.sqrt(),
            high: 0.5 / 2.0f32.sqrt(),
        },
    );
    let (mut mixer, split) = oddio::split(mixer);

    let quiet = oddio::Gain::new(oddio::Sine::new(0.0, 5e2), QUIET_AMPLITUDE);
    let loud = oddio::Gain::new(oddio::Sine::new(0.0, 4e2), 0.8);

    mixer.control::<oddio::Mixer<f32>, _>().play(quiet);

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create("a.wav", spec).unwrap();

    let mut drive = || {
        for _ in 0..(RATE * DURATION_SECS / BLOCK_SIZE as u32) {
            let mut block = [0.0; BLOCK_SIZE];
            oddio::run(&split, RATE, &mut block);
            for &sample in &block {
                writer
                    .write_sample((sample * i16::MAX as f32) as i16)
                    .unwrap();
            }
        }
    };

    drive();
    let mut handle = mixer.control::<oddio::Mixer<f32>, _>().play(loud);
    drive();
    handle.control::<oddio::Stop<_>, _>().stop();
    drive();

    writer.finalize().unwrap();
}
