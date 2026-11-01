//! The launcher's quick answers: `2*(3+4)`, `5 km to mi`, `20 c in f`.

/// The answer to a query that is a sum or a unit conversion, as text; None if it is neither.
pub fn answer(q: &str) -> Option<String> {
    convert(q).or_else(|| calc(q))
}

fn calc(q: &str) -> Option<String> {
    // no row for a bare number or a bare word like `e` or `pi`: those are app searches
    if !q.chars().any(|c| "+-*/^%(".contains(c)) {
        return None;
    }
    let mut p = Parser { s: q.as_bytes(), i: 0 };
    let v = p.expr()?;
    p.ws();
    (p.i == p.s.len() && v.is_finite() && q.trim().parse::<f64>().is_err()).then(|| fmt(v))
}

fn fmt(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{v}");
    }
    let s = format!("{v:.8}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        let hit = self.s.get(self.i) == Some(&c);
        self.i += hit as usize;
        hit
    }
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            if self.eat(b'+') {
                v += self.term()?;
            } else if self.eat(b'-') {
                v -= self.term()?;
            } else {
                return Some(v);
            }
        }
    }
    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            if self.eat(b'*') || self.eat(b'x') {
                v *= self.power()?;
            } else if self.eat(b'/') {
                v /= self.power()?;
            } else if self.eat(b'%') {
                v %= self.power()?;
            } else {
                return Some(v);
            }
        }
    }
    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        Some(if self.eat(b'^') { base.powf(self.power()?) } else { base }) // right to left: 2^3^2 = 2^9
    }
    fn unary(&mut self) -> Option<f64> {
        if self.eat(b'-') {
            return Some(-self.unary()?);
        }
        self.atom()
    }
    fn atom(&mut self) -> Option<f64> {
        self.ws();
        if self.eat(b'(') {
            let v = self.expr()?;
            return self.eat(b')').then_some(v);
        }
        let start = self.i;
        if self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
            while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
                self.i += 1;
            }
            return std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok();
        }
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        let name = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        match name {
            "pi" => return Some(std::f64::consts::PI),
            "e" => return Some(std::f64::consts::E),
            _ => {}
        }
        if !self.eat(b'(') {
            return None;
        }
        let v = self.expr()?;
        if !self.eat(b')') {
            return None;
        }
        Some(match name {
            "sqrt" => v.sqrt(),
            "sin" => v.sin(),
            "cos" => v.cos(),
            "tan" => v.tan(),
            "ln" => v.ln(),
            "log" => v.log10(),
            "abs" => v.abs(),
            "round" => v.round(),
            _ => return None,
        })
    }
}

/// (names, kind, size in the kind's base unit); temperature is handled apart.
const UNITS: &[(&[&str], &str, f64)] = &[
    (&["mm"], "length", 0.001), (&["cm"], "length", 0.01), (&["m", "meter", "meters"], "length", 1.0), (&["km"], "length", 1000.0),
    (&["in", "inch", "inches"], "length", 0.0254), (&["ft", "foot", "feet"], "length", 0.3048), (&["yd", "yard", "yards"], "length", 0.9144), (&["mi", "mile", "miles"], "length", 1609.344),
    (&["mg"], "mass", 0.000001), (&["g", "gram", "grams"], "mass", 0.001), (&["kg"], "mass", 1.0), (&["oz", "ounce", "ounces"], "mass", 0.028349523), (&["lb", "lbs", "pound", "pounds"], "mass", 0.45359237),
    (&["ml"], "volume", 0.001), (&["l", "liter", "liters", "litre", "litres"], "volume", 1.0), (&["tsp"], "volume", 0.00492892), (&["tbsp"], "volume", 0.0147868), (&["cup", "cups"], "volume", 0.236588), (&["gal", "gallon", "gallons"], "volume", 3.78541),
    (&["s", "sec", "second", "seconds"], "time", 1.0), (&["min", "minute", "minutes"], "time", 60.0), (&["h", "hr", "hour", "hours"], "time", 3600.0), (&["d", "day", "days"], "time", 86400.0), (&["week", "weeks"], "time", 604800.0),
    (&["b", "byte", "bytes"], "data", 1.0), (&["kb"], "data", 1e3), (&["mb"], "data", 1e6), (&["gb"], "data", 1e9), (&["tb"], "data", 1e12), (&["kib"], "data", 1024.0), (&["mib"], "data", 1048576.0), (&["gib"], "data", 1073741824.0),
];

fn unit(name: &str) -> Option<(&'static str, f64)> {
    let n = name.to_lowercase();
    UNITS.iter().find(|u| u.0.contains(&n.as_str())).map(|u| (u.1, u.2))
}

fn temp(name: &str) -> Option<char> {
    match name.to_lowercase().trim_start_matches('°') {
        "c" | "celsius" => Some('c'),
        "f" | "fahrenheit" => Some('f'),
        "k" | "kelvin" => Some('k'),
        _ => None,
    }
}

/// "<number> <unit> to|in <unit>"
fn convert(q: &str) -> Option<String> {
    let w: Vec<&str> = q.split_whitespace().collect();
    let (num, rest) = match w.as_slice() {
        [n, ..] if n.parse::<f64>().is_ok() => (n.parse::<f64>().ok()?, &w[1..]),
        [n, ..] => {
            // "5km to mi": the unit is stuck to the number
            let at = n.find(|c: char| c.is_alphabetic())?;
            let v = n[..at].parse::<f64>().ok()?;
            return convert(&format!("{v} {}{}", &n[at..], w[1..].iter().map(|x| format!(" {x}")).collect::<String>()));
        }
        [] => return None,
    };
    let [from, sep, to] = rest else { return None };
    if !["to", "in", "->", "as"].contains(sep) {
        return None;
    }
    if let (Some(a), Some(b)) = (temp(from), temp(to)) {
        let c = match a { 'c' => num, 'f' => (num - 32.0) / 1.8, _ => num - 273.15 };
        let out = match b { 'c' => c, 'f' => c * 1.8 + 32.0, _ => c + 273.15 };
        return Some(format!("{} {}", fmt(out), b.to_ascii_uppercase()));
    }
    let ((ka, fa), (kb, fb)) = (unit(from)?, unit(to)?);
    (ka == kb).then(|| format!("{} {to}", fmt(num * fa / fb)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums() {
        assert_eq!(answer("2*(3+4)").unwrap(), "14");
        assert_eq!(answer("2^3^2").unwrap(), "512");
        assert_eq!(answer("10/4").unwrap(), "2.5");
        assert_eq!(answer("-3 + 5").unwrap(), "2");
        assert_eq!(answer("sqrt(16)+1").unwrap(), "5");
        assert_eq!(answer("1/3").unwrap(), "0.33333333");
    }

    #[test]
    fn not_a_sum() {
        assert!(answer("firefox").is_none());
        assert!(answer("42").is_none());
        assert!(answer("2+").is_none());
        assert!(answer("(1+2").is_none());
        assert!(answer("e").is_none() && answer("pi").is_none(), "bare words are app searches");
        assert_eq!(answer("pi*2").unwrap(), "6.28318531");
    }

    #[test]
    fn units() {
        assert_eq!(answer("5 km to mi").unwrap(), "3.10685596 mi");
        assert_eq!(answer("5km in m").unwrap(), "5000 m");
        assert_eq!(answer("100 c to f").unwrap(), "212 F");
        assert_eq!(answer("1 gib to mb").unwrap(), "1073.741824 mb");
        assert!(answer("5 km to kg").is_none(), "different kinds");
        assert!(answer("5 foo to bar").is_none());
    }
}
