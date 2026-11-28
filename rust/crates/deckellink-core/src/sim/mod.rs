//! Offline graphic simulation of DIALOG 11 NC programs (training and checking aid).
//!
//! Port of the Python package `deckellink.dialog11sim` with identical behaviour
//! (checked by `tools/rust_parity.py`). It never talks to a machine: it interprets
//! program text and produces tool paths for display. The supported syntax was derived
//! from the DECKEL training documentation (modules 3-13); details the documentation
//! leaves open are simplified and reported as notes during the simulation.

pub mod cycles;
pub mod expressions;
pub mod geometry;
pub mod interpreter;
pub mod menus;
pub mod model;
pub mod playback;
pub mod reference;
pub mod syntax;

pub use interpreter::simulate;

/// Python's `format(value, "g")` (6 significant digits, trailing zeros removed).
pub fn py_g(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let sci = format!("{value:.5e}");
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exponent.parse().unwrap_or(0);
    let strip = |text: &str| -> String {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            text.to_string()
        }
    };
    if (-4..6).contains(&exp) {
        let decimals = usize::try_from(5 - exp).unwrap_or(0);
        strip(&format!("{value:.decimals$}"))
    } else {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", strip(mantissa), exp.abs())
    }
}

/// Python `int(x)` for a float (truncation; saturating for absurd values).
#[allow(clippy::cast_possible_truncation)]
pub fn py_int(value: f64) -> i64 {
    value.trunc() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g_format_like_python() {
        for (value, text) in [
            (0.0, "0"),
            (-0.0, "-0"),
            (5.0, "5"),
            (2.5, "2.5"),
            (0.1, "0.1"),
            (1.0 / 3.0, "0.333333"),
            (123_456.0, "123456"),
            (1_234_567.0, "1.23457e+06"),
            (0.000_12, "0.00012"),
            (0.000_012, "1.2e-05"),
            (-40.0, "-40"),
            (99.999_99, "100"),
            (1e100, "1e+100"),
        ] {
            assert_eq!(py_g(value), text, "{value}");
        }
    }
}
