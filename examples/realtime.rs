use std::{
    thread,
    time::{Duration, Instant},
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const DURATION_SECS: u32 = 6;
const BUFFER_SIZE_MS: u32 = 100;

fn main() {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no output device available");
    let sample_rate = device.default_output_config().unwrap().sample_rate();
    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate,
        buffer_size: cpal::BufferSize::Fixed(sample_rate.0 / (1000 / BUFFER_SIZE_MS)),
    };
    let boop = oddio::Samples::from_iter(
        sample_rate.0,
        // Generate a simple sine wave
        (0..sample_rate.0 * DURATION_SECS).map(|i| {
            let t = i as f32 / sample_rate.0 as f32;
            (t * 500.0 * 2.0 * std::f32::consts::PI).sin() * 80.0
        }),
    );

    let speed = 50.0;

    let source = oddio::Spatial::new(
        oddio::SamplesSource::from(boop),
        [-speed, 10.0, 0.0].into(),
        [speed, 0.0, 0.0].into(),
    );

    let (mut remote, mut worker) = oddio::worker();

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let samples = oddio::frame_stereo(data);
                for s in &mut samples[..] {
                    *s = [0.0, 0.0];
                }
                oddio::run(&mut worker, sample_rate.0, samples);
            },
            move |err| {
                eprintln!("{}", err);
            },
        )
        .unwrap();
    stream.play().unwrap();

    let mut source = remote.play(source);

    let start = Instant::now();

    loop {
        thread::sleep(Duration::from_millis(50));
        let dt = start.elapsed();
        if dt >= Duration::from_secs(DURATION_SECS as u64) {
            break;
        }
        // This is in principle a no-op because the velocity isn't changing, but due to imprecise
        // sleep times and the fact that the audio thread runs at unaligned intervals means that the
        // this would produce glitches if not for smoothing done by the worker.
        source.set_motion(
            [-speed + speed * dt.as_secs_f32(), 10.0, 0.0].into(),
            [speed, 0.0, 0.0].into(),
        );
    }
}
