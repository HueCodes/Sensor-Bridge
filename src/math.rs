//! Math helpers that work in both `std` and `no_std`.
//!
//! With `std` enabled, these forward to the inherent methods on `f32` / `f64`;
//! without `std`, they forward to `libm`. Keeping the call sites uniform means
//! callers don't need `cfg` fences sprinkled through numeric code.

#[inline]
pub(crate) fn sqrt_f32(x: f32) -> f32 {
    #[cfg(feature = "std")]
    {
        x.sqrt()
    }
    #[cfg(not(feature = "std"))]
    {
        libm::sqrtf(x)
    }
}

#[inline]
pub(crate) fn sqrt_f64(x: f64) -> f64 {
    #[cfg(feature = "std")]
    {
        x.sqrt()
    }
    #[cfg(not(feature = "std"))]
    {
        libm::sqrt(x)
    }
}

#[inline]
pub(crate) fn ceil_f64(x: f64) -> f64 {
    #[cfg(feature = "std")]
    {
        x.ceil()
    }
    #[cfg(not(feature = "std"))]
    {
        libm::ceil(x)
    }
}

#[inline]
pub(crate) fn sin_cos_f32(x: f32) -> (f32, f32) {
    #[cfg(feature = "std")]
    {
        x.sin_cos()
    }
    #[cfg(not(feature = "std"))]
    {
        libm::sincosf(x)
    }
}
