use crate::error::RototoError;

pub(super) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub(super) fn db_err(err: rusqlite::Error) -> RototoError {
    RototoError::new(format!("console database error: {err}"))
}
