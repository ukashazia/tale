//! What `:profiles` knows about each configured admin profile.
//!
//! A profile is a named Control API credential, not a tailnet and not the local
//! client. This module keeps the two facts the page reports apart on purpose:
//! what the credential *store* says, which costs a file read and is known for
//! every profile at all times, and what the *control plane* says, which costs a
//! request and is therefore only ever learned about the profile being activated.

use super::Timestamp;
use crate::secrets::CredentialKind;

/// What the credential store holds for a profile. Read locally; never a request.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CredentialPresence {
    /// The store was readable and holds no record under this reference.
    Missing,
    Stored {
        kind: CredentialKind,
        scopes: Vec<String>,
    },
    /// The backend itself could not be read — wrong permissions, malformed file.
    /// Distinct from `Missing` because the remedy is different.
    Unreadable { detail: String },
}

impl CredentialPresence {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Stored { kind, .. } => kind.label(),
            Self::Unreadable { .. } => "unreadable",
        }
    }

    /// Whether activation is worth attempting. A profile with no readable
    /// credential cannot reach the control plane, so the page says so instead of
    /// spending a request to be told the same thing.
    pub const fn is_usable(&self) -> bool {
        matches!(self, Self::Stored { .. })
    }
}

/// What the control plane last said about a profile. `NotProbed` is the honest
/// state for every profile the user has not tried to activate: Tale does not
/// spend requests on credentials nobody asked for.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub enum ProbeState {
    #[default]
    NotProbed,
    InFlight,
    Reachable {
        kind: CredentialKind,
        at: Timestamp,
    },
    Rejected {
        detail: String,
        at: Timestamp,
    },
}

impl ProbeState {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::NotProbed => "unverified",
            Self::InFlight => "checking",
            Self::Reachable { .. } => "reachable",
            Self::Rejected { .. } => "rejected",
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Rejected { detail, .. } => Some(detail.as_str()),
            _ => None,
        }
    }

    pub const fn observed_at(&self) -> Option<Timestamp> {
        match self {
            Self::Reachable { at, .. } | Self::Rejected { at, .. } => Some(*at),
            Self::NotProbed | Self::InFlight => None,
        }
    }
}

/// Everything `:profiles` learned about one profile without being asked to
/// activate it, plus whatever the last activation attempt reported.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct ProfileStatus {
    pub presence: Option<CredentialPresence>,
    pub probe: ProbeState,
}

impl ProfileStatus {
    /// The one word the table shows. The store's answer wins while it is bad,
    /// because no control-plane verdict can be trusted over a missing secret.
    pub const fn label(&self) -> &'static str {
        match &self.presence {
            None => "reading",
            Some(
                presence @ (CredentialPresence::Missing | CredentialPresence::Unreadable { .. }),
            ) => presence.label(),
            Some(CredentialPresence::Stored { .. }) => self.probe.label(),
        }
    }

    pub fn is_usable(&self) -> bool {
        self.presence
            .as_ref()
            .is_some_and(CredentialPresence::is_usable)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ProfileSortField {
    Name,
    Tailnet,
    State,
    Access,
}

impl ProfileSortField {
    pub const ALL: [Self; 4] = [Self::Name, Self::Tailnet, Self::State, Self::Access];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Tailnet => "tailnet",
            Self::State => "state",
            Self::Access => "access",
        }
    }

    pub const fn key(self) -> char {
        match self {
            Self::Name => 'n',
            Self::Tailnet => 't',
            Self::State => 's',
            Self::Access => 'a',
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ProfileSortSpec {
    pub field: ProfileSortField,
    pub direction: super::device::SortDirection,
}

impl Default for ProfileSortSpec {
    fn default() -> Self {
        Self {
            field: ProfileSortField::Name,
            direction: super::device::SortDirection::Ascending,
        }
    }
}
