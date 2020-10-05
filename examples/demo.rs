use std::{thread, time::Duration};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const DURATION_SECS: u32 = 6;

fn main() {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no output device available");
    let sample_rate = device.default_output_config().unwrap().sample_rate();
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate,
        buffer_size: cpal::BufferSize::Default,
    };
    let boop = oddio::Sound::from_iter(
        sample_rate.0,
        // Generate a simple sine wave
        (0..sample_rate.0 * DURATION_SECS).map(|i| {
            let t = i as f32 / sample_rate.0 as f32;
            (t * 500.0 * 2.0 * std::f32::consts::PI).sin() * 100.0
        }),
    );
    let mut boop_state = oddio::State::new();
    let speed = 50.0;
    let mut sample = 0;
    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let t = sample as f32 / sample_rate.0 as f32;
                let mut mixer = oddio::Mixer {
                    samples: data,
                    rate: sample_rate.0,
                    velocity: [0.0; 3].into(),
                };
                mixer.mix(oddio::Input {
                    sound: &boop,
                    state: &mut boop_state,
                    position_wrt_listener: [-speed + speed * t, 10.0, 0.0].into(),
                    velocity: [speed, 0.0, 0.0].into(),
                });
                sample += data.len() as u64;
            },
            move |err| {
                eprintln!("{}", err);
            },
        )
        .unwrap();
    stream.play().unwrap();
    thread::sleep(Duration::from_secs(DURATION_SECS as u64));
}
