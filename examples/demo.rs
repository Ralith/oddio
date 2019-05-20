use std::sync::Arc;

use cpal::{SampleRate, StreamData, UnknownTypeOutputBuffer, traits::{HostTrait, EventLoopTrait, DeviceTrait}};
use oddio::Scene;

const SAMPLE_RATE: SampleRate = SampleRate(44100);

fn main() {
    let host = cpal::default_host();
    let event_loop = host.event_loop();
    let device = host.default_output_device().expect("no output device available");
    let supported_formats_range = device
        .supported_output_formats()
        .expect("error while querying formats");
    supported_formats_range
        .filter(|x| {
            x.channels == 2
                && x.data_type == cpal::SampleFormat::F32
                && x.min_sample_rate <= SAMPLE_RATE
                && x.max_sample_rate >= SAMPLE_RATE
        })
        .next()
        .expect("no compatible format");

    let format = cpal::Format {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        data_type: cpal::SampleFormat::F32,
    };
    let stream = event_loop.build_output_stream(&device, &format).unwrap();
    event_loop.play_stream(stream).unwrap();

    let mut scene = Scene::new();
    let mut time = 0u32;

    let mut sound = Vec::with_capacity(SAMPLE_RATE.0 as usize * 100);
    for x in 0..sound.capacity() {
        let t = x as f32 / SAMPLE_RATE.0 as f32;
        sound.push((t * 300.0 * 2.0 * std::f32::consts::PI).sin() * 0.5);
    }
    let sound = Arc::from(sound);

    let mut handle = scene.insert(sound, 0, na::Point3::new(0.0, 0.0, -1.0), na::zero());

    event_loop.run(move |_stream_id, stream_data| match stream_data.unwrap() {
        StreamData::Output {
            buffer: UnknownTypeOutputBuffer::F32(mut buffer),
        } => {
            let buffer: &mut [f32] = &mut *buffer;
            for elem in &mut buffer[..] {
                *elem = 0.0;
            }
            scene.mix(time, &na::one(), &na::zero(), buffer);
            time = time.wrapping_add((buffer.len() / 2) as u32);
            let t = time as f32 / SAMPLE_RATE.0 as f32;
            scene.update(handle, na::Point3::new((2.0*t).sin(), 0.0, -1.0), na::zero());
        }
        _ => unreachable!(),
    });
}
