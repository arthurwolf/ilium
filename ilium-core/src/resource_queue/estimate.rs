use super::*;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceQueueEstimate {
    pub wait_seconds: Option<u64>,
    pub estimated_start_unix_millis: Option<i64>,
    pub reason: Option<String>,
    pub computed_at_unix_millis: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceQueueEntry {
    pub reservation: Reservation,
    pub position: usize,
    pub estimate: ResourceQueueEstimate,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceQueueSnapshot {
    pub resource_id: ResourceId,
    pub definition: ResourceDefinition,
    pub gate: ResourceGate,
    pub owner: Option<Reservation>,
    pub queue: Vec<ResourceQueueEntry>,
    pub observation_reason: Option<String>,
}
fn remaining(q: &Reservation, clock: &ResourceClock) -> Result<u64, String> {
    if q.state != ReservationState::Owned {
        return Err("Owner requires physical clearance".into());
    }
    let a = q.acquired_at.as_ref().ok_or("Acquisition clock missing")?;
    if a.boot_id != clock.boot_id {
        return Err("Ownership clock belongs to another boot".into());
    }
    let elapsed = clock
        .monotonic_millis
        .checked_sub(a.monotonic_millis)
        .ok_or("Monotonic clock regressed")?
        / 1000;
    if elapsed >= q.estimate_seconds {
        return Err("Owner is overdue; estimate is unknown".into());
    }
    Ok(q.estimate_seconds - elapsed)
}
pub fn project_resource_queue(r: &ResourceRecord, clock: &ResourceClock) -> ResourceQueueSnapshot {
    let owner = r.reservations.iter().find(|q| q.state.occupied()).cloned();
    let mut waiting: Vec<_> = r
        .reservations
        .iter()
        .filter(|q| q.state == ReservationState::Queued)
        .cloned()
        .collect();
    waiting.sort_by_key(|q| q.ticket);
    let mut cumulative = if r.clearance_required {
        Err("User must confirm physical device clearance".into())
    } else {
        match &r.gate {
            ResourceGate::Open => owner.as_ref().map_or(Ok(0), |q| remaining(q, clock)),
            ResourceGate::Paused { reason } | ResourceGate::Unavailable { reason } => {
                Err(reason.clone())
            }
        }
    };
    // A queued head without an owner is awaiting fresh eligibility, not instantly acquired.
    if owner.is_none() && !waiting.is_empty() && r.gate == ResourceGate::Open {
        cumulative = Err("Head invocation eligibility requires reconciliation".into())
    }
    let queue = waiting
        .into_iter()
        .enumerate()
        .map(|(i, q)| {
            let wait = cumulative.as_ref().ok().copied();
            let estimate = ResourceQueueEstimate {
                wait_seconds: wait,
                estimated_start_unix_millis: wait
                    .and_then(|s| i64::try_from(s).ok())
                    .and_then(|s| s.checked_mul(1000))
                    .and_then(|s| clock.unix_millis.checked_add(s)),
                reason: cumulative.as_ref().err().cloned(),
                computed_at_unix_millis: clock.unix_millis,
            };
            if let Ok(seconds) = &cumulative {
                cumulative = seconds
                    .checked_add(q.estimate_seconds)
                    .ok_or_else(|| "Estimate overflow".into());
            }
            ResourceQueueEntry {
                reservation: q,
                position: i + 1,
                estimate,
            }
        })
        .collect();
    let observation_reason = cumulative.err();
    ResourceQueueSnapshot {
        resource_id: r.definition.id.clone(),
        definition: r.definition.clone(),
        gate: r.gate.clone(),
        owner,
        queue,
        observation_reason,
    }
}
/// Strict additive duration parser shared by the CLI and forms.
pub fn parse_resource_duration(input: &str) -> Result<u64, ResourceDomainError> {
    let mut value = 0u64;
    let mut digits = 0usize;
    let mut total = 0u64;
    for byte in input.bytes() {
        if byte.is_ascii_digit() {
            value = value
                .checked_mul(10)
                .and_then(|n| n.checked_add(u64::from(byte - b'0')))
                .ok_or_else(|| error("invalid_duration", "duration overflow"))?;
            digits += 1;
        } else {
            let factor = match byte {
                b's' => 1,
                b'm' => 60,
                b'h' => 3600,
                b'd' => 86400,
                _ => return Err(error("invalid_duration", "unknown duration suffix")),
            };
            if digits == 0 {
                return Err(error("invalid_duration", "duration unit has no integer"));
            }
            total = total
                .checked_add(
                    value
                        .checked_mul(factor)
                        .ok_or_else(|| error("invalid_duration", "duration overflow"))?,
                )
                .ok_or_else(|| error("invalid_duration", "duration overflow"))?;
            value = 0;
            digits = 0;
        }
    }
    if digits > 0 || total == 0 || total > MAX_ESTIMATE_SECONDS {
        return Err(error(
            "invalid_duration",
            "use a duration from 1s to 7d with explicit units",
        ));
    }
    Ok(total)
}
