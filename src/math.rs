use libm::{fabsf, sqrtf, trunc, truncf};

pub fn powi(mut lhs: f32, mut rhs: i32) -> f32 {
    let mut r = 1.0;
    let invert = if rhs < 0 {
        rhs *= -1;
        true
    } else {
        false
    };
    loop {
        if rhs % 2 == 1 {
            r *= lhs;
        }
        rhs /= 2;
        if rhs == 0 {
            break;
        }
        lhs *= lhs;
    }
    if invert {
        1.0 / r
    } else {
        r
    }
}

pub fn rem_euclid(lhs: f32, rhs: f32) -> f32 {
    let r = lhs % rhs;
    if r < 0.0 {
        r + fabsf(rhs)
    } else {
        r
    }
}

pub fn fract(v: f64) -> f64 {
    v - trunc(v)
}

pub fn fractf(v: f32) -> f32 {
    v - truncf(v)
}

pub fn norm(x: mint::Vector3<f32>) -> f32 {
    sqrtf(x.as_ref().iter().map(|&x| powi(x, 2)).sum::<f32>())
}

pub fn dot(x: mint::Vector3<f32>, y: mint::Vector3<f32>) -> f32 {
    x.as_ref()
        .iter()
        .zip(y.as_ref().iter())
        .map(|(&x, &y)| x * y)
        .sum::<f32>()
}

pub fn scale(v: mint::Vector3<f32>, f: f32) -> mint::Vector3<f32> {
    [v.x * f, v.y * f, v.z * f].into()
}

pub fn sub(a: mint::Point3<f32>, b: mint::Point3<f32>) -> mint::Vector3<f32> {
    [a.x - b.x, a.y - b.y, a.z - b.z].into()
}

pub fn add(a: mint::Point3<f32>, b: mint::Vector3<f32>) -> mint::Point3<f32> {
    [a.x + b.x, a.y + b.y, a.z + b.z].into()
}

pub fn mix(a: mint::Point3<f32>, b: mint::Point3<f32>, r: f32) -> mint::Point3<f32> {
    let ir = 1.0 - r;
    [ir * a.x + r * b.x, ir * a.y + r * b.y, ir * a.z + r * b.z].into()
}

pub fn invert_quat(q: &mint::Quaternion<f32>) -> mint::Quaternion<f32> {
    mint::Quaternion {
        s: q.s,
        v: [-q.v.x, -q.v.y, -q.v.z].into(),
    }
}

fn quat_mul(q: &mint::Quaternion<f32>, r: &mint::Quaternion<f32>) -> mint::Quaternion<f32> {
    mint::Quaternion {
        s: q.s * r.s - q.v.x * r.v.x - q.v.y * r.v.y - q.v.z * r.v.z,
        v: [
            q.s * r.v.x + q.v.x * r.s + q.v.y * r.v.z - q.v.z * r.v.y,
            q.s * r.v.y - q.v.x * r.v.z + q.v.y * r.s + q.v.z * r.v.x,
            q.s * r.v.z + q.v.x * r.v.y - q.v.y * r.v.x + q.v.z * r.s,
        ]
        .into(),
    }
}

pub fn rotate(rot: &mint::Quaternion<f32>, p: &mint::Point3<f32>) -> mint::Point3<f32> {
    quat_mul(
        rot,
        &quat_mul(
            &mint::Quaternion {
                s: 0.0,
                v: (*p).into(),
            },
            &invert_quat(rot),
        ),
    )
    .v
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::PI;

    #[test]
    fn rotate_x() {
        let p = mint::Point3::from([0.0, 0.0, -1.0]);
        let q = axis_angle([1.0, 0.0, 0.0].into(), PI / 2.0);
        let r = rotate(&q, &p);
        assert_eq!(r.x, 0.0);
        assert!((r.y - 1.0).abs() < 1e-3);
        assert_eq!(r.z, 0.0);
    }

    #[test]
    fn rotate_y() {
        let p = mint::Point3::from([1.0, 0.0, 0.0]);
        let q = axis_angle([0.0, 1.0, 0.0].into(), PI / 2.0);
        let r = rotate(&q, &p);
        assert_eq!(r.x, 0.0);
        assert_eq!(r.y, 0.0);
        assert!((r.z + 1.0).abs() < 1e-3);
    }

    #[test]
    fn rotate_z() {
        let p = mint::Point3::from([0.0, 1.0, 0.0]);
        let q = axis_angle([0.0, 0.0, 1.0].into(), PI / 2.0);
        let r = rotate(&q, &p);
        assert_eq!(r.y, 0.0);
        assert!((r.x + 1.0).abs() < 1e-3);
        assert_eq!(r.z, 0.0);
    }

    fn axis_angle(axis: mint::Vector3<f32>, angle: f32) -> mint::Quaternion<f32> {
        let half = angle * 0.5;
        mint::Quaternion {
            s: half.cos(),
            v: [
                axis.x * half.sin(),
                axis.y * half.sin(),
                axis.z * half.sin(),
            ]
            .into(),
        }
    }
}
