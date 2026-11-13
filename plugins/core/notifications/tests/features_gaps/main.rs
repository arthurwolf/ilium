//! Tests that need items the skeleton lacks (see ../API_GAPS.md): the client side entry
//! `NotificationsClient`, `FakeHost::published_topics`. They live in their own test target so that a
//! missing item breaks only this target and not the `features` target.

#[path = "../features/common.rs"]
mod common;

mod client;
mod topic;
