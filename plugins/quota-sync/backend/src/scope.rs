use std::collections::BTreeSet;

/// 判断 Client Key 当前显式账号组范围是否包含目标账号。
///
/// Key 没有显式分组时，宿主语义是 AllAccounts，因此这里返回 true。
pub(crate) fn key_scope_allows_account(
    key_group_ids: &[String],
    account_group_ids: &[String],
) -> bool {
    if key_group_ids.is_empty() {
        return true;
    }
    let account_groups = account_group_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    key_group_ids
        .iter()
        .any(|group_id| account_groups.contains(group_id.as_str()))
}

#[cfg(test)]
mod tests {
    use super::key_scope_allows_account;

    #[test]
    fn all_accounts_key_allows_any_account() {
        assert!(key_scope_allows_account(&[], &[]));
    }

    #[test]
    fn explicit_scope_requires_group_overlap() {
        assert!(key_scope_allows_account(
            &["grp_a".to_owned()],
            &["grp_a".to_owned(), "grp_b".to_owned()],
        ));
        assert!(!key_scope_allows_account(
            &["grp_c".to_owned()],
            &["grp_a".to_owned(), "grp_b".to_owned()],
        ));
    }
}
