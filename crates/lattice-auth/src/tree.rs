use std::collections::HashMap;

/// Requirements for accessing a type or field.
#[derive(Debug, Clone, Default)]
pub struct AuthRequirement {
    /// Whether authentication is required (from `@requiresAuthentication`).
    pub requires_auth: bool,
    /// Required scopes (from `@requiresScopes`). Outer vec is OR, inner is AND.
    /// e.g. `[["read:users", "read:org"], ["admin"]]` means
    /// (read:users AND read:org) OR admin.
    pub scopes: Vec<Vec<String>>,
}

impl AuthRequirement {
    /// Whether this requirement demands anything.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        !self.requires_auth && self.scopes.is_empty()
    }
}

/// Tree mapping type/field paths to their auth requirements.
///
/// Built once on schema load. Keyed by `TypeName` for type-level or
/// `TypeName.fieldName` for field-level requirements.
#[derive(Debug, Clone, Default)]
pub struct AuthTree {
    requirements: HashMap<String, AuthRequirement>,
}

impl AuthTree {
    /// Create an empty auth tree.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a requirement for a type or field path.
    pub fn insert(&mut self, path: String, req: AuthRequirement) {
        self.requirements.insert(path, req);
    }

    /// Look up the requirement for a specific field, falling back to the
    /// type-level requirement.
    #[must_use]
    pub fn requirement(&self, type_name: &str, field_name: &str) -> Option<&AuthRequirement> {
        let field_key = format!("{type_name}.{field_name}");
        self.requirements
            .get(&field_key)
            .or_else(|| self.requirements.get(type_name))
    }

    /// Whether this tree has any requirements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.requirements.is_empty()
    }

    /// Check whether the given claims satisfy a requirement.
    ///
    /// `user_scopes` is the list of scopes the user has (e.g. from JWT "scope" claim).
    #[must_use]
    pub fn check_access(req: &AuthRequirement, is_authenticated: bool, user_scopes: &[String]) -> bool {
        if req.requires_auth && !is_authenticated {
            return false;
        }

        if req.scopes.is_empty() {
            return true;
        }

        // OR across outer groups, AND within each group.
        req.scopes
            .iter()
            .any(|group| group.iter().all(|scope| user_scopes.contains(scope)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_requirement_is_empty() {
        let req = AuthRequirement::default();
        assert!(req.is_empty());
    }

    #[test]
    fn field_level_overrides_type_level() {
        let mut tree = AuthTree::new();
        tree.insert(
            "User".into(),
            AuthRequirement {
                requires_auth: true,
                scopes: vec![],
            },
        );
        tree.insert(
            "User.email".into(),
            AuthRequirement {
                requires_auth: true,
                scopes: vec![vec!["read:email".into()]],
            },
        );

        let req = tree.requirement("User", "email").unwrap();
        assert!(!req.scopes.is_empty());
    }

    #[test]
    fn falls_back_to_type_level() {
        let mut tree = AuthTree::new();
        tree.insert(
            "User".into(),
            AuthRequirement {
                requires_auth: true,
                scopes: vec![],
            },
        );

        let req = tree.requirement("User", "name").unwrap();
        assert!(req.requires_auth);
    }

    #[test]
    fn returns_none_for_unprotected() {
        let tree = AuthTree::new();
        assert!(tree.requirement("User", "name").is_none());
    }

    #[test]
    fn check_access_requires_auth() {
        let req = AuthRequirement {
            requires_auth: true,
            scopes: vec![],
        };
        assert!(!AuthTree::check_access(&req, false, &[]));
        assert!(AuthTree::check_access(&req, true, &[]));
    }

    #[test]
    fn check_access_requires_scopes() {
        let req = AuthRequirement {
            requires_auth: false,
            scopes: vec![vec!["read:users".into(), "read:org".into()]],
        };
        assert!(!AuthTree::check_access(&req, true, &["read:users".into()]));
        assert!(AuthTree::check_access(
            &req,
            true,
            &["read:users".into(), "read:org".into()]
        ));
    }

    #[test]
    fn check_access_or_groups() {
        let req = AuthRequirement {
            requires_auth: false,
            scopes: vec![vec!["read:users".into(), "read:org".into()], vec!["admin".into()]],
        };
        // admin alone satisfies second group.
        assert!(AuthTree::check_access(&req, true, &["admin".into()]));
    }
}
