# Oddio

Oddio is a game-oriented audio library that is:

- **Lightweight**: Fast compilation, few dependencies, and a simple interface
- **Sans I/O**: Send output wherever you like
- **Real-time**: Audio output is efficient and wait-free: no glitches until you run out of CPU
- **3D**: Spatialization with doppler effects and propagation delay available out of the box
- **Extensible**: Implement `Source` for custom streaming synthesis and filtering
- **Composable**: `Source`s can be transformed without obstructing the inner `Source`'s controls

### Example

```rust
let (mut remote, mixer) = oddio::mixer();

// In audio callback:
let out_frames = oddio::frame_stereo(data);
oddio::run(&mixer, output_sample_rate, out_frames);

// In game logic:
let frames = oddio::FramesSource::from(oddio::Frames::from_slice(sample_rate, &frames));
let mut handle = remote.play(oddio::Spatial::new(frames, position, velocity));

// When position/velocity changes:
handle.control::<oddio::Spatial<_>, _>().set_motion(position, velocity);
```
