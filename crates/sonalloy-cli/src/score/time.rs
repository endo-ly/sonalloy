use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive, Zero};
use serde_json::Value;

use super::{Error, Result};

pub(super) type Quarter = BigRational;

#[derive(Clone)]
pub(super) struct Swing {
    pair: Quarter,
    ratio: Quarter,
}

impl Swing {
    pub(super) fn parse(value: &Value, path: &str) -> Result<Self> {
        super::fields(value, &["subdivision", "ratio"], path)?;
        let subdivision = super::integer(super::required(value, "subdivision", path)?, path)?;
        if ![8, 16].contains(&subdivision) {
            return Err(Error::new(path, "swing subdivision must be 8 or 16"));
        }
        let ratio = rational(super::required(value, "ratio", path)?, path)?;
        if ratio <= q(0, 1) || ratio >= q(1, 1) {
            return Err(Error::new(
                path,
                "swing ratio must be greater than zero and less than one",
            ));
        }
        Ok(Self {
            pair: q(8, subdivision),
            ratio,
        })
    }

    pub(super) fn warp(&self, at: &Quarter, length: &Quarter) -> Quarter {
        let complete =
            Quarter::from_integer((length / &self.pair).floor().to_integer()) * &self.pair;
        if at >= &complete {
            return at.clone();
        }
        let start = Quarter::from_integer((at / &self.pair).floor().to_integer()) * &self.pair;
        let phase = (at - &start) / &self.pair;
        let position = if phase <= q(1, 2) {
            phase * q(2, 1) * &self.ratio
        } else {
            &self.ratio + (phase - q(1, 2)) * q(2, 1) * (q(1, 1) - &self.ratio)
        };
        start + position * &self.pair
    }
}

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
