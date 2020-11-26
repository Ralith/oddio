# Oddio

Oddio is a game-oriented audio library that is:

- **Lightweight**: Fast compilation, few dependencies, and a simple interface
- **Sans I/O**: Send output wherever you like
- **Real-time**: Audio output is efficient and wait-free: no glitches until you run out of CPU
- **3D**: Spatialization with doppler effects and propagation delay available out of the box
- **Extensible**: Implement `Source` for custom streaming synthesis and filtering

### Example

```rust
let (mut remote, mut worker) = oddio::worker();

// In audio callback:
let samples = oddio::frame_stereo(data);
for s in &mut samples[..] {
   *s = [0.0, 0.0];
}
worker.render(output_sample_rate, samples);

// In game logic:
let samples = oddio::SamplesSource::from(oddio::Samples::from_slice(sample_rate, &samples));
let mut handle = remote.play(oddio::Spatial::new(samples, position, velocity));

// When position/velocity changes:
handle.set_motion(position, velocity);
```
