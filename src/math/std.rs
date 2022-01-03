use crate::math::Float;

impl Float for f32 {
    fn abs(self) -> Self {
        Self::abs(self)
    }

    fn sqrt(self) -> Self {
        Self::sqrt(self)
    }

    fn exp(self) -> Self {
        Self::exp(self)
    }

    fn ceil(self) -> Self {
        Self::ceil(self)
    }

    fn trunc(self) -> Self {
        Self::trunc(self)
    }

    fn fract(self) -> Self {
        Self::fract(self)
    }

    fn log10(self) -> Self {
        Self::log10(self)
    }

    fn powf(self, n: Self) -> Self {
        Self::powf(self, n)
    }

    fn powi(self, n: i32) -> Self {
        Self::powi(self, n)
    }

    fn sin(self) -> Self {
        Self::sin(self)
    }

    fn rem_euclid(self, rhs: Self) -> Self {
        Self::rem_euclid(self, rhs)
    }

    fn tanh(self) -> Self {
        Self::tanh(self)
    }
}

impl Float for f64 {
    fn abs(self) -> Self {
        Self::abs(self)
    }

    fn sqrt(self) -> Self {
        Self::sqrt(self)
    }

    fn exp(self) -> Self {
        Self::exp(self)
    }

    fn ceil(self) -> Self {
        Self::ceil(self)
    }

    fn trunc(self) -> Self {
        Self::trunc(self)
    }

    fn fract(self) -> Self {
        Self::fract(self)
    }

    fn log10(self) -> Self {
        Self::log10(self)
    }

    fn powf(self, n: Self) -> Self {
        Self::powf(self, n)
    }

    fn powi(self, n: i32) -> Self {
        Self::powi(self, n)
    }

    fn sin(self) -> Self {
        Self::sin(self)
    }

    fn rem_euclid(self, rhs: Self) -> Self {
        Self::rem_euclid(self, rhs)
    }

    fn tanh(self) -> Self {
        Self::tanh(self)
    }
}
