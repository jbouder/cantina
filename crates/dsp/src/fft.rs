//! A radix-2 complex FFT for the convolver: in place, with the
//! twiddles computed once, and nothing allocated after construction.

use std::f32::consts::PI;
use std::ops::{Add, Mul, Sub};

/// A complex number as a pair.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Complex {
    pub re: f32,
    pub im: f32,
}

impl Complex {
    pub const ZERO: Complex = Complex { re: 0.0, im: 0.0 };
}

impl Mul for Complex {
    type Output = Complex;

    #[inline]
    fn mul(self, other: Complex) -> Complex {
        Complex {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }
}

impl Add for Complex {
    type Output = Complex;

    #[inline]
    fn add(self, other: Complex) -> Complex {
        Complex {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }
}

impl Sub for Complex {
    type Output = Complex;

    #[inline]
    fn sub(self, other: Complex) -> Complex {
        Complex {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }
}

/// An FFT of one power-of-two size.
#[derive(Debug, Clone)]
pub struct Fft {
    size: usize,
    /// Forward twiddles `e^(-2πik/N)` for `k` in `0..N/2`.
    twiddles: Vec<Complex>,
    /// Bit-reversed index of every position.
    reversed: Vec<usize>,
}

impl Fft {
    /// An FFT of `size` points, which must be a power of two.
    pub fn new(size: usize) -> Self {
        assert!(size.is_power_of_two() && size >= 2, "FFT size {size}");
        let twiddles = (0..size / 2)
            .map(|k| {
                let angle = -2.0 * PI * k as f32 / size as f32;
                Complex {
                    re: angle.cos(),
                    im: angle.sin(),
                }
            })
            .collect();
        let bits = size.trailing_zeros();
        let reversed = (0..size)
            .map(|i| i.reverse_bits() >> (usize::BITS - bits))
            .collect();
        Self {
            size,
            twiddles,
            reversed,
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// Forward transform in place.
    pub fn forward(&self, data: &mut [Complex]) {
        self.transform(data, false);
    }

    /// Inverse transform in place, scaled by `1/N` so a round trip is
    /// the identity.
    pub fn inverse(&self, data: &mut [Complex]) {
        self.transform(data, true);
        let scale = 1.0 / self.size as f32;
        for d in data.iter_mut() {
            d.re *= scale;
            d.im *= scale;
        }
    }

    fn transform(&self, data: &mut [Complex], inverse: bool) {
        let n = self.size;
        debug_assert_eq!(data.len(), n);
        for i in 0..n {
            let j = self.reversed[i];
            if j > i {
                data.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let stride = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let mut w = self.twiddles[k * stride];
                    if inverse {
                        w.im = -w.im;
                    }
                    let a = data[start + k];
                    let b = data[start + k + half] * w;
                    data[start + k] = a + b;
                    data[start + k + half] = a - b;
                }
            }
            len *= 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_finds_a_tone() {
        let fft = Fft::new(64);
        let input: Vec<Complex> = (0..64)
            .map(|i| Complex {
                re: (2.0 * PI * 4.0 * i as f32 / 64.0).cos(),
                im: 0.0,
            })
            .collect();
        let mut data = input.clone();
        fft.forward(&mut data);
        // A cosine at bin 4 shows up at bins 4 and 60, each N/2.
        assert!((data[4].re - 32.0).abs() < 1e-3, "{:?}", data[4]);
        assert!((data[60].re - 32.0).abs() < 1e-3);
        assert!(data[5].re.abs() < 1e-3 && data[5].im.abs() < 1e-3);
        fft.inverse(&mut data);
        for (a, b) in data.iter().zip(&input) {
            assert!((a.re - b.re).abs() < 1e-5 && a.im.abs() < 1e-5);
        }
    }

    #[test]
    fn matches_a_direct_dft() {
        let fft = Fft::new(16);
        let mut seed = 3u32;
        let input: Vec<Complex> = (0..16)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                Complex {
                    re: (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5,
                    im: 0.0,
                }
            })
            .collect();
        let mut data = input.clone();
        fft.forward(&mut data);
        for (k, bin) in data.iter().enumerate() {
            let mut sum = Complex::ZERO;
            for (n, x) in input.iter().enumerate() {
                let angle = -2.0 * PI * (k * n) as f32 / 16.0;
                sum = sum
                    + *x * Complex {
                        re: angle.cos(),
                        im: angle.sin(),
                    };
            }
            assert!((sum.re - bin.re).abs() < 1e-4 && (sum.im - bin.im).abs() < 1e-4);
        }
    }
}
