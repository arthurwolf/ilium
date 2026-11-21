//! Admission boundary for private cross-session polling evidence.
use ilium_core::polling_detection::PollingSettings;
use ilium_ipc::polling::PollingEvidenceSubscription;
use ilium_platform::{file_lock_owner, process_control, secure_fs};
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};

/// The peer ID must come from the accepted transport, never request payload.
pub struct FeedAdmission<'a> {
    pub lease_path: &'a Path,
    pub peer_process_id: Option<u32>,
    pub subscription: &'a PollingEvidenceSubscription,
    pub serving_incarnation: &'a str,
    pub settings: &'a PollingSettings,
}

pub struct AuthenticatedDetector {
    lease_path: PathBuf,
    // Keep the original inode alive so replacement cannot exploit inode reuse.
    _lease_file: std::fs::File,
    lease_generation: (u64, u64),
    epoch: String,
    peer_process_id: u32,
    peer_identity: process_control::PtyProcessIdentity,
}

impl AuthenticatedDetector {
    /// Called on an admitted I/O worker before releasing private evidence.
    pub fn admit(request: FeedAdmission<'_>) -> io::Result<Option<Self>> {
        if !request.settings.enabled
            || request.settings.validate().is_err()
            || request.serving_incarnation.is_empty()
            || request.serving_incarnation.len() > 256
            || request.subscription.expected_server_incarnation != request.serving_incarnation
            || request.subscription.leader_epoch.is_empty()
            || request.subscription.leader_epoch.len() > 256
        {
            return Ok(None);
        }
        let Some(peer_process_id) = request.peer_process_id.filter(|value| *value > 0) else {
            return Ok(None);
        };
        let peer_identity = match process_control::capture_pty_process(peer_process_id) {
            Ok(identity) => identity,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let lease_file = secure_fs::open_private_regular_file(
            request.lease_path,
            secure_fs::PrivateFileMode::Read,
        )?;
        let lease_generation = secure_fs::file_generation(&lease_file)?;
        let admitted = Self {
            lease_path: request.lease_path.to_owned(),
            _lease_file: lease_file,
            lease_generation,
            epoch: request.subscription.leader_epoch.clone(),
            peer_process_id,
            peer_identity,
        };
        Ok(admitted
            .still_authorized(request.settings)?
            .then_some(admitted))
    }

    /// Revalidate both the actual kernel owner and this lease lifetime, not
    /// merely a live process. An old subscription cannot follow reacquisition.
    pub fn still_authorized(&self, settings: &PollingSettings) -> io::Result<bool> {
        if !settings.enabled || settings.validate().is_err() {
            return Ok(false);
        }
        let before = read_epoch(&self.lease_path)?;
        if before.0 != self.lease_generation
            || before.1 != self.epoch
            || !file_lock_owner::exclusive_lock_owned_by(
                &self.lease_path,
                self.peer_process_id,
                &self.peer_identity,
            )?
        {
            return Ok(false);
        }
        Ok(read_epoch(&self.lease_path)? == before)
    }
}

fn read_epoch(path: &Path) -> io::Result<((u64, u64), String)> {
    let file = secure_fs::open_private_regular_file(path, secure_fs::PrivateFileMode::Read)?;
    let generation = secure_fs::file_generation(&file)?;
    let mut epoch = String::new();
    file.take(257).read_to_string(&mut epoch)?;
    if epoch.is_empty() || epoch.len() > 256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid detector lease epoch",
        ));
    }
    Ok((generation, epoch))
}

/// Native serving-peer proof, distinct from a detector's authority to receive
/// evidence. The peer process must be supplied by the connected transport.
pub struct AuthenticatedSessionServer {
    // Observation must not own the publisher's lock. This separately opened
    // handle only prevents inode reuse while the proof exists.
    _record_file: std::fs::File,
    record_path: PathBuf,
    generation: (u64, u64),
    socket_path: PathBuf,
    peer_process_id: u32,
    peer_identity: process_control::PtyProcessIdentity,
}

impl AuthenticatedSessionServer {
    /// Run on an admitted native I/O worker after connecting the endpoint.
    pub fn admit(
        registry_directory: &Path,
        hint: &crate::polling_discovery::SessionEndpointHint,
        peer_process_id: Option<u32>,
    ) -> io::Result<Option<Self>> {
        let Some(peer_process_id) = peer_process_id.filter(|value| *value > 0) else {
            return Ok(None);
        };
        if hint.incarnation.is_nil()
            || !registry_directory.is_absolute()
            || !hint.socket_path.is_absolute()
            || hint.socket_path.as_os_str().len() > 4096
        {
            return Ok(None);
        }
        let peer_identity = match process_control::capture_pty_process(peer_process_id) {
            Ok(identity) => identity,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let record_path = registry_directory.join(format!("server-{}.json", hint.incarnation));
        let Some((file, generation, socket_path)) = read_publication(&record_path)? else {
            return Ok(None);
        };
        if socket_path != hint.socket_path {
            return Ok(None);
        }
        let proof = Self {
            _record_file: file,
            record_path,
            generation,
            socket_path,
            peer_process_id,
            peer_identity,
        };
        Ok(proof.still_authorized()?.then_some(proof))
    }

    /// Revalidate the original serving process and publication lifetime before
    /// consuming evidence. Unsupported or incomplete native proof fails closed.
    pub fn still_authorized(&self) -> io::Result<bool> {
        let Some((_, generation, socket_path)) = read_publication(&self.record_path)? else {
            return Ok(false);
        };
        if generation != self.generation
            || socket_path != self.socket_path
            || !file_lock_owner::exclusive_lock_owned_by(
                &self.record_path,
                self.peer_process_id,
                &self.peer_identity,
            )?
        {
            return Ok(false);
        }
        Ok(
            read_publication(&self.record_path)?.is_some_and(|(_, generation, socket_path)| {
                generation == self.generation && socket_path == self.socket_path
            }),
        )
    }
}

type Publication = (std::fs::File, (u64, u64), PathBuf);

fn read_publication(path: &Path) -> io::Result<Option<Publication>> {
    let file = match secure_fs::open_private_regular_file(path, secure_fs::PrivateFileMode::Read) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let generation = secure_fs::file_generation(&file)?;
    let mut bytes = Vec::new();
    (&file).take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "polling publication exceeds 4096 bytes",
        ));
    }
    let socket_path: PathBuf = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if !socket_path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "polling publication endpoint is relative",
        ));
    }
    Ok(Some((file, generation, socket_path)))
}
