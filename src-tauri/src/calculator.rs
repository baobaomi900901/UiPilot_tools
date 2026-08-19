const MAX_EXPRESSION_LENGTH: usize = 400;

pub(crate) fn evaluate(source: &str) -> Option<String> {
    if source.len() > MAX_EXPRESSION_LENGTH {
        return None;
    }
    let mut parser = Parser {
        source: source.as_bytes(),
        index: 0,
        binary_operations: 0,
    };
    let value = parser.expression()?;
    parser.skip_spaces();
    if parser.index != parser.source.len() || parser.binary_operations == 0 || !value.is_finite() {
        return None;
    }
    Some(if value == 0.0 {
        "0".into()
    } else {
        value.to_string()
    })
}

struct Parser<'a> {
    source: &'a [u8],
    index: usize,
    binary_operations: usize,
}

impl Parser<'_> {
    fn expression(&mut self) -> Option<f64> {
        let mut value = self.term()?;
        loop {
            self.skip_spaces();
            let operator = self.current();
            if operator != Some(b'+') && operator != Some(b'-') {
                return Some(value);
            }
            self.index += 1;
            let right = self.term()?;
            self.binary_operations += 1;
            value = if operator == Some(b'+') {
                value + right
            } else {
                value - right
            };
            if !value.is_finite() {
                return None;
            }
        }
    }

    fn term(&mut self) -> Option<f64> {
        let mut value = self.factor()?;
        loop {
            self.skip_spaces();
            let operator = self.current();
            if operator != Some(b'*') && operator != Some(b'/') {
                return Some(value);
            }
            self.index += 1;
            let right = self.factor()?;
            if operator == Some(b'/') && right == 0.0 {
                return None;
            }
            self.binary_operations += 1;
            value = if operator == Some(b'*') {
                value * right
            } else {
                value / right
            };
            if !value.is_finite() {
                return None;
            }
        }
    }

    fn factor(&mut self) -> Option<f64> {
        self.skip_spaces();
        let mut sign = 1.0;
        while matches!(self.current(), Some(b'+') | Some(b'-')) {
            if self.current() == Some(b'-') {
                sign = -sign;
            }
            self.index += 1;
            self.skip_spaces();
        }
        if self.current() == Some(b'(') {
            self.index += 1;
            let value = self.expression()?;
            self.skip_spaces();
            if self.current() != Some(b')') {
                return None;
            }
            self.index += 1;
            return Some(sign * value);
        }
        Some(sign * self.number()?)
    }

    fn number(&mut self) -> Option<f64> {
        let start = self.index;
        while self.current().is_some_and(|byte| byte.is_ascii_digit()) {
            self.index += 1;
        }
        if self.current() == Some(b'.') {
            self.index += 1;
            while self.current().is_some_and(|byte| byte.is_ascii_digit()) {
                self.index += 1;
            }
        }
        let literal = std::str::from_utf8(&self.source[start..self.index]).ok()?;
        if literal.is_empty() || literal == "." {
            return None;
        }
        literal
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
    }

    fn skip_spaces(&mut self) {
        while self.current() == Some(b' ') {
            self.index += 1;
        }
    }

    fn current(&self) -> Option<u8> {
        self.source.get(self.index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate;

    #[test]
    fn recognizes_only_complete_four_function_expressions() {
        for (input, expected) in [
            ("1+1", Some("2")),
            ("2*(3+4)", Some("14")),
            ("-2 + 5", Some("3")),
            (".5*8", Some("4")),
            ("2026-08-16", Some("2002")),
            ("123", None),
            ("-123", None),
            ("1+", None),
            ("2*(3", None),
            ("1/0", None),
            ("2^3", None),
        ] {
            assert_eq!(evaluate(input).as_deref(), expected, "input: {input}");
        }
    }
}
