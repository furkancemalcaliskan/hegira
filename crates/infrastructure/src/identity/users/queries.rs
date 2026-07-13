pub fn user_select(condition: &str) -> String {
    format!("{} WHERE {condition}", user_select_columns())
}

pub fn user_select_columns() -> &'static str {
    "SELECT id, pid, username, password_hash, created_at, reset_token, reset_sent_at, email_verification_token, email_verification_sent_at, email_verified_at, magic_link_token, magic_link_expires_at FROM users"
}

pub fn user_order_by(sorting: Option<&str>) -> &'static str {
    match sorting.unwrap_or_default().trim().to_lowercase().as_str() {
        "username" | "username asc" => "username ASC",
        "username desc" => "username DESC",
        "created_at asc" | "created asc" => "created_at ASC",
        "verified asc" | "email_verified_at asc" => "email_verified_at ASC NULLS FIRST",
        "verified desc" | "email_verified_at desc" => "email_verified_at DESC NULLS LAST",
        _ => "created_at DESC",
    }
}
