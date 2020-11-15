pub fn norm(x: mint::Vector3<f32>) -> f32 {
    x.as_ref().iter().map(|&x| x.powi(2)).sum::<f32>().sqrt()
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
