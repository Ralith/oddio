# Oddio

[![Documentation](https://docs.rs/oddio/badge.svg)](https://docs.rs/oddio/)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE-APACHE)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE-MIT)

Oddio is a game-oriented audio library that is:

- **Lightweight**: Fast compilation, few dependencies, and a simple interface
- **Sans I/O**: Send output wherever you like
- **Real-time**: Audio output is efficient and wait-free: no glitches until you run out of CPU
- **3D**: Spatialization with doppler effects and propagation delay available out of the box
- **Extensible**: Implement `Signal` for custom streaming synthesis and filtering
- **Composable**: `Signal`s can be transformed without obstructing the inner `Signal`'s controls

### Example

```rust
let (mut scene_handle, scene) = oddio::split(oddio::SpatialScene::new());

// In audio callback:
let out_frames = oddio::frame_stereo(data);
oddio::run(&scene, output_sample_rate, out_frames);

// In game logic:
let frames = oddio::FramesSignal::from(oddio::Frames::from_slice(sample_rate, &frames));
let mut handle = scene_handle.control::<oddio::SpatialScene, _>()
    .play(frames, oddio::SpatialOptions { position, velocity, ..Default::default() });

// When position/velocity changes:
handle.control::<oddio::Spatial<_>, _>().set_motion(position, velocity, false);
```

## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
