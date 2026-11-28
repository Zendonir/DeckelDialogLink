//! Arithmetic of DIALOG 11 parameter programming.
//!
//! Operators `+ - x /` (`*` and `:` are accepted as well), brackets, the functions SIN,
//! COS, TAN, ASIN, ACOS, ATAN (degrees), SQR (square), SQRT, INT, ABS, RND, FRC, NEG and
//! the constant PI - as listed in the training documentation (module 11).

use std::sync::OnceLock;

use regex::Regex;

use crate::sim::py_g;

pub const MAX_DEPTH: usize = 40;

/// Expression cannot be evaluated (German message as in the Python version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionError(pub String);

impl std::fmt::Display for ExpressionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Num,
    Param,
    Name,
    Op,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    kind: Kind,
    text: String,
}

const NAMES: [&str; 14] =
    ["ASIN", "ACOS", "ATAN", "SQRT", "SQR", "SIN", "COS", "TAN", "INT", "ABS", "RND", "FRC", "NEG", "PI"];

fn number_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:[0-9]+\.?[0-9]*|\.[0-9]+)").ok()).as_ref()
}

fn param_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:PPT[0-9]+|PP[0-9]+|PT[0-9]+|P[0-9]+)").ok()).as_ref()
}

fn tokenize(text: &str) -> Result<Vec<Token>, ExpressionError> {
    let upper = text.to_uppercase();
    let upper = upper.trim();
    let mut tokens = Vec::new();
    let mut rest = upper;
    while let Some(first) = rest.chars().next() {
        if first.is_whitespace() {
            rest = rest.get(first.len_utf8()..).unwrap_or_default();
            continue;
        }
        let matched: Option<(Kind, usize)> = if let Some(m) = number_re().and_then(|re| re.find(rest)) {
            Some((Kind::Num, m.end()))
        } else if let Some(m) = param_re().and_then(|re| re.find(rest)) {
            Some((Kind::Param, m.end()))
        } else if let Some(name) = NAMES.iter().find(|n| rest.starts_with(**n)) {
            Some((Kind::Name, name.len()))
        } else if "-+*/:X()".contains(first) {
            Some((Kind::Op, first.len_utf8()))
        } else {
            None
        };
        let Some((kind, len)) = matched else {
            return Err(ExpressionError(format!("Unerwartetes Zeichen '{first}' im Ausdruck")));
        };
        tokens.push(Token { kind, text: rest.get(..len).unwrap_or_default().to_string() });
        rest = rest.get(len..).unwrap_or_default();
    }
    Ok(tokens)
}

/// Python `float(text)` for the number syntax of the tokenizer and NC words.
pub fn parse_float(text: &str) -> Result<f64, ExpressionError> {
    let trimmed = text.trim();
    let valid = !trimmed.is_empty()
        && trimmed.trim_start_matches(['+', '-']).chars().all(|c| c.is_ascii_digit() || c == '.')
        && trimmed.trim_start_matches(['+', '-']).chars().any(|c| c.is_ascii_digit());
    match (valid, trimmed.parse::<f64>()) {
        (true, Ok(value)) => Ok(value),
        _ => Err(ExpressionError(format!("could not convert string to float: '{text}'"))),
    }
}

fn function(name: &str, v: f64) -> Option<f64> {
    let value = match name {
        "SIN" if v.is_infinite() => return None,
        "COS" if v.is_infinite() => return None,
        "TAN" if v.is_infinite() => return None,
        "SIN" => v.to_radians().sin(),
        "COS" => v.to_radians().cos(),
        "TAN" => v.to_radians().tan(),
        "ASIN" if v.abs() > 1.0 => return None,
        "ACOS" if v.abs() > 1.0 => return None,
        "ASIN" => v.asin().to_degrees(),
        "ACOS" => v.acos().to_degrees(),
        "ATAN" => v.atan().to_degrees(),
        "SQR" => v * v,
        "SQRT" if v < 0.0 => return None,
        "SQRT" => v.sqrt(),
        "INT" | "FRC" | "RND" if !v.is_finite() => return None,
        "INT" => v.trunc(),
        "ABS" => v.abs(),
        "RND" => {
            if v >= 0.0 {
                (v + 0.5).floor()
            } else {
                -((-v + 0.5).floor())
            }
        }
        "FRC" => v - v.trunc(),
        "NEG" => -v,
        _ => return None,
    };
    Some(value)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    resolve: &'a mut dyn FnMut(&str) -> Result<f64, ExpressionError>,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_op(&self) -> Option<char> {
        self.peek().filter(|t| t.kind == Kind::Op).and_then(|t| t.text.chars().next())
    }

    fn take(&mut self) -> Result<Token, ExpressionError> {
        let token = self.peek().cloned().ok_or_else(|| ExpressionError("Ausdruck unvollständig".into()))?;
        self.pos += 1;
        Ok(token)
    }

    fn expression(&mut self) -> Result<f64, ExpressionError> {
        let mut value = self.term()?;
        while let Some(op @ ('+' | '-')) = self.peek_op() {
            self.take()?;
            let right = self.term()?;
            value = if op == '+' { value + right } else { value - right };
        }
        Ok(value)
    }

    fn term(&mut self) -> Result<f64, ExpressionError> {
        let mut value = self.factor()?;
        while let Some(op @ ('*' | '/' | ':' | 'X')) = self.peek_op() {
            self.take()?;
            let right = self.factor()?;
            if matches!(op, '*' | 'X') {
                value *= right;
            } else {
                if right == 0.0 {
                    return Err(ExpressionError("Division durch Null".into()));
                }
                value /= right;
            }
        }
        Ok(value)
    }

    fn factor(&mut self) -> Result<f64, ExpressionError> {
        self.depth += 1;
        let result = self.factor_inner();
        self.depth -= 1;
        result
    }

    fn factor_inner(&mut self) -> Result<f64, ExpressionError> {
        if self.depth > MAX_DEPTH {
            return Err(ExpressionError("Ausdruck zu tief verschachtelt".into()));
        }
        let token = self.take()?;
        match token.kind {
            Kind::Op if token.text == "+" || token.text == "-" => {
                let value = self.factor()?;
                Ok(if token.text == "-" { -value } else { value })
            }
            Kind::Num => parse_float(&token.text),
            Kind::Param => (self.resolve)(&token.text),
            Kind::Name if token.text == "PI" => Ok(std::f64::consts::PI),
            Kind::Name => {
                let argument = self.factor()?;
                function(&token.text, argument)
                    .ok_or_else(|| ExpressionError(format!("{}({}) ist nicht definiert", token.text, py_g(argument))))
            }
            Kind::Op if token.text == "(" => {
                let value = self.expression()?;
                let close = self.take()?;
                if !(close.kind == Kind::Op && close.text == ")") {
                    return Err(ExpressionError("Schließende Klammer fehlt".into()));
                }
                Ok(value)
            }
            _ => Err(ExpressionError(format!("Unerwartetes Zeichen '{}' im Ausdruck", token.text))),
        }
    }
}

/// Evaluate `text`; parameter references are resolved via `resolve`.
pub fn evaluate(
    text: &str,
    resolve: &mut dyn FnMut(&str) -> Result<f64, ExpressionError>,
) -> Result<f64, ExpressionError> {
    let mut parser = Parser { tokens: tokenize(text)?, pos: 0, resolve, depth: 0 };
    let value = parser.expression()?;
    if let Some(rest) = parser.peek() {
        return Err(ExpressionError(format!("Unerwarteter Rest im Ausdruck: '{}'", rest.text)));
    }
    if !value.is_finite() {
        return Err(ExpressionError("Ergebnis ist keine gültige Zahl".into()));
    }
    Ok(value)
}

fn arithmetic_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:[-+]?(?:[0-9]+\.?[0-9]*|\.[0-9]+)|[-+]?(?:PPT|PP|PT|P)[0-9]+)$").ok()).as_ref()
}

/// True if `text` is a number or a bracketed/parameter expression (not a word chain).
pub fn is_arithmetic(text: &str) -> bool {
    let stripped = text.trim().to_uppercase();
    stripped.starts_with('(') || arithmetic_re().is_some_and(|re| re.is_match(&stripped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(text: &str) -> Result<f64, ExpressionError> {
        let mut resolve = |name: &str| match name {
            "P1" => Ok(2.0),
            _ => Err(ExpressionError(format!("Parameter {name} ist nicht belegt"))),
        };
        evaluate(text, &mut resolve)
    }

    #[test]
    fn evaluates_like_python() {
        assert_eq!(eval("(P1 + 3) x 2").unwrap(), 10.0);
        assert_eq!(eval("10:4").unwrap(), 2.5);
        assert!((eval("SIN 30").unwrap() - 0.5).abs() < 1e-12);
        assert_eq!(eval("SQR(3)").unwrap(), 9.0);
        assert_eq!(eval("RND -2.5").unwrap(), -3.0);
        assert_eq!(eval("-(-4)").unwrap(), 4.0);
        assert_eq!(eval("1/0").unwrap_err().0, "Division durch Null");
        assert_eq!(eval("SQRT(-4)").unwrap_err().0, "SQRT(-4) ist nicht definiert");
        assert_eq!(eval("2 3").unwrap_err().0, "Unerwarteter Rest im Ausdruck: '3'");
        assert_eq!(eval("2 & 3").unwrap_err().0, "Unerwartetes Zeichen '&' im Ausdruck");
        assert_eq!(eval("(2").unwrap_err().0, "Ausdruck unvollständig");
        assert_eq!(eval("P9").unwrap_err().0, "Parameter P9 ist nicht belegt");
        assert!(is_arithmetic(" -P3 ") && is_arithmetic("(1)") && !is_arithmetic("G81 X1"));
    }
}
