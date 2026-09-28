//! Numbers as Python prints them, so `str(x)` and a template read the
//! same here as in the Python a reader knows; and exact summation.

/// Python's `repr(float)`: the shortest digits that read back as the same
/// float, positional between 1e-4 and 1e16 and scientific outside, with
/// `.0` on a whole number.
pub fn float_repr(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    // Rust's `{:e}` gives the shortest round-trip digits: "1.2345e-7".
    let s = format!("{:e}", x);
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let sign = if neg { "-" } else { "" };
    if (-4..16).contains(&exp) {
        let point = exp + 1;
        let body = if point <= 0 {
            format!("0.{}{}", "0".repeat((-point) as usize), digits)
        } else if point as usize >= digits.len() {
            format!("{}{}.0", digits, "0".repeat(point as usize - digits.len()))
        } else {
            format!(
                "{}.{}",
                &digits[..point as usize],
                &digits[point as usize..]
            )
        };
        format!("{sign}{body}")
    } else {
        let m = if digits.len() == 1 {
            digits.clone()
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        let e = if exp < 0 {
            format!("-{:02}", -exp)
        } else {
            format!("+{:02}", exp)
        };
        format!("{sign}{m}e{e}")
    }
}

/// Round half to even at `n` decimal places, from the float's exact
/// value, as Python's `round(x, n)` does.
pub fn round_to(x: f64, n: u32) -> f64 {
    let s = format!("{:.*}", n as usize, x);
    s.parse().unwrap_or(x)
}

/// The exact sum of floats, correctly rounded once: Python's `math.fsum`
/// (Shewchuk's algorithm), so a sum does not depend on the order of the
/// terms or on how many there are.
pub fn fsum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut partials: Vec<f64> = Vec::new();
    for mut x in values {
        let mut i = 0;
        for j in 0..partials.len() {
            let mut y = partials[j];
            if x.abs() < y.abs() {
                std::mem::swap(&mut x, &mut y);
            }
            let hi = x + y;
            let lo = y - (hi - x);
            if lo != 0.0 {
                partials[i] = lo;
                i += 1;
            }
            x = hi;
        }
        partials.truncate(i);
        partials.push(x);
    }
    let mut n = partials.len();
    if n == 0 {
        return 0.0;
    }
    n -= 1;
    let mut hi = partials[n];
    let mut lo = 0.0;
    while n > 0 {
        n -= 1;
        let x = hi;
        let y = partials[n];
        hi = x + y;
        let yr = hi - x;
        lo = y - yr;
        if lo != 0.0 {
            break;
        }
    }
    // Round half to even across the last two partials, as fsum does.
    if n > 0 && ((lo < 0.0 && partials[n - 1] < 0.0) || (lo > 0.0 && partials[n - 1] > 0.0)) {
        let y = lo * 2.0;
        let x = hi + y;
        let yr = x - hi;
        if y == yr {
            hi = x;
        }
    }
    hi
}

/// Python's format mini-language for one number, the part a reader uses:
/// `[sign][,][.precision][type]` with type `f`, `e`, `g`, `%` or `d`.
pub fn format_number(x: f64, is_int: bool, spec: &str) -> Result<String, String> {
    let mut rest = spec;
    let mut plus = false;
    if let Some(r) = rest.strip_prefix('+') {
        plus = true;
        rest = r;
    }
    let mut comma = false;
    if let Some(r) = rest.strip_prefix(',') {
        comma = true;
        rest = r;
    }
    let mut precision: Option<usize> = None;
    if let Some(r) = rest.strip_prefix('.') {
        let digits: String = r.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return Err(format!("the format `{spec}` has a `.` with no precision"));
        }
        precision = Some(
            digits
                .parse()
                .map_err(|_| format!("the precision in `{spec}` is too large"))?,
        );
        rest = &r[digits.len()..];
    }
    let ty = rest.chars().next();
    if rest.chars().count() > 1 {
        return Err(format!("`{spec}` is not a format this language reads"));
    }
    let body = match ty {
        None => {
            if is_int {
                format!("{}", x as i64)
            } else if let Some(p) = precision {
                general(x, p.max(1))
            } else {
                float_repr(x)
            }
        }
        Some('f') => format!("{:.*}", precision.unwrap_or(6), x),
        Some('%') => format!("{:.*}%", precision.unwrap_or(6), x * 100.0),
        Some('e') => sci(x, precision.unwrap_or(6)),
        Some('g') => general(x, precision.unwrap_or(6).max(1)),
        Some('d') => {
            if !is_int {
                return Err("`d` formats a whole number".into());
            }
            format!("{}", x as i64)
        }
        Some(c) => return Err(format!("`{c}` is not a format type this language reads")),
    };
    let body = if comma { group_thousands(&body) } else { body };
    Ok(if plus && !body.starts_with('-') {
        format!("+{body}")
    } else {
        body
    })
}

fn sci(x: f64, p: usize) -> String {
    let s = format!("{:.*e}", p, x);
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    let e: i32 = e.parse().unwrap_or(0);
    format!("{m}e{}{:02}", if e < 0 { "-" } else { "+" }, e.abs())
}

/// Python's `g`: `p` significant digits, scientific when the exponent is
/// below -4 or at least `p`, trailing zeros dropped.
fn general(x: f64, p: usize) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let s = format!("{:.*e}", p - 1, x);
    let e: i32 = s
        .split_once('e')
        .and_then(|(_, e)| e.parse().ok())
        .unwrap_or(0);
    if e < -4 || e >= p as i32 {
        let t = sci(x, p - 1);
        let (m, rest) = t.split_once('e').unwrap_or((&t, ""));
        let m = if m.contains('.') {
            m.trim_end_matches('0').trim_end_matches('.')
        } else {
            m
        };
        format!("{m}e{rest}")
    } else {
        let decimals = (p as i32 - 1 - e).max(0) as usize;
        let t = format!("{:.*}", decimals, x);
        if t.contains('.') {
            t.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            t
        }
    }
}

fn group_thousands(s: &str) -> String {
    let (sign, rest) = if let Some(r) = s.strip_prefix('-') {
        ("-", r)
    } else {
        ("", s)
    };
    let (int, frac) = match rest.find(['.', 'e', '%']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{sign}{out}{frac}")
}
