//! Log boundaries without wrapping calls or changing their results.
use luciddesk_diagnostics::{Level, log};
use std::{fmt::Display, panic::Location};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::pane) enum Operation {
    Conflict,
    DiscoverView,
    Controller,
    SubscribeDesktop,
    InitialInventory,
    ValidateView,
    Component,
    AttachHook,
    StartAudit,
    Reconcile,
    PublishMembership,
    PollMembership,
    SubmitAudit,
    PollAudit,
    ReadAudit,
    Repair,
    SaveWorkspace,
    SubscribeRecycle,
    NativeSnapshot,
    EnumerateSource,
    ValidateSnapshot,
    ValidateIdentities,
    ValidateHook,
}

impl Operation {
    pub fn id(self) -> &'static str {
        match self {
            Self::Conflict => "desktop.compatibility.check",
            Self::DiscoverView => "desktop.view.discover",
            Self::Controller => "desktop.controller.create",
            Self::SubscribeDesktop => "desktop.notifications.subscribe",
            Self::InitialInventory => "desktop.inventory.initial_capture",
            Self::ValidateView => "desktop.view.validate_identity",
            Self::Component => "desktop.component.prepare",
            Self::AttachHook => "desktop.hook.attach",
            Self::StartAudit => "desktop.audit.start_worker",
            Self::Reconcile => "desktop.inventory.reconcile",
            Self::PublishMembership => "desktop.membership.publish",
            Self::PollMembership => "desktop.membership.confirm",
            Self::SubmitAudit => "desktop.audit.submit",
            Self::PollAudit => "desktop.audit.poll_worker",
            Self::ReadAudit => "desktop.audit.read_result",
            Self::Repair => "desktop.membership.repair",
            Self::SaveWorkspace => "desktop.workspace.save",
            Self::SubscribeRecycle => "desktop.recycle.subscribe",
            Self::NativeSnapshot => "desktop.shell.native_snapshot",
            Self::EnumerateSource => "desktop.shell.enumerate_source",
            Self::ValidateSnapshot => "desktop.inventory.validate_snapshot",
            Self::ValidateIdentities => "desktop.inventory.validate_identities",
            Self::ValidateHook => "desktop.hook.validate_session",
        }
    }
    pub fn category(self) -> &'static str {
        match self {
            Self::Conflict => "compatibility",
            Self::DiscoverView | Self::ValidateView => "view-discovery",
            Self::Controller => "window-creation",
            Self::SubscribeDesktop | Self::SubscribeRecycle => "shell-notification",
            Self::Component => "component-deployment",
            Self::AttachHook
            | Self::PublishMembership
            | Self::PollMembership
            | Self::Repair
            | Self::ValidateHook => "hook-ipc",
            Self::StartAudit | Self::SubmitAudit | Self::PollAudit => "audit-worker",
            Self::SaveWorkspace => "persistence",
            Self::NativeSnapshot | Self::EnumerateSource => "shell-enumeration",
            Self::InitialInventory
            | Self::ReadAudit
            | Self::Reconcile
            | Self::ValidateSnapshot
            | Self::ValidateIdentities => "inventory",
        }
    }
}

#[track_caller]
pub(in crate::pane) fn failure(operation: Operation, cause: impl Display) {
    log(
        Level::Error,
        operation.id(),
        &format!(
            "category={} location={} cause={cause}",
            operation.category(),
            Location::caller()
        ),
    );
}

#[track_caller]
pub(in crate::pane) fn completed(operation: Operation, context: impl Display) {
    log(
        Level::Info,
        operation.id(),
        &format!(
            "outcome=ok category={} location={} {context}",
            operation.category(),
            Location::caller()
        ),
    );
}
