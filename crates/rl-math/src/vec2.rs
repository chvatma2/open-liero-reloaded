//! OLX `CVec` = `VectorD2<float>` (`include/CVec.h`). Every operation is a
//! plain `f32` operation in the same order as the C++, so results match bit
//! for bit. Scalars are always `f32`: OLX code that scales by an `int`
//! converts it to `float` first, which is what callers do here too.

use core::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// `GetLength2`: `x*x + y*y`.
    pub fn length2(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    /// `GetLength`: `sqrtf(x*x + y*y)`.
    pub fn length(self) -> f32 {
        libm::sqrtf(self.x * self.x + self.y * self.y)
    }

    /// `Normalize`: `*this / GetLength()` (no zero check, like OLX).
    pub fn normalize(self) -> Vec2 {
        self / self.length()
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}

impl Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, s: f32) -> Vec2 {
        Vec2::new(self.x / s, self.y / s)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        *self = *self - o;
    }
}

impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, s: f32) {
        *self = *self * s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations_are_plain_f32() {
        let v = Vec2::new(3.0, 4.0);
        assert_eq!(v.length2(), 25.0);
        assert_eq!(v.length(), 5.0);
        assert_eq!(v.normalize(), Vec2::new(0.6, 0.8));
        assert_eq!((v * 2.0 - Vec2::new(1.0, 1.0)) / 2.0, Vec2::new(2.5, 3.5));
        assert_eq!(-v, Vec2::new(-3.0, -4.0));
        let mut w = v;
        w += Vec2::new(1.0, 0.0);
        w *= 0.5;
        assert_eq!(w, Vec2::new(2.0, 2.0));
    }

    #[test]
    fn sqrt_is_correctly_rounded() {
        assert_eq!(Vec2::new(1.0, 1.0).length(), core::f32::consts::SQRT_2);
    }
}
