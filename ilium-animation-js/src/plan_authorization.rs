//! Host-derived permission dependencies. Script plans declare requests, while
//! the native broker determines which inputs and operations can actually run.
use crate::{
    error::{AnimationError, Result},
    manifest::{Capability as WireCapability, Manifest},
    permission_projection,
    permissions::{
        Capability, Ceiling, Channel, Demand, PermissionBroker, PermissionPlan, PermissionRequest,
    },
    plan::AnimationPlan,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub struct AuthorizationProjection {
    pub manifest_ceiling: Ceiling,
    pub permission_plan: PermissionPlan,
    /// These IDs originate in the normalized plan, never helper completion
    /// payloads. The broker still authenticates the current channel each time.
    requests: Vec<(String, Capability)>,
}

impl AuthorizationProjection {
    pub fn build(manifest: &Manifest, plan: &AnimationPlan) -> Result<Self> {
        if plan.permissions.len() > 64 || manifest.capabilities.len() > 64 {
            return Err(AnimationError::Budget("permission projection count".into()));
        }
        let manifest_ceiling = Ceiling {
            permissions: manifest
                .capabilities
                .iter()
                .map(permission_projection::right)
                .collect::<Result<Vec<_>>>()?,
        };
        let mut permissions = Vec::with_capacity(plan.permissions.len());
        let mut requests = Vec::with_capacity(plan.permissions.len());
        let mut demands = Vec::with_capacity(plan.permissions.len());
        let mut seen = BTreeSet::new();
        for request in &plan.permissions {
            let right = permission_projection::right(&WireCapability {
                id: request.id.clone(),
                scope: request.scope.clone(),
            })?;
            let request_id = if let Some(id) = &request.request_id {
                id.clone()
            } else {
                format!("scope_{:x}", Sha256::digest(serde_json::to_vec(&right)?))
            };
            if !seen.insert(request_id.clone()) {
                return Err(AnimationError::InvalidPackage(
                    "duplicate permission request ID".into(),
                ));
            }
            // Hash the ID for bounded stable native operation identifiers;
            // prepending an unrestricted 128-byte script ID would exceed the
            // broker's identifier ceiling.
            let demand_id = operation_demand(&request_id);
            demands.push(Demand {
                demand_id,
                request_ids: BTreeSet::from([request_id.clone()]),
            });
            requests.push((request_id.clone(), right.id));
            permissions.push(PermissionRequest {
                request_id: Some(request_id),
                id: right.id,
                scope: right.scope,
                required: request.required,
                reason: request.reason.clone(),
            });
        }
        for (requested, capability, family) in [
            (
                plan.inputs.pointer.is_some(),
                Capability::InputPointer,
                "pointer",
            ),
            (
                plan.inputs.occlusion.is_some(),
                Capability::ScreenOcclusion,
                "occlusion",
            ),
            (
                plan.inputs.location.is_some(),
                Capability::LocationObserver,
                "location",
            ),
        ] {
            if requested && !requests.iter().any(|(_, id)| *id == capability) {
                return Err(AnimationError::PermissionDenied(format!(
                    "{family} input has no declared permission"
                )));
            }
        }
        if plan.inputs.audio.is_some()
            && !requests.iter().any(|(_, id)| {
                matches!(id, Capability::AudioLoopback | Capability::AudioMicrophone)
            })
        {
            return Err(AnimationError::PermissionDenied(
                "audio input has no declared permission".into(),
            ));
        }
        Ok(Self {
            manifest_ceiling,
            permission_plan: PermissionPlan {
                permissions,
                demands,
            },
            requests,
        })
    }

    /// Native selection only: try exact existing singleton HTTP demands in plan
    /// order. A denied optional request stays denied; no unrelated IDs are unioned.
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn dispatch_http(
        &self,
        broker: &mut PermissionBroker,
        channel: &Channel,
        phase: crate::permissions::CallPhase,
        need: crate::permissions::OperationNeed,
    ) -> Result<crate::permissions::OperationTicket> {
        for (request_id, capability) in &self.requests {
            if *capability != Capability::NetworkHttp {
                continue;
            }
            match broker.dispatch(
                channel,
                phase,
                &operation_demand(request_id),
                vec![need.clone()],
            ) {
                Ok(ticket) => return Ok(ticket),
                Err(crate::permissions::PermissionError::Denied) => continue,
                Err(error) => return Err(AnimationError::PermissionDenied(error.to_string())),
            }
        }
        Err(AnimationError::PermissionDenied(
            "no current exact HTTP demand".into(),
        ))
    }

    /// Run before opening subscriptions or constructing input products. Query
    /// the broker's private accepted copy, not a mutable returned plan copy.
    pub fn prune_inputs(
        &self,
        broker: &PermissionBroker,
        channel: &Channel,
        plan: &AnimationPlan,
    ) -> Result<AnimationPlan> {
        // Even an entirely unprivileged plan must reject a retired channel.
        broker
            .grant(channel, "_host_channel_check")
            .map_err(|error| AnimationError::PermissionDenied(error.to_string()))?;
        let allowed = |capability: Capability| -> Result<bool> {
            for (request_id, id) in &self.requests {
                if *id != capability {
                    continue;
                }
                if broker
                    .grant(channel, request_id)
                    .map_err(|error| AnimationError::PermissionDenied(error.to_string()))?
                    .is_some()
                {
                    return Ok(true);
                }
            }
            Ok(false)
        };
        let mut result = plan.clone();
        if result.inputs.pointer.is_some() && !allowed(Capability::InputPointer)? {
            result.inputs.pointer = None;
        }
        if result.inputs.occlusion.is_some() && !allowed(Capability::ScreenOcclusion)? {
            result.inputs.occlusion = None;
        }
        if result.inputs.location.is_some() && !allowed(Capability::LocationObserver)? {
            result.inputs.location = None;
        }
        if let Some(audio) = &result.inputs.audio {
            let requested_source = audio.source.as_deref().unwrap_or("loopback");
            let mut audio_allowed = false;
            for (request_id, id) in &self.requests {
                if !matches!(id, Capability::AudioLoopback | Capability::AudioMicrophone) {
                    continue;
                }
                let Some(grant) = broker
                    .grant(channel, request_id)
                    .map_err(|error| AnimationError::PermissionDenied(error.to_string()))?
                else {
                    continue;
                };
                let crate::permissions::Scope::Audio { device, products } = &grant.right.scope
                else {
                    continue;
                };
                let correct_kind = match requested_source {
                    "microphone" => *id == Capability::AudioMicrophone,
                    "loopback" => *id == Capability::AudioLoopback,
                    _ => true, // A named device still requires its exact host binding.
                };
                let permitted: BTreeSet<String> = products
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<std::result::Result<Vec<_>, _>>()?
                    .iter()
                    .filter_map(|product| product.as_str().map(str::to_owned))
                    .collect();
                if correct_kind
                    && device == requested_source
                    && audio
                        .products
                        .iter()
                        .all(|product| permitted.contains(product))
                {
                    audio_allowed = true;
                    break;
                }
            }
            if !audio_allowed {
                result.inputs.audio = None;
            }
        }
        // Origin-specific provider dependencies are checked by the source
        // dispatcher before any refresh. With no HTTP right, no network
        // input service is opened at all, including optional denied services.
        if !allowed(Capability::NetworkHttp)? {
            result.inputs.series = None;
            result.inputs.earthquakes = None;
            result.inputs.aircraft = None;
            result.inputs.boats = None;
            result.inputs.chess = None;
            result.inputs.weather = None;
        }
        Ok(result)
    }
}

pub fn operation_demand(request_id: &str) -> String {
    format!("op_{:x}", Sha256::digest(request_id.as_bytes()))
}

#[cfg(all(test, feature = "native-host", feature = "native-network"))]
mod http_native_selection_contracts {
    use super::*;
    use crate::permissions::{
        CallPhase, HttpMethod, OperationNeed, PackageIdentity, Right, Scope, UserChoice,
    };
    use std::collections::BTreeMap;
    #[test]
    fn native_endpoint_selection_skips_denied_unrelated_request_and_never_unions_ids() {
        let first = Right {
            id: Capability::NetworkHttp,
            scope: Scope::Network {
                origins: BTreeSet::from(["https://first.example".into()]),
                methods: BTreeSet::from([HttpMethod::Get]),
            },
        };
        let second = Right {
            id: Capability::NetworkHttp,
            scope: Scope::Network {
                origins: BTreeSet::from(["https://example.org".into()]),
                methods: BTreeSet::from([HttpMethod::Get]),
            },
        };
        let ceiling = Ceiling {
            permissions: vec![first.clone(), second.clone()],
        };
        let plan = PermissionPlan {
            permissions: vec![
                PermissionRequest {
                    request_id: Some("denied_first".into()),
                    id: first.id,
                    scope: first.scope.clone(),
                    required: false,
                    reason: "Synthetic denied distinct endpoint".into(),
                },
                PermissionRequest {
                    request_id: Some("allowed_second".into()),
                    id: second.id,
                    scope: second.scope.clone(),
                    required: false,
                    reason: "Synthetic exact endpoint".into(),
                },
            ],
            demands: vec![
                Demand {
                    demand_id: operation_demand("denied_first"),
                    request_ids: BTreeSet::from(["denied_first".into()]),
                },
                Demand {
                    demand_id: operation_demand("allowed_second"),
                    request_ids: BTreeSet::from(["allowed_second".into()]),
                },
            ],
        };
        let projection = AuthorizationProjection {
            manifest_ceiling: ceiling.clone(),
            permission_plan: plan.clone(),
            requests: vec![
                ("denied_first".into(), Capability::NetworkHttp),
                ("allowed_second".into(), Capability::NetworkHttp),
            ],
        };
        let mut broker = PermissionBroker::new(
            PackageIdentity::unverified(
                "http_projection_fixture".into(),
                b"synthetic offline fixture",
            )
            .unwrap(),
            ceiling.clone(),
            ceiling,
        )
        .unwrap();
        let review = broker.prepare(1, 1, plan, BTreeMap::new()).unwrap();
        let active = broker
            .resolve(
                review,
                BTreeMap::from([
                    ("denied_first".into(), UserChoice::DenySession),
                    ("allowed_second".into(), UserChoice::AllowSession),
                ]),
            )
            .unwrap()
            .activation
            .unwrap();
        let queued = projection
            .dispatch_http(
                &mut broker,
                &active.channel,
                CallPhase::Async,
                OperationNeed::http_preflight("https://example.org/x", HttpMethod::Get).unwrap(),
            )
            .unwrap();
        broker
            .check_operation_lineage(&queued, &active.channel)
            .unwrap();
        assert!(matches!(
            broker.check_committed_operation(&queued, &active.channel),
            Err(crate::permissions::PermissionError::NotCommitted)
        )); // No effect or fake commit in this pure selection fixture.
        broker.settle_without_delivery(&queued).unwrap();
        assert!(projection
            .dispatch_http(
                &mut broker,
                &active.channel,
                CallPhase::Async,
                OperationNeed::http_preflight("https://first.example/x", HttpMethod::Get).unwrap()
            )
            .is_err());
        assert!(projection
            .dispatch_http(
                &mut broker,
                &active.channel,
                CallPhase::Async,
                OperationNeed::http_preflight("https://example.org/x", HttpMethod::Post).unwrap()
            )
            .is_err());
    }
}
