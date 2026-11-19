//! A job must observe completion of requests made through other host interfaces.

use ilium_plugin_api::generated::host::inference::HostInference;
use ilium_plugin_api::generated::host::jobs::HostJobs;
use ilium_plugin_api::generated::types::errors::ErrorCode;
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_host::imports::env::ImportEnv;
use ilium_plugin_host::imports::inference::InferenceImport;
use ilium_plugin_host::imports::jobs::{JobBook, JobDeclaration, JobsImport};

fn job_context(env: ImportEnv) -> JobsImport {
    let book = JobBook::new();
    let declaration = JobDeclaration {
        kind: "test".to_owned(),
        deadline_ms: 30_000,
        memory_mib: 64,
    };
    let mut parent = JobsImport::new(env.clone(), book.clone(), vec![declaration]);
    let request = parent
        .start("test".to_owned(), Vec::new())
        .expect("job accepted");
    JobsImport::for_job(env, book, request)
}

#[test]
fn job_wait_reports_an_inference_request_failure() {
    let env = ImportEnv::unrestricted().with_job_scope();
    let mut inference = InferenceImport::new(env.clone(), None);
    let mut jobs = job_context(env);
    let request = inference.models().expect("model request accepted");

    let error = jobs
        .wait_request(request, 5_000)
        .expect_err("missing inference provider must fail");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

#[test]
fn waiting_for_an_unknown_request_reports_not_found() {
    let env = ImportEnv::unrestricted().with_job_scope();
    let mut jobs = job_context(env);
    let error = jobs
        .wait_request(RequestId(12345), 0)
        .expect_err("an unknown request cannot finish");
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn a_plugin_handler_cannot_block_on_a_request() {
    let mut jobs = JobsImport::new(ImportEnv::unrestricted(), JobBook::new(), Vec::new());
    let error = jobs
        .wait_request(RequestId(12345), 0)
        .expect_err("only a job instance can wait");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

#[test]
fn a_job_cannot_start_another_job() {
    let mut jobs = job_context(ImportEnv::unrestricted());
    let error = jobs.start("test".to_owned(), Vec::new()).expect_err("jobs cannot start jobs");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

#[test]
fn a_jobs_slow_call_completion_is_not_sent_to_its_parent_handler() {
    let parent = ImportEnv::unrestricted();
    let job = parent.clone().with_job_scope();
    let mut inference = InferenceImport::new(job.clone(), None);
    let mut jobs = job_context(job);
    let request = inference.models().expect("model request accepted");
    let error = jobs.wait_request(request, 5_000).expect_err("provider is absent");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert!(parent.events().is_empty(), "the job has no event handler and must not notify the parent");
}

#[test]
fn concurrent_jobs_of_one_plugin_cannot_read_each_others_requests() {
    let parent = ImportEnv::unrestricted();
    let first = parent.clone().with_job_scope();
    let second = parent.with_job_scope();
    let mut inference = InferenceImport::new(first, None);
    let request = inference.models().expect("model request accepted");
    let mut jobs = job_context(second);
    let error = jobs.wait_request(request, 0).expect_err("a sibling job owns the request");
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn a_job_cannot_wait_for_another_plugins_request() {
    let env = ImportEnv::unrestricted().with_job_scope();
    let mut inference = InferenceImport::new(env.clone().with_identity("other.plugin", 1), None);
    let request = inference.models().expect("model request accepted");
    let mut jobs = job_context(env.with_identity("owner.plugin", 1));
    let error = jobs
        .wait_request(request, 5_000)
        .expect_err("a foreign request is not visible");
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn a_restarted_instance_cannot_wait_for_its_predecessors_request() {
    let env = ImportEnv::unrestricted().with_job_scope();
    let mut inference = InferenceImport::new(env.clone().with_identity("owner.plugin", 1), None);
    let request = inference.models().expect("model request accepted");
    let mut jobs = job_context(env.with_identity("owner.plugin", 2));
    let error = jobs
        .wait_request(request, 5_000)
        .expect_err("a request from an old instance is not visible");
    assert_eq!(error.code, ErrorCode::NotFound);
}
