use crate::identity::RoleMode;

/// Operations available to Writer role.
pub const WRITER_OPS: &[&str] = &[
    "create_document",
    "insert_lines",
    "replace_lines",
    "delete_lines",
    "lock_lines",
    "heartbeat_locks",
    "release_locks",
    "create_comment",
    "reply_comment",
    "update_comment",
    "resolve_comment",
    "create_suggestion",
    "accept_suggestion",
    "reject_suggestion",
    "view_history",
    "history_diff",
    "set_audit_note",
    "export_document",
];

/// Operations available to Reviewer role.
pub const REVIEWER_OPS: &[&str] = &[
    "create_document",
    "create_comment",
    "reply_comment",
    "update_comment",
    "resolve_comment",
    "create_suggestion",
    "view_history",
    "history_diff",
    "set_audit_note",
    "export_document",
];

/// Check whether a role is allowed to perform an operation.
pub fn is_allowed(role: RoleMode, operation: &str) -> bool {
    match role {
        RoleMode::Writer => WRITER_OPS.contains(&operation),
        RoleMode::Reviewer => REVIEWER_OPS.contains(&operation),
    }
}
