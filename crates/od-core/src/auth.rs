//! Login and permission rules for the port.
//!
//! Password formats come from `od_domain::password` (an exact port of Open
//! Dental's `Authentication`). Group resolution follows the 24.3 model: a
//! user's groups come from `usergroupattach`, with the deprecated
//! `userod.UserGroupNum` kept as a fallback, and a grant exists when a
//! `grouppermission` row matches one of those groups.

use od_domain::password;
use od_domain::{
    FeatureSettings, GroupPermission, Module, Permission, Session, UserGroupAttach, Userod,
};

/// Verify a typed password against the encoded `userod.Password` value.
pub fn verify_login(user: &Userod, input_pass: &str) -> bool {
    if user.is_hidden {
        return false;
    }
    password::verify(&user.password, input_pass)
}

/// All permissions held by a user, including groups attached through
/// `usergroupattach` and the legacy single-group column.
pub fn resolve_permissions(
    user: &Userod,
    attaches: &[UserGroupAttach],
    grants: &[GroupPermission],
) -> Vec<Permission> {
    let mut groups: Vec<od_domain::Id> = attaches
        .iter()
        .filter(|a| a.user_num == user.user_num)
        .map(|a| a.user_group_num)
        .collect();
    if user.user_group_num != 0 && !groups.contains(&user.user_group_num) {
        groups.push(user.user_group_num);
    }

    let mut permissions: Vec<Permission> = Vec::new();
    for grant in grants {
        if groups.contains(&grant.user_group_num)
            && !permissions.contains(&grant.perm_type)
        {
            permissions.push(grant.perm_type);
        }
    }
    permissions.sort_by_key(|p| p.as_od());
    permissions
}

/// True when `enforce` is off (feature toggles alone decide access) or the
/// session holds the permission. `NONE` is never a real restriction.
pub fn can(permission: Permission, session: &Session, enforce: bool) -> bool {
    !enforce || permission == Permission::NONE || session.has(permission)
}

/// Module access combines the user-facing feature toggle with the Open
/// Dental module permission.
pub fn can_access_module(
    module: Module,
    session: &Session,
    features: &FeatureSettings,
    enforce: bool,
) -> bool {
    features.module_enabled(module) && can(Permission::for_module(module), session, enforce)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(num: u32, group: u32) -> Userod {
        Userod {
            user_num: num,
            user_name: format!("user{num}"),
            password: password::generate("secret", password::HashType::Sha3_512).encode(),
            user_group_num: group,
            employee_num: 0,
            clinic_num: 0,
            is_hidden: false,
        }
    }

    #[test]
    fn login_accepts_only_the_matching_password() {
        let user = user(1, 1);
        assert!(verify_login(&user, "secret"));
        assert!(!verify_login(&user, "wrong"));
        let mut hidden = user;
        hidden.is_hidden = true;
        assert!(!verify_login(&hidden, "secret"));
    }

    #[test]
    fn permissions_come_from_attached_and_legacy_groups() {
        let user = user(1, 9);
        let attaches = vec![UserGroupAttach {
            user_group_attach_num: 1,
            user_num: 1,
            user_group_num: 2,
        }];
        let grants = vec![
            GroupPermission {
                group_perm_num: 1,
                user_group_num: 2,
                perm_type: Permission::APPOINTMENTS_MODULE,
                ..Default::default()
            },
            GroupPermission {
                group_perm_num: 2,
                user_group_num: 9,
                perm_type: Permission::FAMILY_MODULE,
                ..Default::default()
            },
            GroupPermission {
                group_perm_num: 3,
                user_group_num: 77,
                perm_type: Permission::SETUP,
                ..Default::default()
            },
        ];
        let perms = resolve_permissions(&user, &attaches, &grants);
        assert_eq!(
            perms,
            vec![Permission::APPOINTMENTS_MODULE, Permission::FAMILY_MODULE]
        );
    }

    #[test]
    fn enforcement_is_opt_in() {
        let session = Session::local();
        assert!(can(Permission::SETUP, &session, false));
        assert!(!can(Permission::SETUP, &Session {
            user_num: 2,
            user_name: "limited".into(),
            permissions: vec![],
        }, true));
        let features = FeatureSettings::default();
        assert!(can_access_module(Module::Family, &session, &features, true));
    }
}
