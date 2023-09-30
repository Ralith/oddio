const DURATION_SECS: u32 = 2;
const RATE: u32 = 44100;
const BLOCK_SIZE: usize = 512;

fn main() {
    let (mut mixer, signal) = oddio::Mixer::new();
    let mut signal = oddio::Adapt::new(
        signal,
        1e-3 / 2.0f32.sqrt(),
        oddio::AdaptOptions {
            tau: 0.1,
            max_gain: 1e6,
            low: 0.1 / 2.0f32.sqrt(),
            high: 0.5 / 2.0f32.sqrt(),
        },
    );

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create("adapt.wav", spec).unwrap();

    let mut drive = || {
        for _ in 0..(RATE * DURATION_SECS / BLOCK_SIZE as u32) {
            let mut block = [0.0; BLOCK_SIZE];
            oddio::run(&mut signal, RATE, &mut block);
            for &sample in &block {
                writer
                    .write_sample((sample * i16::MAX as f32) as i16)
                    .unwrap();
            }
        }
    };

    let quiet = oddio::FixedGain::new(oddio::Sine::new(0.0, 5e2), -60.0);
    let loud = oddio::FixedGain::new(oddio::Sine::new(0.0, 4e2), -2.0);

    mixer.play(quiet);
    drive();
    let mut handle = mixer.play(loud);
    drive();
    handle.stop();
    drive();

    writer.finalize().unwrap();
}
