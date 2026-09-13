//! 文件夹优先去重的纯领域契约与清理计划生成器。
//!
//! 这里不访问数据库，也不接触文件系统。查询层只负责把当前 exact-ready 组投影成
//! [`DedupFolderPlanGroup`]；preview/apply 再复用同一个生成器，避免前后端各维护一套
//! 默认选择规则。

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 保护元数据摘要。任一保护字段成立，都表示该位置默认需要人工复核。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupProtectionSummary {
    pub is_favorited: bool,
    pub rating: i64,
    pub color_label: i64,
    pub album_count: u64,
    pub tag_count: u64,
    pub bookmark_count: u64,
}

impl DedupProtectionSummary {
    /// 收藏、评分、颜色、相册、标签和书签都属于用户明确资产，不能被文件夹偏好覆盖。
    pub fn is_protected(&self) -> bool {
        self.is_favorited
            || self.rating > 0
            || self.color_label > 0
            || self.album_count > 0
            || self.tag_count > 0
            || self.bookmark_count > 0
    }
}

/// 精确逻辑单元的稳定内存键。IPC 层会把它编码成不透明 group key。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DedupFolderGroupKey {
    pub unit_digest: Vec<u8>,
    pub unit_size: i64,
}

/// 参与某个目标文件夹计划计算的当前 exact-ready 成员。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupFolderPlanMember {
    pub item_id: i64,
    pub in_target: bool,
    pub availability: String,
    pub rating: i64,
    pub color_label: i64,
    pub protection: DedupProtectionSummary,
    pub physical_key: Option<Vec<u8>>,
}

/// 一个与目标范围相交的精确重复组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupFolderPlanGroup {
    pub key: DedupFolderGroupKey,
    pub unit_size: i64,
    /// 查询层按既有 keeper 规则提供；纯函数仍会对缺失/漂移的值做确定性回退。
    pub suggested_keeper_id: Option<i64>,
    pub members: Vec<DedupFolderPlanMember>,
}

/// 文件夹清理计划的基准模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DedupFolderPlanBaseMode {
    Recommended,
    None,
}

/// 纯计划生成的稳定错误。IPC 层把它映射为不泄漏内部细节的稳定 code。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DedupFolderPlanError {
    #[error("cleanup override contains a duplicate item id")]
    DuplicateOverrideItem,
    #[error("cleanup override contains an item outside the target duplicate groups")]
    ItemNotInTarget,
    #[error("cleanup override includes a keeper")]
    KeeperConflict,
    #[error("cleanup selection would remove every member of a duplicate group")]
    GroupWouldLoseKeeper,
}

/// 计划生成结果。选中的 ID 只在后端事务/预览内部存在，不直接作为大数组下发 IPC。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupFolderCleanupPlan {
    pub selected_item_ids: Vec<i64>,
    pub selected_position_count: u64,
    pub affected_group_count: u64,
    pub logical_bytes: u64,
}

fn is_online(availability: &str) -> bool {
    availability == "online"
}

fn fallback_keeper_id(group: &DedupFolderPlanGroup) -> Option<i64> {
    group
        .members
        .iter()
        .min_by(|left, right| {
            is_online(&right.availability)
                .cmp(&is_online(&left.availability))
                .then_with(|| {
                    right
                        .protection
                        .is_protected()
                        .cmp(&left.protection.is_protected())
                })
                .then_with(|| right.rating.cmp(&left.rating))
                .then_with(|| right.color_label.cmp(&left.color_label))
                .then_with(|| left.item_id.cmp(&right.item_id))
        })
        .map(|member| member.item_id)
}

fn keeper_id(group: &DedupFolderPlanGroup) -> Option<i64> {
    group
        .suggested_keeper_id
        .filter(|id| group.members.iter().any(|member| member.item_id == *id))
        .or_else(|| fallback_keeper_id(group))
}

fn is_physical_alias(group: &DedupFolderPlanGroup, item_id: i64) -> bool {
    let Some(member) = group
        .members
        .iter()
        .find(|member| member.item_id == item_id)
    else {
        return false;
    };
    let Some(physical_key) = member.physical_key.as_ref() else {
        return false;
    };
    group
        .members
        .iter()
        .filter(|candidate| candidate.physical_key.as_ref() == Some(physical_key))
        .count()
        > 1
}

/// 按文件夹优先方案生成默认清理选择，并应用少量前端 override。
///
/// - 有范围外成员的组：目标范围内所有未保护成员默认进入计划；
/// - 只在目标范围内部重复的组：按 keeper 规则保留一个，其余未保护成员进入计划；
/// - `none` 模式只接受显式 include；
/// - 任一组都不得被选到零保留。
pub fn generate_cleanup_plan(
    groups: &[DedupFolderPlanGroup],
    base_mode: DedupFolderPlanBaseMode,
    included_item_ids: &[i64],
    excluded_item_ids: &[i64],
) -> std::result::Result<DedupFolderCleanupPlan, DedupFolderPlanError> {
    let mut item_to_group = HashMap::new();
    let mut included = BTreeSet::new();
    let mut excluded = BTreeSet::new();

    for group in groups {
        for member in &group.members {
            if item_to_group
                .insert(member.item_id, group.key.clone())
                .is_some()
            {
                // 一个媒体项不应同时属于两个逻辑组；把异常输入视为无效计划，而不是
                // 让同一位置在逻辑字节统计中被重复计算。
                return Err(DedupFolderPlanError::ItemNotInTarget);
            }
        }
    }

    for item_id in included_item_ids {
        if !included.insert(*item_id) {
            return Err(DedupFolderPlanError::DuplicateOverrideItem);
        }
        if !item_to_group.contains_key(item_id)
            || !groups.iter().any(|group| {
                group
                    .members
                    .iter()
                    .any(|member| member.item_id == *item_id && member.in_target)
            })
        {
            return Err(DedupFolderPlanError::ItemNotInTarget);
        }
    }
    for item_id in excluded_item_ids {
        if !excluded.insert(*item_id) {
            return Err(DedupFolderPlanError::DuplicateOverrideItem);
        }
        if !item_to_group.contains_key(item_id)
            || !groups.iter().any(|group| {
                group
                    .members
                    .iter()
                    .any(|member| member.item_id == *item_id && member.in_target)
            })
        {
            return Err(DedupFolderPlanError::ItemNotInTarget);
        }
    }
    if included.intersection(&excluded).next().is_some() {
        return Err(DedupFolderPlanError::DuplicateOverrideItem);
    }

    let mut selected_item_ids = BTreeSet::new();
    let mut affected_group_count = 0_u64;
    let mut logical_bytes = 0_u64;

    for group in groups {
        if group.members.len() < 2 {
            continue;
        }
        let external_member_exists = group.members.iter().any(|member| !member.in_target);
        let keeper_id = keeper_id(group).ok_or(DedupFolderPlanError::GroupWouldLoseKeeper)?;
        let default_selection = group
            .members
            .iter()
            .filter(|member| {
                member.in_target
                    && !member.protection.is_protected()
                    && !is_physical_alias(group, member.item_id)
            })
            .filter(|member| external_member_exists || member.item_id != keeper_id)
            .map(|member| member.item_id)
            .collect::<BTreeSet<_>>();

        let mut group_selection = match base_mode {
            DedupFolderPlanBaseMode::Recommended => default_selection,
            DedupFolderPlanBaseMode::None => BTreeSet::new(),
        };
        group_selection.retain(|item_id| !excluded.contains(item_id));
        group_selection.extend(
            included
                .iter()
                .filter(|item_id| {
                    group
                        .members
                        .iter()
                        .any(|member| member.item_id == **item_id)
                })
                .copied(),
        );

        if !external_member_exists && group_selection.contains(&keeper_id) {
            return Err(DedupFolderPlanError::KeeperConflict);
        }
        if group_selection.len() == group.members.len() {
            return Err(DedupFolderPlanError::GroupWouldLoseKeeper);
        }

        if !group_selection.is_empty() {
            affected_group_count = affected_group_count.saturating_add(1);
            let selected_count = u64::try_from(group_selection.len()).unwrap_or(u64::MAX);
            logical_bytes = logical_bytes.saturating_add(
                u64::try_from(group.unit_size.max(0))
                    .unwrap_or(0)
                    .saturating_mul(selected_count),
            );
            selected_item_ids.extend(group_selection);
        }
    }

    let selected_position_count = u64::try_from(selected_item_ids.len()).unwrap_or(u64::MAX);
    Ok(DedupFolderCleanupPlan {
        selected_item_ids: selected_item_ids.into_iter().collect(),
        selected_position_count,
        affected_group_count,
        logical_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(item_id: i64, in_target: bool, protected: bool) -> DedupFolderPlanMember {
        DedupFolderPlanMember {
            item_id,
            in_target,
            availability: "online".to_string(),
            rating: 0,
            color_label: 0,
            protection: DedupProtectionSummary {
                is_favorited: protected,
                rating: 0,
                color_label: 0,
                album_count: 0,
                tag_count: 0,
                bookmark_count: 0,
            },
            physical_key: None,
        }
    }

    fn group(keeper: i64, members: Vec<DedupFolderPlanMember>) -> DedupFolderPlanGroup {
        DedupFolderPlanGroup {
            key: DedupFolderGroupKey {
                unit_digest: vec![keeper as u8],
                unit_size: 100,
            },
            unit_size: 100,
            suggested_keeper_id: Some(keeper),
            members,
        }
    }

    #[test]
    fn external_a_b_coverage_cleans_only_target_side() {
        let plan = generate_cleanup_plan(
            &[group(
                1,
                vec![member(1, false, false), member(2, true, false)],
            )],
            DedupFolderPlanBaseMode::Recommended,
            &[],
            &[],
        )
        .expect("A/B plan");

        assert_eq!(plan.selected_item_ids, vec![2]);
        assert_eq!(plan.affected_group_count, 1);
        assert_eq!(plan.logical_bytes, 100);
    }

    #[test]
    fn parent_child_scope_is_external_but_does_not_change_keeper_safety() {
        // The parent member is outside a child target. Scope/source UI decides whether the
        // ancestor is a displayable source; the pure plan only guarantees the child can be
        // cleaned while the parent member remains.
        let plan = generate_cleanup_plan(
            &[group(
                10,
                vec![member(10, false, false), member(11, true, false)],
            )],
            DedupFolderPlanBaseMode::Recommended,
            &[],
            &[],
        )
        .expect("parent/child plan");
        assert_eq!(plan.selected_item_ids, vec![11]);
    }

    #[test]
    fn internal_duplicate_keeps_one_and_skips_protected_member() {
        let plan = generate_cleanup_plan(
            &[group(
                20,
                vec![
                    member(20, true, false),
                    member(21, true, true),
                    member(22, true, false),
                ],
            )],
            DedupFolderPlanBaseMode::Recommended,
            &[],
            &[],
        )
        .expect("internal plan");
        assert_eq!(plan.selected_item_ids, vec![22]);
        assert_eq!(plan.logical_bytes, 100);
    }

    #[test]
    fn overrides_can_add_protected_item_but_unknown_item_is_rejected() {
        let groups = [group(
            30,
            vec![member(30, true, false), member(31, true, true)],
        )];
        let plan = generate_cleanup_plan(&groups, DedupFolderPlanBaseMode::None, &[31], &[])
            .expect("explicit protected selection");
        assert_eq!(plan.selected_item_ids, vec![31]);

        assert_eq!(
            generate_cleanup_plan(&groups, DedupFolderPlanBaseMode::None, &[999], &[])
                .expect_err("unknown item must fail"),
            DedupFolderPlanError::ItemNotInTarget
        );
    }

    #[test]
    fn physical_aliases_are_review_only_by_default() {
        let mut first = member(40, true, false);
        first.physical_key = Some(b"same-file".to_vec());
        let mut second = member(41, true, false);
        second.physical_key = Some(b"same-file".to_vec());

        let plan = generate_cleanup_plan(
            &[group(40, vec![first, second])],
            DedupFolderPlanBaseMode::Recommended,
            &[],
            &[],
        )
        .expect("physical aliases remain review-only");

        assert!(plan.selected_item_ids.is_empty());
    }
}
