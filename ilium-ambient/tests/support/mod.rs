//! Explicit owned execution composition for integration tests and offline probes.
use ilium_ambient::resources::AmbientResources;
use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
};

pub struct ResourcesFixture {
    pub resources: AmbientResources,
    // Owner remains alive for the caller's entire probe/fixture scope.
    _execution: Execution,
}
impl ResourcesFixture {
    pub fn new() -> Result<Self, String> {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 8,
            jobs: 16,
            service_jobs: 0,
            input_bytes: 512 * 1024 * 1024,
            result_bytes: 512 * 1024 * 1024,
            worker_threads: 16,
            worker_bytes: 2304 * 1024 * 1024,
        });
        let lane = LaneConfig {
            threads: 2,
            queue_slots: 8,
            priority: None,
            resident_bytes_per_thread: 1024 * 1024,
        };
        let execution = Execution::start(
            quota,
            ExecutionConfig {
                cpu: lane,
                io: lane,
                service: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
            },
        )
        .map_err(|error| error.to_string())?;
        let finite = execution
            .client(ClientLimits {
                jobs: 16,
                service_jobs: 0,
                input_bytes: 512 * 1024 * 1024,
                result_bytes: 512 * 1024 * 1024,
            })
            .map_err(|error| format!("fixture admission: {error:?}"))?;
        Ok(Self {
            resources: AmbientResources::new(finite),
            _execution: execution,
        })
    }
}
