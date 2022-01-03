use crate::math::Float;

impl Float for f32 {
    fn abs(self) -> Self {
        libm::fabsf(self)
    }

    fn sqrt(self) -> Self {
        libm::sqrtf(self)
    }

    fn exp(self) -> Self {
        libm::expf(self)
    }

    fn ceil(self) -> Self {
        libm::ceilf(self)
    }

    fn trunc(self) -> Self {
        libm::truncf(self)
    }

    fn fract(self) -> Self {
        self - self.trunc()
    }

    fn log10(self) -> Self {
        libm::log10f(self)
    }

    fn powf(self, n: Self) -> Self {
        libm::powf(self, n)
    }

    fn powi(mut self, mut rhs: i32) -> Self {
        let mut r = 1.0;
        let invert = if rhs < 0 {
            rhs *= -1;
            true
        } else {
            false
        };
        loop {
            if rhs % 2 == 1 {
                r *= self;
            }
            rhs /= 2;
            if rhs == 0 {
                break;
            }
            self *= self;
        }
        if invert {
            1.0 / r
        } else {
            r
        }
    }

    fn sin(self) -> Self {
        libm::sinf(self)
    }

    fn rem_euclid(self, rhs: Self) -> Self {
        let r = self % rhs;
        if r < 0.0 {
            r + rhs.abs()
        } else {
            r
        }
    }
}

impl Float for f64 {
    fn abs(self) -> Self {
        libm::fabs(self)
    }

    fn sqrt(self) -> Self {
        libm::sqrt(self)
    }

    fn exp(self) -> Self {
        libm::exp(self)
    }

    fn ceil(self) -> Self {
        libm::ceil(self)
    }

    fn trunc(self) -> Self {
        libm::trunc(self)
    }

    fn fract(self) -> Self {
        self - self.trunc()
    }

    fn log10(self) -> Self {
        libm::log10(self)
    }

    fn powf(self, n: Self) -> Self {
        libm::pow(self, n)
    }

    fn powi(mut self, mut rhs: i32) -> Self {
        let mut r = 1.0;
        let invert = if rhs < 0 {
            rhs *= -1;
            true
        } else {
            false
        };
        loop {
            if rhs % 2 == 1 {
                r *= self;
            }
            rhs /= 2;
            if rhs == 0 {
                break;
            }
            self *= self;
        }
        if invert {
            1.0 / r
        } else {
            r
        }
    }

    fn sin(self) -> Self {
        libm::sin(self)
    }

    fn rem_euclid(self, rhs: Self) -> Self {
        let r = self % rhs;
        if r < 0.0 {
            r + rhs.abs()
        } else {
            r
        }
    }
}
