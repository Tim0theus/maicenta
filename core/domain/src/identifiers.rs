use std::fmt;

macro_rules! identifier {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates an identifier after validating its serialized form.
            ///
            /// # Errors
            ///
            /// Returns [`DomainError::InvalidIdentifier`] when the value is
            /// empty or contains non-portable characters.
            pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
                let value = value.into();
                let valid = !value.is_empty()
                    && value.len() <= 128
                    && value.bytes().all(|character| {
                        character.is_ascii_alphanumeric()
                            || matches!(character, b'_' | b'-' | b'.' | b'@')
                    });

                if valid {
                    Ok(Self(value))
                } else {
                    Err(DomainError::InvalidIdentifier)
                }
            }

            /// Returns the identifier as a borrowed string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

identifier!(
    AccountId,
    "Stable identifier for a configured local account."
);
identifier!(
    MailboxId,
    "Stable identifier for a mailbox within an account."
);
identifier!(MessageId, "Stable local identifier for a message.");
identifier!(
    AttachmentId,
    "Stable local identifier for a message attachment."
);
identifier!(
    WorkspaceItemId,
    "Stable identifier for a calendar event, task, or contact."
);

/// Errors raised while validating domain values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainError {
    InvalidIdentifier,
    InvalidEmailAddress,
    EmptyDisplayName,
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier => formatter.write_str("invalid identifier"),
            Self::InvalidEmailAddress => formatter.write_str("invalid email address"),
            Self::EmptyDisplayName => formatter.write_str("display name must not be empty"),
        }
    }
}

impl std::error::Error for DomainError {}

#[cfg(test)]
mod tests {
    use super::{AccountId, AttachmentId, DomainError, MessageId};

    #[test]
    fn accepts_portable_identifiers() {
        let account = AccountId::parse("personal_mail-1").expect("valid identifier");
        assert_eq!(account.as_str(), "personal_mail-1");
        let attachment = AttachmentId::parse("attachment.a1").expect("valid identifier");
        assert_eq!(attachment.as_str(), "attachment.a1");
    }

    #[test]
    fn accepts_provider_message_identifiers() {
        let message = MessageId::parse("account@example.org.123").expect("valid identifier");
        assert_eq!(message.as_str(), "account@example.org.123");
    }

    #[test]
    fn rejects_identifiers_with_path_characters() {
        assert_eq!(
            AccountId::parse("../profile"),
            Err(DomainError::InvalidIdentifier)
        );
    }
}
