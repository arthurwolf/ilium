use super::*;
use std::collections::BTreeSet;
fn validate_id(value: &str) -> Result<(), ResourceDomainError> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        Err(error(
            "invalid_id",
            "identity must contain 1..256 printable bytes",
        ))
    } else {
        Ok(())
    }
}
fn bounded(value: &str, max: usize, nonempty: bool) -> Result<(), ResourceDomainError> {
    if value.len() > max || (nonempty && value.trim().is_empty()) || value.contains('\0') {
        Err(error(
            "invalid_input",
            "text is empty, too long or contains NUL",
        ))
    } else {
        Ok(())
    }
}
fn validate_definition(d: &ResourceDefinition) -> Result<(), ResourceDomainError> {
    validate_id(&d.id.0)?;
    bounded(&d.title, 480, true)?;
    if d.title.trim().chars().count() > 120 {
        return Err(error("invalid_input", "title exceeds 120 characters"));
    }
    bounded(&d.description, 8192, false)?;
    bounded(&d.rules, 16384, false)?;
    bounded(&d.kind, 64, true)?;
    if let Some(p) = &d.physical_identity {
        bounded(p, 1024, true)?;
        if !p.contains(':') {
            return Err(error(
                "physical_identity",
                "physical identity must have a typed namespace",
            ));
        }
    }
    Ok(())
}
fn normalized(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}
fn validate_link(l: &ProjectResourceLink) -> Result<(), ResourceDomainError> {
    validate_id(&l.project_id.0)?;
    validate_id(&l.resource_id.0)?;
    if l.slug.is_empty()
        || l.slug.len() > 64
        || l.slug.starts_with('-')
        || l.slug.ends_with('-')
        || l.slug.contains("--")
        || !l
            .slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(error(
            "invalid_slug",
            "slug must match [a-z0-9]+(?:-[a-z0-9]+)* in 1..64 ASCII bytes",
        ));
    }
    bounded(&l.project_path, 4096, true)
}
fn validate_agent(a: &AgentIdentity) -> Result<(), ResourceDomainError> {
    if a.pane_id.0 == 0 {
        return Err(error("invalid_agent", "root is not an agent pane"));
    }
    validate_id(&a.project_id.0)?;
    validate_id(&a.invocation_id.0)?;
    validate_id(&a.session_id)?;
    validate_id(&a.runtime_generation)?;
    bounded(&a.label, 480, true)
}
/// Validate untrusted decoded catalogs before allowing any mutation.
pub fn validate_resource_catalog(c: &ResourceCatalog) -> Result<(), ResourceDomainError> {
    if c.schema_version != 1 {
        return Err(error(
            "unsupported_schema",
            "resource catalog schema is not supported",
        ));
    }
    if c.resources
        .values()
        .filter(|r| !r.definition.archived)
        .count()
        > 1024
        || c.links.len() > 262144
        || c.receipts.len() > 100000
    {
        return Err(error("quota", "catalog capacity exceeded"));
    }
    history::validate_history(c)?;
    let mut links = BTreeSet::new();
    let mut pairs = BTreeSet::new();
    let mut physical = BTreeSet::new();
    let mut reservations = BTreeSet::new();
    let mut invocations = Vec::new();
    let mut total = 0;
    let mut terminal = 0;
    for l in &c.links {
        validate_link(l)?;
        if l.created_at_unix_millis < 0 {
            return Err(error(
                "invalid_timestamp",
                "resource link creation timestamp must be nonnegative",
            ));
        }
        if !c.resources.contains_key(&l.resource_id)
            || !links.insert((l.project_id.clone(), l.slug.clone()))
            || !pairs.insert((l.project_id.clone(), l.resource_id.clone()))
        {
            return Err(error(
                "corrupt_catalog",
                "duplicate or dangling project link",
            ));
        }
    }
    for (id, r) in &c.resources {
        validate_definition(&r.definition)?;
        if r.definition.created_at_unix_millis < 0
            || r.definition.updated_at_unix_millis < r.definition.created_at_unix_millis
        {
            return Err(error(
                "invalid_timestamp",
                "resource creation/update timestamps are negative or unordered",
            ));
        }
        match r.definition.policy {
            ResourcePolicy::ExclusiveFifo => {}
        }
        if r.definition.definition_revision == 0 {
            return Err(error(
                "corrupt_catalog",
                "definition revision must be positive",
            ));
        }
        if r.clearance_required && r.gate == ResourceGate::Open {
            return Err(error(
                "corrupt_catalog",
                "clearance-required resource cannot have an open gate",
            ));
        }
        if id != &r.definition.id {
            return Err(error(
                "corrupt_catalog",
                "resource key differs from identity",
            ));
        }
        if let Some(p) = &r.definition.physical_identity {
            if !r.definition.archived && !physical.insert(normalized(p)) {
                return Err(error(
                    "duplicate_physical_identity",
                    "device already has an authoritative resource",
                ));
            }
        }
        if c.links.iter().filter(|l| &l.resource_id == id).count() > 256 {
            return Err(error("quota", "resource project link capacity exceeded"));
        }
        if r.next_ticket == 0 || r.next_generation == 0 {
            return Err(error(
                "corrupt_catalog",
                "ticket and generation counters must be positive",
            ));
        }
        let mut occupied = 0;
        let mut tickets = BTreeSet::new();
        let mut generations = BTreeSet::new();
        let mut active = 0;
        for q in &r.reservations {
            if !q.state.active() {
                terminal += 1;
            }
            validate_agent(&q.agent)?;
            validate_id(&q.id.0)?;
            if q.requested_at.boot_id.is_empty()
                || q.ticket == 0
                || &q.resource_id != id
                || !reservations.insert(q.id.clone())
                || !tickets.insert(q.ticket)
                || q.ticket >= r.next_ticket
                || q.ownership_generation >= r.next_generation
                || q.estimate_seconds == 0
                || q.estimate_seconds > MAX_ESTIMATE_SECONDS
            {
                return Err(error(
                    "corrupt_catalog",
                    "invalid reservation identity, ticket, generation or estimate",
                ));
            }
            if q.ownership_generation > 0 && !generations.insert(q.ownership_generation) {
                return Err(error("corrupt_catalog", "duplicate ownership generation"));
            }
            if q.state == ReservationState::Queued
                && (q.acquired_at.is_some() || q.ownership_generation != 0)
            {
                return Err(error(
                    "corrupt_catalog",
                    "queued reservation contains an ownership grant",
                ));
            }
            if q.state.active() && q.terminal_at.is_some() {
                return Err(error(
                    "corrupt_catalog",
                    "active reservation contains a terminal timestamp",
                ));
            }
            if !q.state.active() && q.terminal_at.is_none() {
                return Err(error(
                    "corrupt_catalog",
                    "terminal reservation is missing timestamp",
                ));
            }
            if q.state.occupied() {
                occupied += 1;
                if q.acquired_at.is_none() || q.ownership_generation == 0 {
                    return Err(error(
                        "corrupt_catalog",
                        "occupied reservation has no grant",
                    ));
                }
            }
            if q.state.active() {
                active += 1;
                total += 1;
                if invocations.iter().any(|a: &&AgentIdentity| {
                    a.same_invocation(&q.agent) || a.invocation_id == q.agent.invocation_id
                }) {
                    return Err(error(
                        "corrupt_catalog",
                        "invocation has multiple active reservations",
                    ));
                }
                invocations.push(&q.agent);
                if !c
                    .links
                    .iter()
                    .any(|l| l.project_id == q.agent.project_id && l.resource_id == *id)
                {
                    return Err(error(
                        "corrupt_catalog",
                        "active reservation has no project link",
                    ));
                }
            }
        }
        if r.definition.archived && (active > 0 || c.links.iter().any(|l| l.resource_id == *id)) {
            return Err(error(
                "corrupt_catalog",
                "archived resource retains links or active claims",
            ));
        }
        if occupied > 1 || active > 1024 {
            return Err(error(
                "corrupt_catalog",
                "multiple owners or queue capacity exceeded",
            ));
        }
    }
    if total > 0 && c.boot_id.is_empty() {
        return Err(error(
            "corrupt_catalog",
            "active claims require a boot identity",
        ));
    }
    for (id, receipt) in &c.receipts {
        request_created_at(id)?;
        if receipt.result.revision > c.revision {
            return Err(error(
                "corrupt_catalog",
                "receipt revision exceeds catalog revision",
            ));
        }
    }
    if terminal > 100000 {
        return Err(error("quota", "terminal reservation capacity exceeded"));
    }
    if total > 8192 {
        return Err(error("quota", "global reservation capacity exceeded"));
    }
    Ok(())
}
/// Accept UUIDv7 or the explicit millisecond:nonce request form. Both carry an admission time.
pub fn request_created_at(id: &RequestId) -> Result<i64, ResourceDomainError> {
    if let Some((time, nonce)) = id.0.split_once(':') {
        if !nonce.is_empty()
            && nonce.len() <= 128
            && nonce
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            let timestamp: i64 = time
                .parse()
                .map_err(|_| error("invalid_request_id", "invalid request timestamp"))?;
            if timestamp >= 0 {
                return Ok(timestamp);
            }
        }
    }
    let hex: String = id.0.chars().filter(|c| *c != '-').collect();
    if id.0.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|i| id.0.as_bytes()[i] == b'-')
        && hex.len() == 32
        && hex.as_bytes()[12] == b'7'
        && matches!(hex.as_bytes()[16], b'8' | b'9' | b'a' | b'b' | b'A' | b'B')
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return i64::from_str_radix(&hex[..12], 16)
            .map_err(|_| error("invalid_request_id", "invalid UUIDv7 time"));
    }
    Err(error(
        "invalid_request_id",
        "request ID must be time-bearing UUIDv7 or milliseconds:nonce",
    ))
}
fn same_actor(a: &ResourceActor, b: &ResourceActor) -> bool {
    match (a, b) {
        (ResourceActor::Agent(a), ResourceActor::Agent(b)) => a.same_invocation(b),
        (ResourceActor::User, ResourceActor::User)
        | (ResourceActor::System, ResourceActor::System) => true,
        _ => false,
    }
}
fn require_user(ctx: &MutationContext) -> Result<(), ResourceDomainError> {
    if matches!(ctx.actor, ResourceActor::User) {
        Ok(())
    } else {
        Err(error(
            "user_required",
            "this action requires a user interaction",
        ))
    }
}
fn reason(value: &str) -> Result<(), ResourceDomainError> {
    bounded(value, 2048, true)?;
    if value.chars().count() > 512 {
        Err(error("invalid_reason", "reason exceeds 512 characters"))
    } else {
        Ok(())
    }
}
fn own(q: &Reservation, ctx: &MutationContext) -> Result<(), ResourceDomainError> {
    match &ctx.actor {
        ResourceActor::Agent(a) if a.same_invocation(&q.agent) => Ok(()),
        _ => Err(error(
            "stale_invocation",
            "only the current owning invocation may mutate this reservation",
        )),
    }
}
fn estimate(seconds: u64) -> Result<(), ResourceDomainError> {
    if (1..=MAX_ESTIMATE_SECONDS).contains(&seconds) {
        Ok(())
    } else {
        Err(error(
            "invalid_duration",
            "estimate must be 1..604800 seconds",
        ))
    }
}
fn find_mut<'a>(
    c: &'a mut ResourceCatalog,
    id: &ReservationId,
) -> Result<(&'a mut ResourceRecord, usize), ResourceDomainError> {
    for r in c.resources.values_mut() {
        if let Some(i) = r.reservations.iter().position(|q| &q.id == id) {
            return Ok((r, i));
        }
    }
    Err(error("not_found", "reservation does not exist"))
}
fn record_mut<'a>(
    c: &'a mut ResourceCatalog,
    id: &ResourceId,
) -> Result<&'a mut ResourceRecord, ResourceDomainError> {
    c.resources
        .get_mut(id)
        .ok_or_else(|| error("not_found", "resource does not exist"))
}
fn promote(r: &mut ResourceRecord, ctx: &MutationContext) -> Result<(), ResourceDomainError> {
    if r.clearance_required
        || r.gate != ResourceGate::Open
        || r.reservations.iter().any(|q| q.state.occupied())
    {
        return Ok(());
    }
    loop {
        let Some(index) = r
            .reservations
            .iter()
            .enumerate()
            .filter(|(_, q)| q.state == ReservationState::Queued)
            .min_by_key(|(_, q)| q.ticket)
            .map(|(i, _)| i)
        else {
            return Ok(());
        };
        let q = &mut r.reservations[index];
        match ctx.eligibility.get(&q.agent.invocation_id) {
            Some(InvocationEligibility::Dead) => {
                q.state = ReservationState::Cancelled;
                q.terminal_at = Some(ctx.clock.unix_millis);
            }
            Some(InvocationEligibility::Live) => {
                q.state = ReservationState::Owned;
                q.acquired_at = Some(ctx.clock.clone());
                q.ownership_generation = r.next_generation;
                r.next_generation = r
                    .next_generation
                    .checked_add(1)
                    .ok_or_else(|| error("overflow", "ownership generation exhausted"))?;
                q.acquisition_pending = true;
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
}
pub fn apply_resource_command(
    catalog: &ResourceCatalog,
    command: ResourceCommand,
    ctx: &MutationContext,
) -> Result<ResourceTransition, ResourceDomainError> {
    validate_resource_catalog(catalog)?;
    validate_id(&ctx.clock.boot_id)?;
    if ctx.clock.unix_millis < 0 {
        return Err(error("invalid_clock", "wall clock precedes Unix epoch"));
    }
    let created = request_created_at(&ctx.request_id)?;
    let age = ctx
        .clock
        .unix_millis
        .checked_sub(created)
        .ok_or_else(|| error("invalid_request_id", "timestamp overflow"))?;
    if age > REQUEST_WINDOW_MILLIS {
        return Err(error(
            "idempotency_expired",
            "request is older than seven days",
        ));
    }
    if age < -300000 {
        return Err(error(
            "invalid_request_id",
            "request exceeds five minute future skew",
        ));
    }
    if let Some(receipt) = catalog.receipts.get(&ctx.request_id) {
        if receipt.command != command || !same_actor(&receipt.actor, &ctx.actor) {
            return Err(error(
                "idempotency_conflict",
                "request ID was used with a different mutation or actor",
            ));
        }
        let mut result = receipt.result.clone();
        result.reused = true;
        return Ok(ResourceTransition {
            catalog: catalog.clone(),
            result,
            events: Vec::new(),
        });
    }
    if ctx.expected_revision.is_some_and(|r| r != catalog.revision) {
        return Err(error(
            "revision_conflict",
            "catalog changed; reload before confirmation",
        ));
    }
    let mut c = catalog.clone();
    let mut resource_id = None;
    let mut reservation_id = None;
    let mut reused = false;
    if !c.boot_id.is_empty()
        && c.boot_id != ctx.clock.boot_id
        && !matches!(command, ResourceCommand::ReconcileBoot)
    {
        return Err(error(
            "boot_reconciliation_required",
            "persist boot fencing before accepting commands",
        ));
    }
    if c.boot_id.is_empty() {
        c.boot_id = ctx.clock.boot_id.clone()
    }
    match &command {
        ResourceCommand::RebindProject {
            project_id,
            project_path,
        } => {
            require_user(ctx)?;
            validate_id(&project_id.0)?;
            bounded(project_path, 4096, true)?;
            if ctx.expected_revision.is_none() {
                return Err(error(
                    "revision_required",
                    "project rebind requires current catalog revision",
                ));
            }
            if !c.links.iter().any(|link| link.project_id == *project_id) {
                return Err(error("not_found", "project has no resource bindings"));
            }
            if c.links
                .iter()
                .any(|link| link.project_path == *project_path && link.project_id != *project_id)
            {
                return Err(error(
                    "project_path_conflict",
                    "path belongs to another persistent project identity",
                ));
            }
            for link in c
                .links
                .iter_mut()
                .filter(|link| link.project_id == *project_id)
            {
                link.project_path = project_path.clone();
                link.sync_debt = Some(
                    "Project moved; resource settings and instructions need synchronization".into(),
                );
            }
        }
        ResourceCommand::Create { definition, link } => {
            require_user(ctx)?;
            validate_definition(definition)?;
            validate_link(link)?;
            if c.resources.contains_key(&definition.id) || definition.id != link.resource_id {
                return Err(error(
                    "conflict",
                    "resource identity already exists or link mismatches",
                ));
            }
            let mut definition = definition.clone();
            definition.definition_revision = 1;
            definition.policy = ResourcePolicy::ExclusiveFifo;
            definition.created_at_unix_millis = ctx.clock.unix_millis;
            definition.updated_at_unix_millis = ctx.clock.unix_millis;
            definition.archived = false;
            resource_id = Some(definition.id.clone());
            c.resources.insert(
                definition.id.clone(),
                ResourceRecord {
                    definition,
                    gate: ResourceGate::Open,
                    clearance_required: false,
                    next_ticket: 1,
                    next_generation: 1,
                    reservations: Vec::new(),
                },
            );
            let mut link = link.clone();
            link.created_at_unix_millis = ctx.clock.unix_millis;
            link.desired_instruction_revision = 1;
            c.links.push(link);
        }
        ResourceCommand::Link { link } => {
            require_user(ctx)?;
            validate_link(link)?;
            let r = record_mut(&mut c, &link.resource_id)?;
            if r.definition.archived {
                return Err(error("archived", "cannot link archived resource"));
            }
            let mut link = link.clone();
            link.created_at_unix_millis = ctx.clock.unix_millis;
            link.desired_instruction_revision = r.definition.definition_revision;
            resource_id = Some(link.resource_id.clone());
            if let Some(existing) = c
                .links
                .iter()
                .find(|l| l.project_id == link.project_id && l.resource_id == link.resource_id)
            {
                if existing.slug != link.slug {
                    return Err(error(
                        "link_conflict",
                        "resource is already linked with another slug",
                    ));
                }
                reused = true;
            } else {
                c.links.push(link);
            }
        }
        ResourceCommand::Edit { definition } => {
            require_user(ctx)?;
            validate_definition(definition)?;
            let r = record_mut(&mut c, &definition.id)?;
            let revision = r
                .definition
                .definition_revision
                .checked_add(1)
                .ok_or_else(|| error("overflow", "definition revision exhausted"))?;
            if definition.archived != r.definition.archived {
                return Err(error(
                    "invalid_input",
                    "archive uses its dedicated guarded command",
                ));
            }
            let created_at = r.definition.created_at_unix_millis;
            let updated_at = r
                .definition
                .updated_at_unix_millis
                .max(ctx.clock.unix_millis);
            r.definition = definition.clone();
            r.definition.policy = ResourcePolicy::ExclusiveFifo;
            r.definition.created_at_unix_millis = created_at;
            r.definition.updated_at_unix_millis = updated_at;
            r.definition.definition_revision = revision;
            resource_id = Some(definition.id.clone());
            for l in &mut c.links {
                if l.resource_id == definition.id {
                    l.desired_instruction_revision = revision
                }
            }
        }
        ResourceCommand::Unlink {
            project_id,
            resource_id: id,
        } => {
            require_user(ctx)?;
            let r = record_mut(&mut c, id)?;
            if r.reservations
                .iter()
                .any(|q| q.agent.project_id == *project_id && q.state.active())
            {
                return Err(error(
                    "busy",
                    "project still has active resource reservations",
                ));
            }
            let n = c.links.len();
            c.links
                .retain(|l| l.project_id != *project_id || l.resource_id != *id);
            if n == c.links.len() {
                return Err(error("not_found", "project link does not exist"));
            }
            resource_id = Some(id.clone());
        }
        ResourceCommand::Archive { resource_id: id } => {
            require_user(ctx)?;
            if c.links.iter().any(|l| l.resource_id == *id) {
                return Err(error("busy", "unlink all projects before archive"));
            }
            let r = record_mut(&mut c, id)?;
            if r.reservations.iter().any(|q| q.state.active()) {
                return Err(error("busy", "active reservations prevent archive"));
            }
            r.definition.archived = true;
            r.definition.updated_at_unix_millis = r
                .definition
                .updated_at_unix_millis
                .max(ctx.clock.unix_millis);
            resource_id = Some(id.clone());
        }
        ResourceCommand::Reserve {
            resource_id: id,
            reservation_id: rid,
            estimate_seconds,
        } => {
            estimate(*estimate_seconds)?;
            validate_id(&rid.0)?;
            let ResourceActor::Agent(agent) = &ctx.actor else {
                return Err(error(
                    "agent_required",
                    "reserve needs a verified live agent invocation",
                ));
            };
            validate_agent(agent)?;
            if ctx.eligibility.get(&agent.invocation_id) != Some(&InvocationEligibility::Live) {
                return Err(error(
                    "invocation_unknown",
                    "reserve requires fresh live invocation evidence",
                ));
            }
            if !c
                .links
                .iter()
                .any(|l| l.resource_id == *id && l.project_id == agent.project_id)
            {
                return Err(error(
                    "not_linked",
                    "resource is not linked to invoking project",
                ));
            }
            if let Some(existing) = c
                .resources
                .values()
                .flat_map(|r| &r.reservations)
                .find(|q| {
                    q.state.active()
                        && (q.agent.same_invocation(agent)
                            || q.agent.invocation_id == agent.invocation_id)
                })
            {
                if !existing.agent.same_invocation(agent) {
                    return Err(error(
                        "stale_invocation",
                        "invocation identity is bound to another pane runtime",
                    ));
                }
                if existing.resource_id != *id {
                    return Err(error(
                        "active_claim",
                        "invocation already has an active reservation on another resource",
                    ));
                }
                reservation_id = Some(existing.id.clone());
                reused = true;
            } else {
                if c.resources
                    .values()
                    .flat_map(|r| &r.reservations)
                    .any(|q| q.id == *rid)
                {
                    return Err(error("conflict", "reservation identity already exists"));
                }
                let r = record_mut(&mut c, id)?;
                if r.definition.archived {
                    return Err(error("archived", "cannot reserve archived resource"));
                }
                let ticket = r.next_ticket;
                r.next_ticket = r
                    .next_ticket
                    .checked_add(1)
                    .ok_or_else(|| error("overflow", "queue ticket exhausted"))?;
                r.reservations.push(Reservation {
                    id: rid.clone(),
                    request_id: ctx.request_id.clone(),
                    resource_id: id.clone(),
                    agent: agent.clone(),
                    ticket,
                    estimate_seconds: *estimate_seconds,
                    requested_at: ctx.clock.clone(),
                    acquired_at: None,
                    terminal_at: None,
                    state: ReservationState::Queued,
                    ownership_generation: 0,
                    acquisition_pending: false,
                });
                promote(r, ctx)?;
                reservation_id = Some(rid.clone())
            }
            resource_id = Some(id.clone());
        }
        ResourceCommand::Release {
            reservation_id: id,
            ownership_generation,
        } => {
            let (r, i) = find_mut(&mut c, id)?;
            own(&r.reservations[i], ctx)?;
            let q = &mut r.reservations[i];
            if q.ownership_generation != *ownership_generation || !q.state.occupied() {
                return Err(error(
                    "stale_generation",
                    "release does not name a current occupied generation",
                ));
            }
            q.state = ReservationState::Released;
            q.terminal_at = Some(ctx.clock.unix_millis);
            q.acquisition_pending = false;
            resource_id = Some(r.definition.id.clone());
            reservation_id = Some(id.clone());
            promote(r, ctx)?;
        }
        ResourceCommand::Cancel { reservation_id: id } => {
            let (r, i) = find_mut(&mut c, id)?;
            own(&r.reservations[i], ctx)?;
            let q = &mut r.reservations[i];
            if q.state.occupied() {
                return Err(error(
                    "already_owned",
                    "owned resource requires explicit release",
                ));
            }
            if q.state == ReservationState::Queued {
                q.state = ReservationState::Cancelled;
                q.terminal_at = Some(ctx.clock.unix_millis)
            }
            resource_id = Some(r.definition.id.clone());
            reservation_id = Some(id.clone());
            promote(r, ctx)?;
        }
        ResourceCommand::CancelByUser {
            reservation_id: id,
            reason: why,
        } => {
            require_user(ctx)?;
            reason(why)?;
            if ctx.expected_revision.is_none() {
                return Err(error(
                    "revision_required",
                    "cancelling another invocation needs a fresh revision",
                ));
            }
            let (r, i) = find_mut(&mut c, id)?;
            let q = &mut r.reservations[i];
            if q.state.occupied() {
                return Err(error(
                    "already_owned",
                    "owned resource requires explicit recovery",
                ));
            }
            if q.state == ReservationState::Queued {
                q.state = ReservationState::Cancelled;
                q.terminal_at = Some(ctx.clock.unix_millis)
            }
            resource_id = Some(r.definition.id.clone());
            reservation_id = Some(id.clone());
            promote(r, ctx)?;
        }
        ResourceCommand::Estimate {
            reservation_id: id,
            estimate_seconds,
        } => {
            estimate(*estimate_seconds)?;
            let (r, i) = find_mut(&mut c, id)?;
            own(&r.reservations[i], ctx)?;
            if !r.reservations[i].state.active() {
                return Err(error("terminal", "reservation is no longer active"));
            }
            r.reservations[i].estimate_seconds = *estimate_seconds;
            resource_id = Some(r.definition.id.clone());
            reservation_id = Some(id.clone());
        }
        ResourceCommand::Acknowledge {
            reservation_id: id,
            ownership_generation,
        } => {
            let (r, i) = find_mut(&mut c, id)?;
            own(&r.reservations[i], ctx)?;
            let q = &mut r.reservations[i];
            if q.state != ReservationState::Owned || q.ownership_generation != *ownership_generation
            {
                return Err(error(
                    "stale_generation",
                    "acquisition receipt no longer owns resource",
                ));
            }
            q.acquisition_pending = false;
            resource_id = Some(r.definition.id.clone());
            reservation_id = Some(id.clone());
        }
        ResourceCommand::SetGate {
            resource_id: id,
            gate,
        } => {
            require_user(ctx)?;
            match gate {
                ResourceGate::Paused { reason: r } | ResourceGate::Unavailable { reason: r } => {
                    reason(r)?
                }
                ResourceGate::Open => {}
            }
            let r = record_mut(&mut c, id)?;
            if *gate == ResourceGate::Open && r.clearance_required {
                return Err(error(
                    "clearance_required",
                    "confirm physical device clearance before reopening resource",
                ));
            }
            r.gate = gate.clone();
            promote(r, ctx)?;
            resource_id = Some(id.clone());
        }
        ResourceCommand::Revoke {
            reservation_id: id,
            reason: why,
        } => {
            require_user(ctx)?;
            reason(why)?;
            if ctx.expected_revision.is_none() {
                return Err(error(
                    "revision_required",
                    "recovery needs a fresh expected revision",
                ));
            }
            let (r, i) = find_mut(&mut c, id)?;
            if !r.reservations[i].state.occupied() {
                return Err(error("not_owned", "only an occupied claim can be revoked"));
            }
            r.reservations[i].state = ReservationState::Revoked {
                reason: why.clone(),
            };
            r.reservations[i].terminal_at = Some(ctx.clock.unix_millis);
            r.reservations[i].acquisition_pending = false;
            r.clearance_required = true;
            r.gate = ResourceGate::Paused {
                reason: format!("Device clearance required: {why}"),
            };
            resource_id = Some(r.definition.id.clone());
            reservation_id = Some(id.clone());
        }
        ResourceCommand::ConfirmClear {
            resource_id: id,
            reason: why,
        } => {
            require_user(ctx)?;
            reason(why)?;
            if ctx.expected_revision.is_none() {
                return Err(error(
                    "revision_required",
                    "clearance requires current catalog revision",
                ));
            }
            let r = record_mut(&mut c, id)?;
            if r.reservations
                .iter()
                .any(|q| q.state == ReservationState::Owned)
            {
                return Err(error(
                    "busy",
                    "revoke live owner before confirming device clear",
                ));
            }
            for q in &mut r.reservations {
                if matches!(q.state, ReservationState::NeedsAttention { .. }) {
                    q.state = ReservationState::Revoked {
                        reason: why.clone(),
                    };
                    q.terminal_at = Some(ctx.clock.unix_millis);
                    q.acquisition_pending = false
                }
            }
            r.clearance_required = false;
            r.gate = ResourceGate::Open;
            promote(r, ctx)?;
            resource_id = Some(id.clone());
        }
        ResourceCommand::RecoverStaleOwner {
            resource_id: id,
            reason: why,
        } => {
            // Agents may clear only claims the server already proved ownerless
            // (boot change or dead invocation both produce NeedsAttention);
            // a live Owned claim still needs the user's guarded revoke.
            if matches!(ctx.actor, ResourceActor::System) {
                return Err(error(
                    "agent_or_user_required",
                    "stale-owner recovery is requested by an agent or the user",
                ));
            }
            reason(why)?;
            if ctx.expected_revision.is_none() {
                return Err(error(
                    "revision_required",
                    "recovery needs a fresh expected revision",
                ));
            }
            let r = record_mut(&mut c, id)?;
            if r.reservations
                .iter()
                .any(|q| q.state == ReservationState::Owned)
            {
                return Err(error(
                    "owner_live",
                    "the owner is not proven gone; a user must revoke a live owner",
                ));
            }
            if !r
                .reservations
                .iter()
                .any(|q| matches!(q.state, ReservationState::NeedsAttention { .. }))
            {
                return Err(error(
                    "nothing_to_recover",
                    "no stale owner is waiting for recovery",
                ));
            }
            for q in &mut r.reservations {
                if matches!(q.state, ReservationState::NeedsAttention { .. }) {
                    q.state = ReservationState::Revoked {
                        reason: why.clone(),
                    };
                    q.terminal_at = Some(ctx.clock.unix_millis);
                    q.acquisition_pending = false
                }
            }
            if r.clearance_required {
                r.clearance_required = false;
                r.gate = ResourceGate::Open;
            }
            promote(r, ctx)?;
            resource_id = Some(id.clone());
        }
        ResourceCommand::ReconcileInvocation { agent, evidence } => {
            if !matches!(ctx.actor, ResourceActor::System) {
                return Err(error(
                    "system_required",
                    "lifecycle evidence must be server validated",
                ));
            }
            for r in c.resources.values_mut() {
                for q in &mut r.reservations {
                    if q.agent.same_invocation(agent) && *evidence == InvocationEligibility::Dead {
                        if q.state == ReservationState::Queued {
                            q.state = ReservationState::Cancelled;
                            q.terminal_at = Some(ctx.clock.unix_millis)
                        } else if q.state == ReservationState::Owned {
                            q.state = ReservationState::NeedsAttention {
                                reason: "Owning invocation exited; workloads may still run".into(),
                            }
                        }
                    }
                }
                promote(r, ctx)?;
            }
        }
        ResourceCommand::ReconcileEligibility => {
            if !matches!(ctx.actor, ResourceActor::System) {
                return Err(error(
                    "system_required",
                    "queue eligibility requires server proof",
                ));
            }
            for r in c.resources.values_mut() {
                promote(r, ctx)?;
            }
        }
        ResourceCommand::ReconcileBoot => {
            if !matches!(ctx.actor, ResourceActor::System) {
                return Err(error(
                    "system_required",
                    "boot reconciliation requires server evidence",
                ));
            }
            if c.boot_id != ctx.clock.boot_id {
                for r in c.resources.values_mut() {
                    for q in &mut r.reservations {
                        if q.state == ReservationState::Queued {
                            q.state = ReservationState::Cancelled;
                            q.terminal_at = Some(ctx.clock.unix_millis)
                        } else if q.state.occupied() {
                            q.state = ReservationState::NeedsAttention {
                                reason: "Machine boot changed; physical clearance required".into(),
                            };
                            q.ownership_generation = r.next_generation;
                            r.next_generation = r
                                .next_generation
                                .checked_add(1)
                                .ok_or_else(|| error("overflow", "generation exhausted"))?;
                            q.acquisition_pending = false
                        }
                    }
                }
                c.boot_id = ctx.clock.boot_id.clone()
            }
        }
        ResourceCommand::SetSyncDebt {
            project_id,
            resource_id: id,
            reason: r,
        } => {
            if !matches!(ctx.actor, ResourceActor::System | ResourceActor::User) {
                return Err(error(
                    "system_required",
                    "instruction synchronization requires authority",
                ));
            }
            if let Some(r) = r {
                reason(r)?
            }
            let link = c
                .links
                .iter_mut()
                .find(|l| l.project_id == *project_id && l.resource_id == *id)
                .ok_or_else(|| error("not_found", "project link missing"))?;
            link.sync_debt = r.clone();
            resource_id = Some(id.clone());
        }
    }
    c.revision = c
        .revision
        .checked_add(1)
        .ok_or_else(|| error("overflow", "catalog revision exhausted"))?;
    // Admission timestamps reject ancient retries before receipt lookup, so pruning cannot recreate mutations.
    let protected: BTreeSet<_> = c
        .resources
        .values()
        .flat_map(|r| &r.reservations)
        .filter(|q| {
            q.state.active()
                || q.terminal_at.is_some_and(|t| {
                    ctx.clock.unix_millis.saturating_sub(t) <= REQUEST_WINDOW_MILLIS
                })
        })
        .map(|q| q.id.clone())
        .collect();
    c.receipts.retain(|_, r| {
        ctx.clock.unix_millis.saturating_sub(r.created_at) <= REQUEST_WINDOW_MILLIS
            || r.result
                .reservation_id
                .as_ref()
                .is_some_and(|id| protected.contains(id))
    });
    for r in c.resources.values_mut() {
        let mut terminal: Vec<_> = r
            .reservations
            .iter()
            .filter(|q| !q.state.active())
            .collect();
        terminal.sort_by_key(|q| {
            (
                std::cmp::Reverse(q.terminal_at),
                std::cmp::Reverse(q.ticket),
            )
        });
        let keep: BTreeSet<_> = terminal
            .into_iter()
            .take(100)
            .map(|q| q.id.clone())
            .collect();
        r.reservations.retain(|q| {
            q.state.active()
                || keep.contains(&q.id)
                || q.terminal_at.is_some_and(|t| {
                    ctx.clock.unix_millis.saturating_sub(t) <= REQUEST_WINDOW_MILLIS
                })
        });
    }
    let result = ResourceCommandResult {
        resource_id: resource_id.clone(),
        reservation_id: reservation_id.clone(),
        revision: c.revision,
        reused,
    };
    history::record_lifecycle(catalog, &mut c, &command, ctx);
    c.receipts.insert(
        ctx.request_id.clone(),
        MutationReceipt {
            command,
            actor: ctx.actor.clone(),
            result: result.clone(),
            created_at: ctx.clock.unix_millis,
        },
    );
    validate_resource_catalog(&c)?;
    let events = resource_id
        .into_iter()
        .map(|id| ResourceEvent {
            resource_id: id,
            reservation_id: reservation_id.clone(),
            message: "Resource queue changed".into(),
        })
        .collect();
    Ok(ResourceTransition {
        catalog: c,
        result,
        events,
    })
}
