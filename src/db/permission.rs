use documosa_core::permissions::is_allowed;
use crate::error::{AppError, Result};
use crate::models::Identity;

pub(crate) fn require_permission(actor: &Identity, operation: &str) -> Result<()> {
    if !is_allowed(actor.role_mode, operation) {
        return Err(AppError::Forbidden(format!(
            "role {} is not allowed to perform {operation}",
            actor.role_mode.as_str()
        )));
    }
    Ok(())
}
