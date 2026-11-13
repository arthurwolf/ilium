//! Access policy for the local automation boundary, shared by command and HTTP adapters.
use crate::Operation;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessLevel {
    Read,
    Act,
    Admin,
}

impl AccessLevel {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "act" => Some(Self::Act),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Act => "act",
            Self::Admin => "admin",
        }
    }
    pub fn authorize(self, operation: &Operation) -> Result<(), &'static str> {
        let required = if matches!(
            operation,
            Operation::Terminate { .. } | Operation::KillSession
        ) {
            Self::Admin
        } else {
            Self::Act
        };
        if self >= required {
            Ok(())
        } else {
            Err("operation requires a higher access level")
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tokens {
    pub read: String,
    pub act: String,
    pub admin: String,
}
impl Tokens {
    pub fn authenticate(&self, token: Option<&str>) -> Result<AccessLevel, &'static str> {
        let Some(token) = token else {
            return Ok(AccessLevel::Read);
        };
        if token.is_empty() {
            return Err("invalid access token");
        }
        for level in [AccessLevel::Read, AccessLevel::Act, AccessLevel::Admin] {
            if self.token(level) == token {
                return Ok(level);
            }
        }
        Err("invalid access token")
    }
    pub fn token(&self, level: AccessLevel) -> &str {
        match level {
            AccessLevel::Read => &self.read,
            AccessLevel::Act => &self.act,
            AccessLevel::Admin => &self.admin,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outside_callers_default_to_read_and_tokens_grant_only_their_own_level() {
        let tokens = Tokens {
            read: "fixture-read".into(),
            act: "fixture-act".into(),
            admin: "fixture-admin".into(),
        };
        assert_eq!(tokens.authenticate(None), Ok(AccessLevel::Read));
        assert_eq!(
            tokens.authenticate(Some("fixture-act")),
            Ok(AccessLevel::Act)
        );
        assert_eq!(
            tokens.authenticate(Some("fixture-admin")),
            Ok(AccessLevel::Admin)
        );
        assert!(tokens.authenticate(Some("wrong")).is_err());
        assert!(tokens.authenticate(Some("")).is_err());
    }
    #[test]
    fn act_can_address_explicit_panes_but_termination_requires_admin() {
        let rename = Operation::Rename {
            node: 7,
            title: "fixture".into(),
        };
        assert!(AccessLevel::Read.authorize(&rename).is_err());
        assert!(AccessLevel::Act.authorize(&rename).is_ok());
        for operation in [Operation::Terminate { pane: 7 }, Operation::KillSession] {
            assert!(AccessLevel::Act.authorize(&operation).is_err());
            assert!(AccessLevel::Read.authorize(&operation).is_err());
            assert!(AccessLevel::Admin.authorize(&operation).is_ok());
        }
    }
}
