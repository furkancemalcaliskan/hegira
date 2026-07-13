use sqlx::{Postgres, Transaction};

pub type DbTransaction<'a> = Transaction<'a, Postgres>;
