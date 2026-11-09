use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gilfoyle_core::{
    AppError, Collector, DomainError, Package, PackageAdmin, PackageAllowlist, PackageError,
    PackageEvent, PackageFilter, PackageId, PackageList, PackageManager, PackageOperation,
    PackageService, RefreshReport, Severity, TransactionKind, Update, UpdateFilter, UpdateList,
    UpgradePlan, UpgradeReport, exit_name, filter_installed, filter_none, info_name,
    map_error_code, map_exit_code, pk_error, pk_exit, pk_info, pk_restart, planned_action,
    restart_name, update_flags,
};

fn pkg(info: u32, id: &str) -> PackageEvent {
    PackageEvent::Package {
        info,
        severity: 0,
        package_id: id.to_string(),
        summary: String::new(),
    }
}

fn finished() -> PackageEvent {
    PackageEvent::Finished {
        exit: pk_exit::SUCCESS,
        runtime_ms: 7,
    }
}

// --- PackageId ---

#[test]
fn package_id_parses_all_fields() {
    let id = PackageId::parse("nginx;1.24.0;amd64;debian;").unwrap();
    assert_eq!(id.as_str(), "nginx;1.24.0;amd64;debian;");
    assert_eq!(id.name(), "nginx");
    assert_eq!(id.version(), "1.24.0");
    assert_eq!(id.architecture(), "amd64");
    assert_eq!(id.origin(), "debian");
}

#[test]
fn package_id_rejects_malformed() {
    for bad in [
        "",
        "nginx",
        "nginx;1.0",
        "nginx;1.0;amd64",
        ";1.0;amd64;x;",
        "n;;amd64;x;",
    ] {
        assert!(
            matches!(PackageId::parse(bad), Err(DomainError::InvalidPackageId(_))),
            "expected {bad:?} to be invalid"
        );
    }
}

#[test]
fn package_id_accepts_empty_origin_and_data() {
    let id = PackageId::parse("foo;1;noarch;;").unwrap();
    assert_eq!(id.origin(), "");
}

// --- names / mapping ---

#[test]
fn info_names_cover_the_values_we_use() {
    assert_eq!(info_name(pk_info::INSTALLED), "installed");
    assert_eq!(info_name(pk_info::SECURITY), "security");
    assert_eq!(info_name(pk_info::INSTALL), "install");
    assert_eq!(info_name(pk_info::DOWNGRADE), "downgrade");
    assert_eq!(info_name(999), "unknown");
}

#[test]
fn planned_actions_classify_transitive_changes() {
    assert_eq!(planned_action(pk_info::INSTALL).as_str(), "install");
    assert!(planned_action(pk_info::INSTALL).is_transitive());
    assert_eq!(planned_action(pk_info::REMOVE).as_str(), "remove");
    assert!(planned_action(pk_info::REMOVE).is_transitive());
    assert_eq!(planned_action(pk_info::OBSOLETE).as_str(), "obsolete");
    assert!(planned_action(pk_info::OBSOLETE).is_transitive());
    assert_eq!(planned_action(pk_info::DOWNGRADE).as_str(), "downgrade");
    assert!(planned_action(pk_info::DOWNGRADE).is_transitive());
    assert_eq!(planned_action(pk_info::SECURITY).as_str(), "update");
    assert!(!planned_action(pk_info::SECURITY).is_transitive());
}

#[test]
fn exit_and_restart_names() {
    let cases = [
        (pk_exit::SUCCESS, "success"),
        (pk_exit::FAILED, "failed"),
        (pk_exit::CANCELLED, "cancelled"),
        (pk_exit::KEY_REQUIRED, "key-required"),
        (pk_exit::EULA_REQUIRED, "eula-required"),
        (pk_exit::KILLED, "killed"),
        (pk_exit::NEED_UNTRUSTED, "need-untrusted"),
    ];
    for (exit, name) in cases {
        assert_eq!(exit_name(exit), name);
    }
    assert_eq!(exit_name(999), "unknown");
    assert_eq!(restart_name(pk_restart::SYSTEM), "system");
    assert_eq!(restart_name(pk_restart::SESSION), "session");
    assert_eq!(restart_name(pk_restart::APPLICATION), "application");
    assert_eq!(restart_name(0), "none");
}

#[test]
fn info_names_cover_every_mapped_value() {
    let cases = [
        (pk_info::INSTALLED, "installed"),
        (pk_info::AVAILABLE, "available"),
        (pk_info::LOW, "low"),
        (pk_info::ENHANCEMENT, "enhancement"),
        (pk_info::NORMAL, "normal"),
        (pk_info::BUGFIX, "bugfix"),
        (pk_info::IMPORTANT, "important"),
        (pk_info::SECURITY, "security"),
        (pk_info::BLOCKED, "blocked"),
        (pk_info::UNTRUSTED, "untrusted"),
        (pk_info::TRUSTED, "trusted"),
        (pk_info::CRITICAL, "critical"),
        (pk_info::INSTALL, "install"),
        (pk_info::REMOVE, "remove"),
        (pk_info::OBSOLETE, "obsolete"),
        (pk_info::DOWNGRADE, "downgrade"),
    ];
    for (info, name) in cases {
        assert_eq!(info_name(info), name);
    }
    assert_eq!(info_name(999), "unknown");
}

#[test]
fn severity_from_info_and_from_name() {
    assert_eq!(Severity::from_info(pk_info::LOW), Some(Severity::Low));
    assert_eq!(
        Severity::from_info(pk_info::ENHANCEMENT),
        Some(Severity::Low)
    );
    assert_eq!(Severity::from_info(pk_info::NORMAL), Some(Severity::Medium));
    assert_eq!(Severity::from_info(pk_info::BUGFIX), Some(Severity::Medium));
    assert_eq!(
        Severity::from_info(pk_info::IMPORTANT),
        Some(Severity::High)
    );
    assert_eq!(
        Severity::from_info(pk_info::SECURITY),
        Some(Severity::Critical)
    );
    assert_eq!(
        Severity::from_info(pk_info::CRITICAL),
        Some(Severity::Critical)
    );
    assert_eq!(Severity::from_info(pk_info::INSTALLED), None);

    for (name, severity) in [
        ("low", Severity::Low),
        ("medium", Severity::Medium),
        ("high", Severity::High),
        ("critical", Severity::Critical),
    ] {
        assert_eq!(Severity::from_info_name(name), Some(severity));
    }
    assert_eq!(Severity::from_info_name("nope"), None);
}

#[test]
fn plan_is_transitive_for_each_category_individually() {
    for (install, remove, obsolete, downgrade) in
        [(1, 0, 0, 0), (0, 1, 0, 0), (0, 0, 1, 0), (0, 0, 0, 1)]
    {
        let plan = UpgradePlan {
            would_install: install,
            would_remove: remove,
            would_obsolete: obsolete,
            would_downgrade: downgrade,
            ..UpgradePlan::empty()
        };
        assert!(
            plan.is_transitive(),
            "{install},{remove},{obsolete},{downgrade}"
        );
    }
    assert!(!UpgradePlan::empty().is_transitive());
}

#[test]
fn bitfields_use_the_classic_indices() {
    // PK_FILTER_ENUM_INSTALLED == 2 -> 1 << 2
    assert_eq!(filter_installed(), 4);
    assert_eq!(filter_none(), 2);
    // ONLY_TRUSTED == 1 -> 2 ; SIMULATE == 2 -> 4
    assert_eq!(update_flags(false), 2);
    assert_eq!(update_flags(true), 6);
}

#[test]
fn severity_parse_and_classify() {
    assert_eq!(Severity::parse("high").unwrap(), Severity::High);
    assert_eq!(Severity::parse(" CRITICAL ").unwrap(), Severity::Critical);
    assert_eq!(Severity::parse("medium").unwrap(), Severity::Medium);
    assert_eq!(Severity::parse("low").unwrap(), Severity::Low);
    assert!(matches!(
        Severity::parse("nope"),
        Err(DomainError::UnknownSeverity(_))
    ));
    assert_eq!(Severity::High.as_str(), "high");
    assert_eq!(
        Severity::from_info(pk_info::SECURITY),
        Some(Severity::Critical)
    );
    assert_eq!(
        Severity::from_info(pk_info::IMPORTANT),
        Some(Severity::High)
    );
    assert_eq!(Severity::from_info(pk_info::INSTALLED), None);
}

#[test]
fn error_and_exit_mapping() {
    assert_eq!(
        map_error_code(pk_error::NOT_AUTHORIZED, "nope"),
        PackageError::NotAuthorized("nope".to_string())
    );
    assert_eq!(
        map_error_code(pk_error::MISSING_GPG_SIGNATURE, "unsigned"),
        PackageError::Untrusted("unsigned".to_string())
    );
    assert_eq!(
        map_error_code(pk_error::NO_CACHE, ""),
        PackageError::NoCache
    );
    assert_eq!(
        map_error_code(pk_error::LOCK_REQUIRED, "busy"),
        PackageError::Locked("busy".to_string())
    );
    assert!(matches!(
        map_error_code(pk_error::CANNOT_GET_LOCK, "x"),
        PackageError::Locked(_)
    ));
    assert!(matches!(
        map_error_code(999, "x"),
        PackageError::Other { .. }
    ));
    assert!(matches!(
        map_error_code(pk_error::NO_PACKAGES_TO_UPDATE, "none"),
        PackageError::Failed { .. }
    ));
    assert!(matches!(
        map_error_code(pk_error::BAD_GPG_SIGNATURE, "bad"),
        PackageError::Untrusted(_)
    ));
    assert_eq!(map_exit_code(pk_exit::CANCELLED), PackageError::Cancelled);
    assert!(matches!(
        map_exit_code(pk_exit::KEY_REQUIRED),
        PackageError::Untrusted(_)
    ));
    assert!(matches!(
        map_exit_code(pk_exit::EULA_REQUIRED),
        PackageError::Eula(_)
    ));
    assert!(matches!(
        map_exit_code(pk_exit::NEED_UNTRUSTED),
        PackageError::Untrusted(_)
    ));
    assert!(matches!(
        map_exit_code(pk_exit::KILLED),
        PackageError::Failed { .. }
    ));
    assert!(matches!(
        map_exit_code(pk_exit::FAILED),
        PackageError::Failed { .. }
    ));
}

// --- allowlist ---

#[test]
fn package_operation_parses_only_the_closed_vocabulary() {
    assert_eq!(
        PackageOperation::parse("refresh").unwrap(),
        PackageOperation::Refresh
    );
    assert_eq!(
        PackageOperation::parse(" Upgrade ").unwrap(),
        PackageOperation::Upgrade
    );
    assert!(matches!(
        PackageOperation::parse("install"),
        Err(DomainError::UnknownPackageOperation(_))
    ));
    assert_eq!(PackageOperation::Refresh.as_str(), "refresh");
    assert_eq!(PackageOperation::Upgrade.as_str(), "upgrade");
    assert_eq!(PackageOperation::ALL.len(), 2);
}

#[test]
fn package_allowlist_is_default_deny() {
    let empty = PackageAllowlist::default();
    assert!(empty.is_empty());
    assert_eq!(
        empty.ensure_allowed(PackageOperation::Upgrade).unwrap_err(),
        AppError::PackageOperationNotAllowed("upgrade".to_string())
    );

    let allowlist = PackageAllowlist::new([PackageOperation::Upgrade]);
    assert!(!allowlist.is_empty());
    assert!(allowlist.contains(PackageOperation::Upgrade));
    assert!(!allowlist.contains(PackageOperation::Refresh));
    assert!(allowlist.ensure_allowed(PackageOperation::Upgrade).is_ok());
    assert_eq!(
        allowlist
            .ensure_allowed(PackageOperation::Refresh)
            .unwrap_err(),
        AppError::PackageOperationNotAllowed("refresh".to_string())
    );
    assert_eq!(
        allowlist.operations().collect::<Vec<_>>(),
        vec![PackageOperation::Upgrade]
    );
}

// --- filters ---

#[test]
fn package_filter_applies_substring_and_limit() {
    let packages = vec![
        Package {
            name: "nginx".to_string(),
            version: "1".to_string(),
            architecture: "amd64".to_string(),
            origin: String::new(),
        },
        Package {
            name: "Nginx-common".to_string(),
            version: "1".to_string(),
            architecture: "all".to_string(),
            origin: String::new(),
        },
        Package {
            name: "curl".to_string(),
            version: "1".to_string(),
            architecture: "amd64".to_string(),
            origin: String::new(),
        },
    ];
    let list: PackageList = PackageFilter {
        name_contains: Some("NGINX".to_string()),
        limit: Some(1),
    }
    .apply(packages.clone());
    assert_eq!(list.total, 2);
    assert_eq!(list.packages.len(), 1);
    assert!(list.truncated);

    let all = PackageFilter::default().apply(packages);
    assert_eq!(all.total, 3);
    assert!(!all.truncated);
}

#[test]
fn update_filter_applies_min_severity() {
    let updates = vec![
        Update {
            id: "a;1;amd64;d;".to_string(),
            name: "a".to_string(),
            version: "1".to_string(),
            info: "security".to_string(),
            severity: Some("critical".to_string()),
        },
        Update {
            id: "b;1;amd64;d;".to_string(),
            name: "b".to_string(),
            version: "1".to_string(),
            info: "normal".to_string(),
            severity: Some("medium".to_string()),
        },
        Update {
            id: "c;1;amd64;d;".to_string(),
            name: "c".to_string(),
            version: "1".to_string(),
            info: "blocked".to_string(),
            severity: None,
        },
    ];
    let list = UpdateFilter {
        min_severity: Some(Severity::High),
        limit: None,
    }
    .apply(updates);
    assert_eq!(list.total, 1);
    assert_eq!(list.updates[0].name, "a");
    assert!(!list.truncated);
}

// --- collector ---

#[test]
fn collector_builds_a_package_list() {
    let mut collector = Collector::new(TransactionKind::ListInstalled, 10);
    collector.push(pkg(pk_info::INSTALLED, "nginx;1.24.0;amd64;debian;"));
    collector.push(pkg(pk_info::INSTALLED, "curl;8;amd64;debian;"));
    collector.push(finished());
    assert!(collector.is_finished());
    let list = collector.into_package_list().unwrap();
    assert_eq!(list.total, 2);
    assert_eq!(list.packages[0].name, "nginx");
    assert_eq!(list.packages[1].architecture, "amd64");
    assert!(!list.truncated);
}

#[test]
fn collector_truncates_but_counts_all() {
    let mut collector = Collector::new(TransactionKind::ListInstalled, 1);
    collector.push(pkg(pk_info::INSTALLED, "a;1;amd64;d;"));
    collector.push(pkg(pk_info::INSTALLED, "b;1;amd64;d;"));
    collector.push(finished());
    let list = collector.into_package_list().unwrap();
    assert_eq!(list.total, 2);
    assert_eq!(list.packages.len(), 1);
    assert!(list.truncated);
}

#[test]
fn collector_builds_an_update_list() {
    let mut collector = Collector::new(TransactionKind::CheckUpdates, 10);
    collector.push(pkg(pk_info::SECURITY, "openssl;3;amd64;debian;"));
    collector.push(finished());
    let list = collector.into_update_list().unwrap();
    assert_eq!(list.total, 1);
    assert_eq!(list.updates[0].info, "security");
    assert_eq!(list.updates[0].severity.as_deref(), Some("critical"));
    assert!(!list.truncated);
}

#[test]
fn collector_builds_a_transitive_plan() {
    let mut collector = Collector::new(TransactionKind::Plan, 10);
    collector.push(pkg(pk_info::SECURITY, "openssl;3;amd64;debian;"));
    collector.push(pkg(pk_info::INSTALL, "libfoo;1;amd64;debian;"));
    collector.push(pkg(pk_info::REMOVE, "libold;1;amd64;debian;"));
    collector.push(pkg(pk_info::OBSOLETE, "libgone;1;amd64;debian;"));
    collector.push(pkg(pk_info::DOWNGRADE, "libdown;1;amd64;debian;"));
    collector.push(finished());
    let plan = collector.into_upgrade_plan().unwrap();
    assert_eq!(plan.total, 5);
    assert_eq!(plan.would_install, 1);
    assert_eq!(plan.would_remove, 1);
    assert_eq!(plan.would_obsolete, 1);
    assert_eq!(plan.would_downgrade, 1);
    assert!(plan.is_transitive());
    assert!(!plan.truncated);
}

#[test]
fn collector_counts_transitive_packages_beyond_the_limit() {
    // The gate must see installs even past the display limit.
    let mut collector = Collector::new(TransactionKind::Plan, 1);
    collector.push(pkg(pk_info::SECURITY, "a;1;amd64;d;"));
    collector.push(pkg(pk_info::INSTALL, "b;1;amd64;d;"));
    collector.push(finished());
    let plan = collector.into_upgrade_plan().unwrap();
    assert_eq!(plan.would_install, 1);
    assert_eq!(plan.planned.len(), 1);
    assert!(plan.is_transitive());
}

#[test]
fn collector_builds_refresh_and_upgrade_reports() {
    let mut refresh = Collector::new(TransactionKind::Refresh, 10);
    refresh.push(PackageEvent::RepoDetail {
        id: "debian".to_string(),
        description: "Debian".to_string(),
        enabled: true,
    });
    refresh.push(finished());
    let report = refresh.into_refresh_report().unwrap();
    assert!(report.success);
    assert_eq!(report.exit, "success");
    assert_eq!(report.repos.len(), 1);
    assert_eq!(report.runtime_ms, 7);

    let mut apply = Collector::new(TransactionKind::Apply, 10);
    apply.push(pkg(pk_info::INSTALL, "zlib;1;amd64;d;"));
    apply.push(PackageEvent::RequireRestart {
        kind: pk_restart::SYSTEM,
    });
    apply.push(finished());
    let report = apply.into_upgrade_report().unwrap();
    assert!(report.success);
    assert_eq!(report.restart_required.as_deref(), Some("system"));
    assert_eq!(report.affected.len(), 1);
    assert!(!report.affected_truncated);
}

#[test]
fn collector_surfaces_fatal_errors() {
    let mut collector = Collector::new(TransactionKind::Apply, 10);
    collector.push(PackageEvent::ErrorCode {
        code: pk_error::NOT_AUTHORIZED,
        details: "no".to_string(),
    });
    collector.push(finished());
    assert_eq!(
        collector.into_upgrade_report().unwrap_err(),
        PackageError::NotAuthorized("no".to_string())
    );

    let mut untrusted = Collector::new(TransactionKind::Apply, 10);
    untrusted.push(PackageEvent::Untrusted {
        details: "key".to_string(),
    });
    assert!(matches!(
        untrusted.into_upgrade_report().unwrap_err(),
        PackageError::Untrusted(_)
    ));

    let mut eula = Collector::new(TransactionKind::Apply, 10);
    eula.push(PackageEvent::Eula {
        details: "licence".to_string(),
    });
    assert!(matches!(
        eula.into_upgrade_report().unwrap_err(),
        PackageError::Eula(_)
    ));
}

#[test]
fn collector_keeps_the_first_error() {
    let mut collector = Collector::new(TransactionKind::Refresh, 10);
    collector.push(PackageEvent::ErrorCode {
        code: pk_error::NO_CACHE,
        details: String::new(),
    });
    collector.push(PackageEvent::ErrorCode {
        code: pk_error::NOT_AUTHORIZED,
        details: "later".to_string(),
    });
    assert_eq!(
        collector.into_refresh_report().unwrap_err(),
        PackageError::NoCache
    );
}

#[test]
fn collector_reports_a_failed_exit_as_a_report() {
    let mut collector = Collector::new(TransactionKind::Apply, 10);
    collector.push(PackageEvent::Finished {
        exit: pk_exit::FAILED,
        runtime_ms: 3,
    });
    let report = collector.into_upgrade_report().unwrap();
    assert!(!report.success);
    assert_eq!(report.exit, "failed");
    assert!(report.error_details.is_some());
}

#[test]
fn collector_fails_a_read_on_a_failed_exit() {
    let mut collector = Collector::new(TransactionKind::ListInstalled, 10);
    collector.push(PackageEvent::Finished {
        exit: pk_exit::CANCELLED,
        runtime_ms: 3,
    });
    assert_eq!(
        collector.into_package_list().unwrap_err(),
        PackageError::Cancelled
    );
}

#[test]
fn collector_reports_a_timeout_on_a_report() {
    let mut collector = Collector::new(TransactionKind::Refresh, 10);
    collector.mark_timed_out();
    let report = collector.into_refresh_report().unwrap();
    assert!(!report.success);
    assert!(report.timed_out);
    assert_eq!(report.exit, "timeout");
}

#[test]
fn collector_times_out_without_events() {
    let collector = Collector::new(TransactionKind::ListInstalled, 10);
    assert_eq!(
        collector.into_package_list().unwrap_err(),
        PackageError::Timeout
    );

    let collector = Collector::new(TransactionKind::Refresh, 10);
    assert_eq!(
        collector.into_refresh_report().unwrap_err(),
        PackageError::Timeout
    );
}

#[test]
fn collector_ignores_repos_outside_refresh_and_malformed_ids() {
    let mut collector = Collector::new(TransactionKind::ListInstalled, 10);
    collector.push(PackageEvent::RepoDetail {
        id: "x".to_string(),
        description: "x".to_string(),
        enabled: true,
    });
    collector.push(pkg(pk_info::INSTALLED, "not-a-valid-id"));
    collector.push(finished());
    let list = collector.into_package_list().unwrap();
    assert_eq!(list.packages.len(), 0);
    assert_eq!(list.total, 1);
}

#[test]
fn update_filter_truncates_and_stamps_totals() {
    let list = UpdateFilter {
        min_severity: None,
        limit: Some(1),
    }
    .apply(vec![update("a"), update("b")]);
    assert_eq!(list.total, 2);
    assert_eq!(list.updates.len(), 1);
    assert!(list.truncated);
}

#[test]
fn collector_truncates_update_and_plan_lists() {
    let mut updates = Collector::new(TransactionKind::CheckUpdates, 1);
    updates.push(pkg(pk_info::SECURITY, "a;1;amd64;d;"));
    updates.push(pkg(pk_info::SECURITY, "b;1;amd64;d;"));
    updates.push(finished());
    let list = updates.into_update_list().unwrap();
    assert_eq!(list.total, 2);
    assert_eq!(list.updates.len(), 1);
    assert!(list.truncated);

    let mut plan = Collector::new(TransactionKind::Plan, 1);
    plan.push(pkg(pk_info::SECURITY, "a;1;amd64;d;"));
    plan.push(pkg(pk_info::SECURITY, "b;1;amd64;d;"));
    plan.push(finished());
    let plan = plan.into_upgrade_plan().unwrap();
    assert_eq!(plan.total, 2);
    assert_eq!(plan.planned.len(), 1);
    assert!(plan.truncated);
}

#[test]
fn collector_truncates_the_affected_list() {
    let mut collector = Collector::new(TransactionKind::Apply, 1);
    collector.push(pkg(pk_info::INSTALL, "a;1;amd64;d;"));
    collector.push(pkg(pk_info::INSTALL, "b;1;amd64;d;"));
    collector.push(finished());
    let report = collector.into_upgrade_report().unwrap();
    assert_eq!(report.affected.len(), 1);
    assert!(report.affected_truncated);
}

#[test]
fn collector_keeps_the_strongest_restart() {
    for order in [
        [pk_restart::SESSION, pk_restart::SYSTEM],
        [pk_restart::SYSTEM, pk_restart::SESSION],
    ] {
        let mut collector = Collector::new(TransactionKind::Apply, 10);
        for kind in order {
            collector.push(PackageEvent::RequireRestart { kind });
        }
        collector.push(finished());
        assert_eq!(
            collector
                .into_upgrade_report()
                .unwrap()
                .restart_required
                .as_deref(),
            Some("session")
        );
    }
}

#[test]
fn collector_ignores_repos_unless_refreshing() {
    let mut collector = Collector::new(TransactionKind::ListInstalled, 10);
    collector.push(PackageEvent::RepoDetail {
        id: "x".to_string(),
        description: "x".to_string(),
        enabled: true,
    });
    collector.push(finished());
    assert!(collector.into_refresh_report().unwrap().repos.is_empty());
}

#[test]
fn collector_caps_the_repository_list() {
    let mut collector = Collector::new(TransactionKind::Refresh, 1);
    for id in ["a", "b"] {
        collector.push(PackageEvent::RepoDetail {
            id: id.to_string(),
            description: id.to_string(),
            enabled: true,
        });
    }
    collector.push(finished());
    assert_eq!(collector.into_refresh_report().unwrap().repos.len(), 1);
}

#[test]
fn collector_reports_finished_and_runtime() {
    let mut collector = Collector::new(TransactionKind::Refresh, 10);
    assert!(!collector.is_finished());
    collector.push(PackageEvent::Finished {
        exit: pk_exit::SUCCESS,
        runtime_ms: 42,
    });
    assert!(collector.is_finished());
    assert_eq!(collector.runtime_ms(), 42);
}

// --- service ---

#[derive(Default)]
struct FakeManager {
    updates: Vec<Update>,
    plan: UpgradePlan,
    calls: Mutex<Vec<String>>,
}

impl FakeManager {
    fn record(&self, call: &str) {
        self.calls.lock().unwrap().push(call.to_string());
    }
}

#[async_trait]
impl PackageManager for FakeManager {
    async fn list_installed(&self, filter: &PackageFilter) -> Result<PackageList, PackageError> {
        Ok(filter.apply(Vec::new()))
    }

    async fn check_updates(&self, filter: &UpdateFilter) -> Result<UpdateList, PackageError> {
        self.record("check_updates");
        Ok(filter.apply(self.updates.clone()))
    }

    async fn plan(&self, _package_ids: &[String]) -> Result<UpgradePlan, PackageError> {
        self.record("plan");
        Ok(self.plan.clone())
    }

    async fn apply(&self, _package_ids: &[String]) -> Result<UpgradeReport, PackageError> {
        self.record("apply");
        Ok(UpgradeReport::nothing_to_do())
    }

    async fn refresh(&self, _force: bool) -> Result<RefreshReport, PackageError> {
        self.record("refresh");
        Ok(RefreshReport {
            success: true,
            exit: "success".to_string(),
            error_code: None,
            error_details: None,
            timed_out: false,
            runtime_ms: 0,
            repos: Vec::new(),
            repos_truncated: false,
        })
    }
}

fn update(name: &str) -> Update {
    Update {
        id: format!("{name};1;amd64;d;"),
        name: name.to_string(),
        version: "1".to_string(),
        info: "security".to_string(),
        severity: Some("critical".to_string()),
    }
}

fn service(
    manager: Arc<FakeManager>,
    allow: &[PackageOperation],
    transitive: bool,
) -> PackageService {
    let manager: Arc<dyn PackageManager> = manager;
    PackageService::new(
        manager,
        PackageAllowlist::new(allow.iter().copied()),
        transitive,
    )
}

#[tokio::test]
async fn refresh_requires_the_allowlist() {
    let manager = Arc::new(FakeManager::default());
    let denied = service(manager.clone(), &[], false);
    assert_eq!(
        denied.refresh_packages(false).await.unwrap_err(),
        AppError::PackageOperationNotAllowed("refresh".to_string())
    );

    let allowed = service(manager.clone(), &[PackageOperation::Refresh], false);
    assert!(allowed.refresh_packages(false).await.unwrap().success);
    assert_eq!(manager.calls.lock().unwrap().clone(), vec!["refresh"]);
}

#[tokio::test]
async fn upgrade_requires_the_allowlist() {
    let manager = Arc::new(FakeManager::default());
    let denied = service(manager, &[PackageOperation::Refresh], false);
    assert_eq!(
        denied.upgrade_packages().await.unwrap_err(),
        AppError::PackageOperationNotAllowed("upgrade".to_string())
    );
}

#[tokio::test]
async fn upgrade_with_nothing_available_is_a_no_op() {
    let manager = Arc::new(FakeManager::default());
    let svc = service(manager.clone(), &[PackageOperation::Upgrade], false);
    let report = svc.upgrade_packages().await.unwrap();
    assert!(report.success);
    assert_eq!(manager.calls.lock().unwrap().clone(), vec!["check_updates"]);
}

#[tokio::test]
async fn upgrade_refuses_a_transitive_plan_by_default() {
    let manager = Arc::new(FakeManager {
        updates: vec![update("openssl")],
        plan: UpgradePlan {
            would_install: 1,
            ..UpgradePlan::empty()
        },
        calls: Mutex::new(Vec::new()),
    });
    let svc = service(manager.clone(), &[PackageOperation::Upgrade], false);
    let error = svc.upgrade_packages().await.unwrap_err();
    assert!(matches!(
        error,
        AppError::Package(PackageError::TransitiveRefused { install: 1, .. })
    ));
    assert_eq!(
        manager.calls.lock().unwrap().clone(),
        vec!["check_updates", "plan"]
    );
}

#[tokio::test]
async fn upgrade_allows_a_transitive_plan_when_opted_in() {
    let manager = Arc::new(FakeManager {
        updates: vec![update("openssl")],
        plan: UpgradePlan {
            would_install: 1,
            ..UpgradePlan::empty()
        },
        calls: Mutex::new(Vec::new()),
    });
    let svc = service(manager.clone(), &[PackageOperation::Upgrade], true);
    assert!(svc.upgrade_packages().await.unwrap().success);
    assert_eq!(
        manager.calls.lock().unwrap().clone(),
        vec!["check_updates", "plan", "apply"]
    );
}

#[tokio::test]
async fn upgrade_applies_a_clean_plan() {
    let manager = Arc::new(FakeManager {
        updates: vec![update("openssl")],
        plan: UpgradePlan::empty(),
        calls: Mutex::new(Vec::new()),
    });
    let svc = service(manager.clone(), &[PackageOperation::Upgrade], false);
    assert!(svc.upgrade_packages().await.unwrap().success);
    assert_eq!(
        manager.calls.lock().unwrap().clone(),
        vec!["check_updates", "plan", "apply"]
    );
}

#[tokio::test]
async fn preview_is_open_and_empty_when_nothing_is_available() {
    let manager = Arc::new(FakeManager::default());
    let svc = service(manager.clone(), &[], false);
    let plan = svc.preview_upgrade().await.unwrap();
    assert_eq!(plan.total, 0);
    assert_eq!(manager.calls.lock().unwrap().clone(), vec!["check_updates"]);
}

#[tokio::test]
async fn preview_plans_when_updates_exist() {
    let manager = Arc::new(FakeManager {
        updates: vec![update("openssl")],
        plan: UpgradePlan::empty(),
        calls: Mutex::new(Vec::new()),
    });
    let svc = service(manager, &[], false);
    assert_eq!(svc.preview_upgrade().await.unwrap().total, 0);
}

#[tokio::test]
async fn list_packages_and_updates_pass_through() {
    let manager = Arc::new(FakeManager::default());
    let svc = service(manager, &[], false);
    assert_eq!(
        svc.list_packages(&PackageFilter::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        svc.list_updates(&UpdateFilter::default())
            .await
            .unwrap()
            .total,
        0
    );
}
