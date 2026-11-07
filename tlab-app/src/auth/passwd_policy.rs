/// Application password rules, independent of the hashing algorithm.
pub trait PasswordPolicy: Send + Sync {
    fn validate(&self, password: &str) -> Result<(), PasswordPolicyViolation>;
}

#[derive(Debug)]
pub struct PasswordPolicyViolation {
    pub code: &'static str,
    pub message: &'static str,
}

pub struct NonEmptyPasswordPolicy;

impl PasswordPolicy for NonEmptyPasswordPolicy {
    fn validate(&self, password: &str) -> Result<(), PasswordPolicyViolation> {
        if password.is_empty() {
            return Err(PasswordPolicyViolation {
                code: "PASSWORD_REQUIRED",
                message: "password must not be empty",
            });
        }
        Ok(())
    }
}
