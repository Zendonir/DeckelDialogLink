//! Error types. All user-visible messages are German, matching the Python version.

use std::fmt;

/// One invalid field of a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

impl FieldError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self { field: field.into(), message: message.into() }
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

/// A profile could not be accepted; lists every offending field.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct ProfileValidationError {
    pub errors: Vec<FieldError>,
    pub source_name: String,
}

impl fmt::Display for ProfileValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let joined = self.errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ");
        let text = format!("Ungültiges Profil {}: {}", self.source_name, joined);
        f.write_str(text.trim())
    }
}

/// Error description for the operator (title, probable causes, technical details).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserFacingError {
    pub title: String,
    pub causes: Vec<String>,
    pub technical_details: String,
    pub operation: String,
}

impl UserFacingError {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), ..Self::default() }
    }

    pub fn as_text(&self) -> String {
        let mut lines = vec![self.title.clone()];
        if !self.causes.is_empty() {
            lines.push(String::new());
            lines.push("Mögliche Ursachen:".into());
            lines.extend(self.causes.iter().map(|c| format!("- {c}")));
        }
        if !self.technical_details.is_empty() {
            lines.push(String::new());
            lines.push("Technische Details:".into());
            lines.push(self.technical_details.clone());
        }
        lines.join("\n")
    }
}

impl fmt::Display for UserFacingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.title)
    }
}

/// Data cannot be prepared according to the profile.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TransformError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages() {
        let err = ProfileValidationError {
            errors: vec![FieldError::new("serial.parity", "x"), FieldError::new("id", "y")],
            source_name: String::new(),
        };
        assert_eq!(err.to_string(), "Ungültiges Profil : serial.parity: x; id: y");
        let user = UserFacingError {
            title: "T".into(),
            causes: vec!["a".into()],
            technical_details: "d".into(),
            operation: String::new(),
        };
        assert_eq!(user.as_text(), "T\n\nMögliche Ursachen:\n- a\n\nTechnische Details:\nd");
        assert_eq!(TransformError("z".into()).to_string(), "z");
    }
}
