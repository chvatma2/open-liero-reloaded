//! Default values and `skip_serializing_if` predicates for the authoring
//! types. Floats compare by bits: `-0.0` and subnormals are never "the
//! default" (Review Focus 1).

macro_rules! f32_defaults {
    ($($get:ident / $is:ident = $v:expr;)*) => {$(
        #[allow(dead_code)]
        pub(crate) fn $get() -> f32 { $v }
        #[allow(dead_code)]
        pub(crate) fn $is(v: &f32) -> bool { v.to_bits() == ($v as f32).to_bits() }
    )*};
}

f32_defaults! {
    zero / is_zero = 0.0;
    one / is_one = 1.0;
    half / is_half = 0.5;
    tenth / is_tenth = 0.1;
    worm_ground_acceleration / is_worm_ground_acceleration = 8.0;
    worm_air_acceleration / is_worm_air_acceleration = 4.0;
    worm_gravity / is_worm_gravity = 50.0;
    worm_jump_speed / is_worm_jump_speed = -75.0;
    worm_unused_angle_speed / is_worm_unused_angle_speed = 150.0;
    worm_unused_ground_friction / is_worm_unused_ground_friction = 0.4;
    rope_strength / is_rope_strength = 3.5;
}

macro_rules! i32_defaults {
    ($($get:ident / $is:ident = $v:expr;)*) => {$(
        #[allow(dead_code)]
        pub(crate) fn $get() -> i32 { $v }
        #[allow(dead_code)]
        pub(crate) fn $is(v: &i32) -> bool { *v == $v }
    )*};
}

i32_defaults! {
    one_i32 / is_one_i32 = 1;
    rope_max_length / is_rope_max_length = 300;
    rope_rest_length / is_rope_rest_length = 30;
}

#[allow(dead_code)]
pub(crate) fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

#[allow(dead_code)]
pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}

#[allow(dead_code)]
pub(crate) fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_defaults_compare_bits() {
        assert!(is_zero(&0.0));
        assert!(!is_zero(&-0.0), "-0.0 must be written, or it reads back as 0.0");
        assert!(!is_zero(&1e-40), "a subnormal is not zero");
        assert!(is_half(&0.5));
        assert!(!is_half(&0.500_000_06));
        assert!(is_tenth(&0.1));
    }
}
