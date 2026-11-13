//! Plain-language vocabulary for the resource control panel: one place that
//! names every button, explains it in a tooltip, and decides when it is usable.
use super::tree::ordered;
use crate::app::App;
use ilium_core::resource_queue::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceAction {
    Help,
    Edit,
    Group,
    Refresh,
    Retry,
    PauseResume,
    Offline,
    CancelWaiting,
    Estimate,
    OwnCancel,
    OwnRelease,
    TakeBack,
    ConfirmSafe,
    Repair,
    Unlink,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionGroup {
    Resource,
    Queue,
    Recovery,
    Remove,
}

impl ActionGroup {
    pub const fn caption(self) -> &'static str {
        match self {
            Self::Resource => "Resource",
            Self::Queue => "Queue",
            Self::Recovery => "Recovery",
            Self::Remove => "Remove",
        }
    }
}

impl ResourceAction {
    pub const ALL: [Self; 16] = [
        Self::Help,
        Self::Edit,
        Self::Group,
        Self::Refresh,
        Self::Retry,
        Self::PauseResume,
        Self::Offline,
        Self::CancelWaiting,
        Self::Estimate,
        Self::OwnCancel,
        Self::OwnRelease,
        Self::TakeBack,
        Self::ConfirmSafe,
        Self::Repair,
        Self::Unlink,
        Self::Delete,
    ];

    /// Keyboard accelerator; the keys predate the buttons and still work.
    pub const fn key(self) -> char {
        match self {
            Self::Help => '?',
            Self::Edit => 'e',
            Self::Group => 'g',
            Self::Refresh => 'v',
            Self::Retry => 't',
            Self::PauseResume => 'p',
            Self::Offline => 'u',
            Self::CancelWaiting => 'x',
            Self::Estimate => 'd',
            Self::OwnCancel => 'z',
            Self::OwnRelease => 'o',
            Self::TakeBack => 'r',
            Self::ConfirmSafe => 'c',
            Self::Repair => 'i',
            Self::Unlink => 'l',
            Self::Delete => 'a',
        }
    }

    pub const fn group(self) -> ActionGroup {
        match self {
            Self::Help | Self::Edit | Self::Group | Self::Refresh | Self::Retry => {
                ActionGroup::Resource
            }
            Self::PauseResume
            | Self::Offline
            | Self::CancelWaiting
            | Self::Estimate
            | Self::OwnCancel
            | Self::OwnRelease => ActionGroup::Queue,
            Self::TakeBack | Self::ConfirmSafe | Self::Repair => ActionGroup::Recovery,
            Self::Unlink | Self::Delete => ActionGroup::Remove,
        }
    }

    pub fn label(self, record: &ResourceRecord) -> &'static str {
        match self {
            Self::Help => "? Help",
            Self::Edit => "Edit",
            Self::Group => "Group view",
            Self::Refresh => "Refresh",
            Self::Retry => "Retry",
            Self::PauseResume if matches!(record.gate, ResourceGate::Open) => "Pause queue",
            Self::PauseResume => "Resume queue",
            Self::Offline => "Mark offline",
            Self::CancelWaiting => "Cancel request",
            Self::Estimate => "My estimate",
            Self::OwnCancel => "Cancel mine",
            Self::OwnRelease => "Release mine",
            Self::TakeBack => "Take back",
            Self::ConfirmSafe => "Confirm safe",
            Self::Repair => "Repair setup",
            Self::Unlink => "Remove here",
            Self::Delete => "Delete",
        }
    }

    /// Title and explanation shown when the pointer rests on the button.
    pub fn tooltip(self, record: &ResourceRecord) -> (&'static str, &'static str) {
        match self {
            Self::Help => (
                "Help",
                "Explains every word and button on this panel in plain language.",
            ),
            Self::Edit => (
                "Edit this resource",
                "Change its name, description, rules, kind and hardware identity. Changes apply to every project that uses it.",
            ),
            Self::Group => (
                "Group view",
                "Shows or hides the agents that use a resource underneath it in the project tree.",
            ),
            Self::Refresh => (
                "Refresh",
                "Asks the server for the latest queue state right now.",
            ),
            Self::Retry => (
                "Retry",
                "Sends the last request again, exactly as before, after a connection problem.",
            ),
            Self::PauseResume if matches!(record.gate, ResourceGate::Open) => (
                "Pause the queue",
                "Stops giving the hardware to new agents. Whoever holds it keeps it. Waiting agents stay in line.",
            ),
            Self::PauseResume => (
                "Resume the queue",
                "Lets waiting agents be given the hardware again, in order.",
            ),
            Self::Offline => (
                "Mark hardware offline",
                "Use when the hardware is unplugged, broken or being serviced. Nobody is given it until you resume the queue.",
            ),
            Self::CancelWaiting => (
                "Cancel the selected request",
                "Removes the selected waiting agent from the line. Agents behind it move up.",
            ),
            Self::Estimate => (
                "Update my time estimate",
                "Tell the queue how long your agent really needs. Others see better wait times. It never releases the hardware.",
            ),
            Self::OwnCancel => (
                "Cancel my waiting request",
                "Your own selected agent leaves the line without using the hardware.",
            ),
            Self::OwnRelease => (
                "Release my hardware",
                "Your own selected agent gives the hardware back so the next one can start. Stop its workload first.",
            ),
            Self::TakeBack => (
                "Take back ownership",
                "Forcibly removes the current owner. Use it only when that agent is stuck or gone. The hardware then needs a safety check.",
            ),
            Self::ConfirmSafe => (
                "Confirm the hardware is safe",
                "After a take-back or a crash, say the hardware is idle and safe so the next agent may start. It does not stop any program.",
            ),
            Self::Repair => (
                "Repair project setup",
                "Re-writes the agent instructions and settings of projects whose copy is out of date.",
            ),
            Self::Unlink => (
                "Remove from this project",
                "This project stops using the resource. The resource itself stays for other projects.",
            ),
            Self::Delete => (
                "Delete this resource",
                "Removes it from this project and deletes it. It must be unused and not shared with other projects.",
            ),
        }
    }

    /// `Ok` when the button can act now, otherwise the plain reason it cannot.
    pub fn availability(self, app: &App, record: &ResourceRecord) -> Result<(), String> {
        let selected = app
            .resources
            .selected_reservation
            .as_ref()
            .and_then(|id| ordered(record).into_iter().find(|claim| &claim.id == id));
        let own = selected.filter(|claim| app.verified_resource_actor(claim).is_some());
        let no = |reason: &str| Err(reason.to_owned());
        match self {
            Self::Retry if app.resources.retry_request.is_none() => {
                no("Nothing to retry: the last request succeeded.")
            }
            Self::CancelWaiting
                if !selected
                    .is_some_and(|claim| matches!(claim.state, ReservationState::Queued)) =>
            {
                no("Select a waiting request in the queue table first.")
            }
            Self::Estimate if own.is_none() => no("Select an agent running in this Ilium session."),
            Self::OwnCancel
                if !own.is_some_and(|claim| matches!(claim.state, ReservationState::Queued)) =>
            {
                no("Select your own waiting agent first.")
            }
            Self::OwnRelease
                if !own.is_some_and(|claim| matches!(claim.state, ReservationState::Owned)) =>
            {
                no("Select your own agent that currently holds the hardware.")
            }
            Self::TakeBack if !record.reservations.iter().any(|c| c.state.occupied()) => {
                no("Nobody holds this resource right now.")
            }
            Self::ConfirmSafe if !record.clearance_required => no("No safety check is pending."),
            Self::Repair
                if !app.resources.catalog.as_ref().is_some_and(|catalog| {
                    catalog.links.iter().any(|link| {
                        link.resource_id == record.definition.id && link.sync_debt.is_some()
                    })
                }) =>
            {
                no("Every project's setup is up to date.")
            }
            _ => Ok(()),
        }
    }
}
