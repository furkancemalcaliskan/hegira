use sqlx::{Row, postgres::PgRow};

use domain::identity::users::User;

pub fn map_user(row: PgRow) -> User {
    User {
        id: row.get("id"),
        pid: row.get("pid"),
        username: row.get("username"),
        password_hash: row.get("password_hash"),
        created_at: row.get("created_at"),
        reset_token: row.get("reset_token"),
        reset_sent_at: row.get("reset_sent_at"),
        email_verification_token: row.get("email_verification_token"),
        email_verification_sent_at: row.get("email_verification_sent_at"),
        email_verified_at: row.get("email_verified_at"),
        magic_link_token: row.get("magic_link_token"),
        magic_link_expires_at: row.get("magic_link_expires_at"),
    }
}
