#![warn(missing_docs)]
//! A library for playing 3D audio in real time

use std::cmp;
use std::sync::Arc;

use slab::Slab;

/// A collection of spatialized sources that can be heard together
pub struct Scene {
    live_sources: Vec<Source>,
    sources: Slab<SourceData>,
}

impl Scene {
    /// Construct an empty scene
    pub fn new() -> Self {
        Self {
            live_sources: Vec::new(),
            sources: Slab::new(),
        }
    }

    /// Play `sound` at `start_time`
    pub fn insert(
        &mut self,
        sound: Arc<[f32]>,
        start_time: u32,
        position: na::Point3<f32>,
        velocity: na::Vector3<f32>,
    ) -> Source {
        let i = self.sources.insert(SourceData {
            sound,
            live_idx: self.live_sources.len(),
            start_time,
            position,
            velocity,
            radius: 0.01f32,
        });
        let v = Source(i);
        self.live_sources.push(v);
        v
    }

    /// Update the position and velocity of a source
    pub fn update(
        &mut self,
        source: Source,
        position: na::Point3<f32>,
        velocity: na::Vector3<f32>,
    ) {
        // TODO: Allow mixing to lerp between positions
        let source = &mut self.sources[source.0];
        source.position = position;
        source.velocity = velocity;
    }

    /// Stop playing `source`
    pub fn remove(&mut self, source: Source) {
        let data = self.sources.remove(source.0);
        let live_source = self.live_sources.swap_remove(data.live_idx);
        debug_assert_eq!(live_source, source);
        if let Some(moved) = self.live_sources.get_mut(data.live_idx) {
            self.sources[moved.0].live_idx = data.live_idx;
        }
    }

    /// Generate audio samples for a particular stereo listener
    pub fn mix(
        &self,
        now: u32,
        listener: &na::Isometry3<f32>,
        velocity: &na::Vector3<f32>,
        out: &mut [f32],
    ) {
        let _ = velocity; // TODO: Doppler effect
        let left_ear = listener * na::Translation3::new(-EAR_RADIUS, 0.0, 0.0);
        let right_ear = listener * na::Translation3::new(EAR_RADIUS, 0.0, 0.0);
        assert_eq!(out.len() % 2, 0);
        for s in &self.live_sources {
            let source = &self.sources[s.0];
            source.mix(&left_ear, now, out, 0);
            source.mix(&right_ear, now, out, 1);
        }
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

/// A sound currently being played from a particular location
struct SourceData {
    sound: Arc<[f32]>,
    live_idx: usize,
    position: na::Point3<f32>,
    velocity: na::Vector3<f32>,
    start_time: u32,
    /// Loudness within this radius is constant.
    radius: f32,
}

impl SourceData {
    fn mix(&self, ear: &na::Isometry3<f32>, now: u32, out: &mut [f32], channel: usize) {
        let distance = na::distance(&(ear * na::Point3::origin()), &self.position);
        let delay = seconds_to_samples(SAMPLE_RATE, distance / SPEED_OF_SOUND);
        let start_time = self.start_time + delay;
        if let Some(c) = Cursors::compute(
            now,
            start_time,
            self.sound.len() as u32,
            (out.len() / 2) as u32,
        ) {
            // Energy = amplitude^2, so simple division accomplishes inverse-square falloff
            let attenuation = 1.0 / distance.max(self.radius);
            for i in 0..(c.len as usize) {
                out[2 * (c.out_start as usize + i) + channel] +=
                    self.sound[c.sound_start as usize + i] * attenuation;
            }
        }
    }
}

#[derive(Debug)]
struct Cursors {
    sound_start: u32,
    out_start: u32,
    len: u32,
}

impl Cursors {
    fn compute(now: u32, start_time: u32, sound_samples: u32, out_samples: u32) -> Option<Self> {
        let sound_start = now.saturating_sub(start_time);
        let out_start = start_time.saturating_sub(now);
        let sound_len = sound_samples.checked_sub(sound_start)?;
        let out_len = out_samples.checked_sub(out_start)?;
        Some(Self {
            sound_start,
            out_start,
            len: cmp::min(sound_len, out_len),
        })
    }
}

/// Handle to a particular instance of a sound being played somewhere in a `Scene`.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Source(usize);

/// Hz
const SAMPLE_RATE: f32 = 44100.0;
/// Rate sound travels from sources to listeners (m/s)
const SPEED_OF_SOUND: f32 = 343.0;
/// Distance from center of head to an ear (m)
const EAR_RADIUS: f32 = 0.1075;
/// Rate at which source sources change position
const INTERP_SPEED: f32 = SPEED_OF_SOUND;

fn seconds_to_samples(rate: f32, seconds: f32) -> u32 {
    (seconds * rate) as u32
}

fn samples_to_seconds(rate: f32, samples: u32) -> f32 {
    samples as f32 / rate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors() {
        let x = Cursors::compute(0, 0, 10, 10).unwrap();
        assert_eq!(x.sound_start, 0);
        assert_eq!(x.out_start, 0);
        assert_eq!(x.len, 10);

        let x = Cursors::compute(0, 0, 20, 10).unwrap();
        assert_eq!(x.sound_start, 0);
        assert_eq!(x.out_start, 0);
        assert_eq!(x.len, 10);

        let x = Cursors::compute(0, 0, 10, 20).unwrap();
        assert_eq!(x.sound_start, 0);
        assert_eq!(x.out_start, 0);
        assert_eq!(x.len, 10);

        let x = Cursors::compute(5, 0, 10, 10).unwrap();
        assert_eq!(x.sound_start, 5);
        assert_eq!(x.out_start, 0);
        assert_eq!(x.len, 5);

        let x = Cursors::compute(0, 5, 10, 10).unwrap();
        assert_eq!(x.sound_start, 0);
        assert_eq!(x.out_start, 5);
        assert_eq!(x.len, 5);
    }

    #[test]
    fn insert_remove() {
        let mut scene = Scene::new();
        let sound: Arc<[f32]> = Arc::from(&[][..]);
        let a = scene.insert(sound.clone(), 0, na::Point3::origin(), na::zero());
        scene.insert(sound.clone(), 0, na::Point3::origin(), na::zero());
        scene.remove(a);
        let b = scene.insert(sound.clone(), 0, na::Point3::origin(), na::zero());
        assert_eq!(a, b);
    }

    #[test]
    fn travel_time() {
        let mut scene = Scene::new();
        let sound: Arc<[f32]> = Arc::from(&[1.0][..]);
        scene.insert(
            sound.clone(),
            0,
            na::Point3::new(-0.1, 0.0, 0.0),
            na::zero(),
        );

        let mut buf = [0.0; 64];
        scene.mix(0, &na::one(), &na::zero(), &mut buf);
        let left_pos = buf.iter().step_by(2).position(|&x| x != 0.0).unwrap();
        let right_pos = buf
            .iter()
            .skip(1)
            .step_by(2)
            .position(|&x| x != 0.0)
            .unwrap();
        assert!(left_pos < right_pos, "sound arrives at left ear first");
        assert!(
            buf[left_pos * 2] > buf[right_pos * 2 + 1],
            "sound is louder at left ear"
        );

        // Listener rotated 180 degrees
        let mut buf = [0.0; 64];
        scene.mix(
            0,
            &na::convert(na::UnitQuaternion::from_axis_angle(
                &na::Vector3::z_axis(),
                std::f32::consts::PI,
            )),
            &na::zero(),
            &mut buf,
        );
        let left_pos = buf.iter().step_by(2).position(|&x| x != 0.0).unwrap();
        let right_pos = buf
            .iter()
            .skip(1)
            .step_by(2)
            .position(|&x| x != 0.0)
            .unwrap();
        assert!(left_pos > right_pos, "sound arrives at right ear first");
        assert!(
            buf[left_pos * 2] < buf[right_pos * 2 + 1],
            "sound is louder at right ear"
        );
    }

    #[test]
    fn continuity() {
        let mut scene = Scene::new();
        let mut sound = Vec::with_capacity(1024);
        for i in 0..sound.capacity() {
            sound.push(i as f32 / (sound.capacity() - 1) as f32);
        }
        let sound: Arc<[f32]> = Arc::from(&sound[..]);
        scene.insert(
            sound.clone(),
            0,
            na::Point3::new(-EAR_RADIUS, 0.0, 0.0),
            na::zero(),
        );

        let mut buf = [0.0; 64];
        let mut time = 0;
        let mut out = Vec::new();
        loop {
            scene.mix(time, &na::one(), &na::zero(), &mut buf);
            if buf.iter().all(|&x| x == 0.0) {
                break;
            }
            out.extend(buf.iter().cloned().step_by(2));
            time += (buf.len() / 2) as u32;
            buf = [0.0; 64];
        }
        println!("{:?}", out);
        let prefix = out.iter().position(|&x| x != 0.0).unwrap() - 1;
        assert!(out.len() >= prefix + sound.len());
        let out = &out[prefix..prefix + sound.len()];
        for x in out.windows(3) {
            let diff1 = x[1] - x[0];
            let diff2 = x[2] - x[1];
            assert!((diff2 - diff1).abs() < 0.0001);
        }
    }
}
