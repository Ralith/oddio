/// Helper to linearly ramp a parameter towards a target value
///
/// Useful for implementing filters like [`Gain`](crate::Gain) which have dynamic parameters, where
/// applying changes to parameters directly would cause unpleasant artifacts such as popping.
///
/// # Example
/// ```
/// let mut value = oddio::Smoothed::new(0.0);
/// assert_eq!(value.get(), 0.0);
/// // Changes only take effect after time passes
/// value.set(1.0);
/// assert_eq!(value.get(), 0.0);
/// value.advance(0.5);
/// assert_eq!(value.get(), 0.5);
/// // A new value can be supplied mid-interpolation without causing a discontinuity
/// value.set(1.5);
/// value.advance(0.5);
/// assert_eq!(value.get(), 1.0);
/// value.advance(0.5);
/// assert_eq!(value.get(), 1.5);
/// // Interpolation halts once the target value is reached
/// value.advance(0.5);
/// assert_eq!(value.get(), 1.5);
/// ```
#[derive(Copy, Clone, Default)]
pub struct Smoothed<T> {
    prev: T,
    next: T,
    progress: f32,
}

impl<T> Smoothed<T> {
    /// Create with initial value `x`
    pub fn new(x: T) -> Self
    where
        T: Clone,
    {
        Self {
            prev: x.clone(),
            next: x,
            progress: 0.0,
        }
    }

    /// Advance interpolation by `proportion`. For example, to advance at a fixed sample rate over a
    /// particular smoothing period, pass `sample_interval / smoothing_period`.
    pub fn advance(&mut self, proportion: f32) {
        self.progress = (self.progress + proportion).min(1.0);
    }

    /// Progress from the previous towards the next value
    pub fn progress(&self) -> f32 {
        self.progress
    }

    /// Set the next value to `x`
    pub fn set(&mut self, value: T)
    where
        T: Interpolate,
    {
        self.prev = self.get();
        self.next = value;
        self.progress = 0.0;
    }

    /// Get the current value
    pub fn get(&self) -> T
    where
        T: Interpolate,
    {
        self.prev.interpolate(&self.next, self.progress)
    }
}

/// Types that can be linearly interpolated, for use with [`Smoothed`]
pub trait Interpolate {
    /// Interpolate between `self` and `other` by `t`, which should be in [0, 1]
    fn interpolate(&self, other: &Self, t: f32) -> Self;
}

impl Interpolate for f32 {
    fn interpolate(&self, other: &Self, t: f32) -> Self {
        let diff = other - self;
        self + t * diff
    }
}
