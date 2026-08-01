pub const DEFAULT_ADMIN_USERNAME: &str = "admin@example.com";
pub const DEFAULT_ADMIN_ROLE_NAME: &str = "admin";

pub fn is_protected_admin_username(username: &str) -> bool {
    let username = username.trim();
    username.eq_ignore_ascii_case(DEFAULT_ADMIN_USERNAME) || username.eq_ignore_ascii_case("admin")
}

pub fn is_protected_admin_role(role_name: &str) -> bool {
    role_name
        .trim()
        .eq_ignore_ascii_case(DEFAULT_ADMIN_ROLE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_identity_names_remain_case_insensitive_and_trimmed() {
        assert!(is_protected_admin_username(" admin@example.com "));
        assert!(is_protected_admin_username("ADMIN"));
        assert!(!is_protected_admin_username("administrator@example.com"));

        assert!(is_protected_admin_role(" ADMIN "));
        assert!(!is_protected_admin_role("administrator"));
    }
}
