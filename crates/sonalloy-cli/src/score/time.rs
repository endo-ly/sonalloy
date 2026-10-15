use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive, Zero};
use serde_json::Value;

use super::{Error, Result};

pub(super) type Quarter = BigRational;

pub(super) fn q(n: i64, d: i64) -> Quarter {
    Quarter::new(BigInt::from(n), BigInt::from(d))
}

pub(super) fn rational(value: &Value, path: &str) -> Result<Quarter> {
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => {
            return Err(Error::new(
                path,
                "expected an integer, decimal, or quoted fraction",
            ));
        }
    };
    let parse = |s: &str| {
        s.parse::<BigInt>()
            .map_err(|_| Error::new(path, "invalid rational number"))
    };
    if text.len() > 128 {
        return Err(Error::new(path, "rational number exceeds 128 characters"));
    }
    if let Some((n, d)) = text.split_once('/') {
        let d = parse(d)?;
        if d <= BigInt::zero() {
            return Err(Error::new(path, "expected a positive denominator"));
        }
        return Ok(Quarter::new(parse(n)?, d));
    }
    let (mantissa, exponent) = text
        .split_once(['e', 'E'])
        .map_or((text.as_str(), 0), |(m, e)| {
            (m, e.parse::<i32>().unwrap_or(i32::MAX))
        });
    if exponent.abs_diff(0) > 100 {
        return Err(Error::new(path, "decimal exponent exceeds 100"));
    }
    let decimals = mantissa.split_once('.').map_or(0, |(_, d)| d.len());
    let n = parse(&mantissa.replace('.', ""))?;
    let power = i32::try_from(decimals).unwrap_or(128) - exponent;
    let ten = BigInt::from(10);
    Ok(if power >= 0 {
        Quarter::new(n, ten.pow(power.unsigned_abs()))
    } else {
        Quarter::from_integer(n * ten.pow(power.unsigned_abs()))
    })
}

pub(super) fn ppq<'a>(times: impl IntoIterator<Item = &'a Quarter>) -> Result<u16> {
    let mut resolution = 480_u64;
    for time in times {
        let denominator = time
            .denom()
            .to_u64()
            .ok_or_else(|| Error::new("form", "time cannot be represented within 32767 PPQ"))?;
        let mut a = resolution;
        let mut b = denominator;
        while b != 0 {
            (a, b) = (b, a % b);
        }
        resolution = (resolution / a)
            .checked_mul(denominator)
            .filter(|n| *n <= 32767)
            .ok_or_else(|| Error::new("form", "exact timing requires more than 32767 PPQ"))?;
    }
    u16::try_from(resolution).map_err(|_| Error::new("form", "invalid PPQ"))
}

pub(super) fn tick(time: &Quarter, ppq: u16) -> Result<u64> {
    let value = time * Quarter::from_integer(ppq.into());
    if !value.denom().is_one() {
        return Err(Error::new("form", "time is not an integer tick"));
    }
    value
        .numer()
        .to_u64()
        .ok_or_else(|| Error::new("form", "time is outside the supported tick range"))
}
