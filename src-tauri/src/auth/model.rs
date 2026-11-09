use serde::{Deserialize, Serialize};

/// Password hash is deliberately never included here — this type is what gets
/// serialized back to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub display_name: String,
    pub created_at: String,
    /// False for a profile made without a password: it opens without one,
    /// and the lock screen offers a plain "Continue".
    #[serde(default)]
    pub has_password: bool,
}
