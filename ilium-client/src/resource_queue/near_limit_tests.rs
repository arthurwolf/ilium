//! Synthetic capacity fixture: exercises the receive path, never device state.
use super::*;
use crate::{incoming_projection::ProjectionRetention, ipc_preparation::IpcPreparation};
use ilium_ipc::ServerEvent;
use ratatui::{backend::TestBackend, Terminal};

const ACTIVE_CLAIMS: usize = 8192;

fn snapshot(revision: u64) -> (TreeFixture, ServerEvent) {
    let (tree, mut catalog, project, pane) = tree::tests::fixture();
    catalog.revision = revision;
    let record = catalog.resources.get_mut(&ResourceId::from("gpu")).unwrap();
    let base = record.reservations.pop().unwrap();
    record.reservations = (0..ACTIVE_CLAIMS)
        .map(|index| {
            let mut claim = base.clone();
            claim.id = ReservationId(format!("capacity-{index:04}"));
            claim.request_id = RequestId(format!("request-{index:04}"));
            claim.agent.invocation_id = InvocationId(format!("invocation-{index:04}"));
            claim.agent.label = format!("Synthetic agent {index:04}");
            claim.ticket = index as u64 + 1;
            claim
        })
        .collect();
    record.next_ticket = ACTIVE_CLAIMS as u64 + 1;
    (
        TreeFixture {
            tree,
            project,
            pane,
        },
        ServerEvent::ResourceSnapshot {
            catalog,
            clock: ResourceClock {
                unix_millis: 1000,
                monotonic_millis: 1000,
                boot_id: "boot".into(),
            },
            local_invocations: Vec::new(),
            active_waits: vec![(pane, ReservationId::from("capacity-8191"))],
            wait_revision: revision,
        },
    )
}

struct TreeFixture {
    tree: ilium_core::Tree,
    project: NodeId,
    pane: NodeId,
}

async fn admitted_apply(app: &mut App, preparation: &IpcPreparation, event: ServerEvent) -> usize {
    // Real framing, bounded decode, finite codec admission, independent decoded
    // storage admission, projection preparation and retained ownership transfer.
    let frame = ilium_ipc::encode_frame(&event).unwrap();
    drop(event);
    let reservation = preparation.reserve_decoder().await.unwrap();
    let decoded = preparation
        .run_reserved(reservation, move |_| {
            let event: ServerEvent = ilium_ipc::decode_bounded_frame(&frame)?;
            let bytes = event.retained_bytes();
            Ok((event, bytes))
        })
        .await
        .unwrap();
    let (event, retention) = preparation
        .retain_decoded(decoded)
        .await
        .unwrap()
        .into_parts();
    let declared = retention.as_ref().unwrap().declared_bytes();
    let mut update = ProjectionRetention::prepare_owned(&event, app, retention.as_ref()).unwrap();
    app.processing_derivation_retention = update.derived_retention.take();
    app.processing_event_retention = retention;
    crate::render_cache::apply(app, event);
    app.incoming_projection
        .commit(update, app.processing_event_retention.as_ref());
    app.processing_event_retention = None;
    app.processing_derivation_retention = None;
    assert_eq!(
        preparation.decoder_usage_jobs(),
        0,
        "persistent state must release codec job credit"
    );
    declared
}

fn rendered_text(app: &App, pane: NodeId, detail: bool) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| {
            if detail {
                super::detail::render(app, frame, frame.area());
            } else {
                super::detail::render_wait(
                    app,
                    frame,
                    ratatui::layout::Rect::new(0, 0, 80, 2),
                    pane,
                );
            }
        })
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[tokio::test]
async fn near_limit_snapshot_admission_projection_replacement_and_fresh_attach_release_credit() {
    let (_execution, preparation) = IpcPreparation::standalone().unwrap();
    assert_eq!(preparation.decoded_storage_bytes(), 0);
    let mut prior_charge = None;
    // A fresh App is the transport-reconnect consumer: its wait registry starts
    // empty and accepts the server's reset revision independently of old state.
    for attach in 0..2 {
        let (fixture, event) = snapshot(0);
        let mut first_event = Some(event);
        let mut app = App::new("test".into(), "/synthetic-resource-project".into());
        app.tree = fixture.tree;
        for revision in 0..3 {
            let event = first_event.take().unwrap_or_else(|| snapshot(revision).1);
            let declared = admitted_apply(&mut app, &preparation, event).await;
            let charge = preparation.decoded_storage_bytes();
            assert!(
                charge >= declared,
                "catalog ownership must remain admitted after apply"
            );
            assert!(
                charge <= declared + 64 * 1024,
                "replacement must not accumulate old snapshots: {charge} / {declared}"
            );
            if let Some(previous) = prior_charge {
                assert_eq!(
                    charge, previous,
                    "same-size replacement and reconnect must have stable retained quota"
                );
            }
            prior_charge = Some(charge);
            let catalog = app.resources.catalog.as_ref().unwrap();
            assert_eq!(catalog.revision, revision);
            let record = &catalog.resources[&ResourceId::from("gpu")];
            assert_eq!(record.reservations.len(), ACTIVE_CLAIMS);
            assert_eq!(app.resources.rows.targets.len(), ACTIVE_CLAIMS + 1);
            assert_eq!(app.resources.rows.projects[&fixture.project].len(), 1);
            assert_eq!(
                app.resources.waits.get(&fixture.pane),
                Some(&ReservationId::from("capacity-8191"))
            );
            let allocation = record.reservations.as_ptr();
            app.show_resource(fixture.project, ResourceId::from("gpu"));
            let detail = rendered_text(&app, fixture.pane, true);
            let strip = rendered_text(&app, fixture.pane, false);
            assert!(detail.contains("Shared resource queue"));
            assert!(strip.contains("queue position 8192"), "{strip}");
            let record = app.resources.record(&ResourceId::from("gpu")).unwrap();
            assert_eq!(
                record.reservations.as_ptr(),
                allocation,
                "rendering must borrow the one catalog allocation"
            );
            assert!(
                app.panes.is_empty(),
                "foreign claims must not manufacture per-agent terminal runtimes"
            );
            assert_eq!(
                preparation.decoded_storage_bytes(),
                charge,
                "rendering must not retain another decoded catalog"
            );
        }
        drop(app);
        assert_eq!(
            preparation.decoded_storage_bytes(),
            0,
            "attach {attach} left retained projection credit"
        );
    }
}

#[tokio::test]
async fn near_limit_snapshot_refuses_insufficient_decoded_quota_without_installing_partial_state() {
    let (_execution, preparation) = IpcPreparation::standalone().unwrap();
    let preparation = preparation.with_test_decoded_capacity(1024);
    let (_, event) = snapshot(1);
    let frame = ilium_ipc::encode_frame(&event).unwrap();
    drop(event);
    let reservation = preparation.reserve_decoder().await.unwrap();
    let decoded = preparation
        .run_reserved(reservation, move |_| {
            let event: ServerEvent = ilium_ipc::decode_bounded_frame(&frame)?;
            let bytes = event.retained_bytes();
            Ok((event, bytes))
        })
        .await
        .unwrap();
    assert!(decoded.view().1 > 1024);
    assert!(preparation.retain_decoded(decoded).await.is_err());
    assert_eq!(preparation.decoded_storage_bytes(), 0);
    assert_eq!(preparation.decoder_usage_jobs(), 0);
}
