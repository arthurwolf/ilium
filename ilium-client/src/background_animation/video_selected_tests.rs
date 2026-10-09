// Included in the original actual-helper fixture module. Selected bytes are
// labelled synthetic input; no user file, microphone or external HTTP is read.
const SELECTED_VIDEO_SCRIPT: &str = r#"export function plan(){return {fps:2,output:{mode:'pixels',format:'rgba8',update:'replace'},controls:{density:false,dither:false,contrast:false,inversion:false},permissions:[{request_id:'clip',id:'disk.read',scope:{kind:'disk',slot:'clip',selection:'file'},required:true,reason:'Read the selected synthetic Video fixture'}],inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}} export async function create(host){const grant=host.permissions.get('clip');if(!grant?.handle)throw Error('original_selected_video_grant_missing');const opened=await host.media.video.open({asset:grant.handle,max_pixels:8,max_fps:2});if(!opened.ok)throw Error(opened.error.code);const video=opened.value;return {render(context,frame){const latest=video.latest();if(!latest.ok||!latest.value)throw Error('selected_video_frame_missing');const drawn=host.media.images.blit({frame,image:latest.value.image,rectangle:{unit:'pixels',x:0,y:0,width:2,height:4},fit:'stretch'});if(!drawn.ok)throw Error(drawn.error.code);frame.present()},dispose(){}}}"#;

fn selected_video_archive() -> Vec<u8> {
    let manifest = json!({
        "api_version":1,"id":"native-replay-custody","name":"Native replay custody",
        "version":"1.0.0","entry":"entry.mjs","modes":["pre_rendered"],
        "capabilities":[{"id":"disk.read","scope":{"kind":"disk","slot":"clip","selection":"file"}}],
        "settings":{"type":"object","properties":{}},
        "files":[{"path":"entry.mjs","bytes":SELECTED_VIDEO_SCRIPT.len(),
            "sha256":format!("{:x}",Sha256::digest(SELECTED_VIDEO_SCRIPT.as_bytes()))}]
    });
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("entry.mjs", options).unwrap();
    zip.write_all(SELECTED_VIDEO_SCRIPT.as_bytes()).unwrap();
    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    zip.finish().unwrap().into_inner()
}

fn selected_video_backend_before_review(fixture: &Fixture) -> PluginBackend {
    use ilium_animation_js::permissions::{Capability, Right, Scope};
    let wake = fixture.wake_sender.clone();
    let files = PluginPermissionFiles::new(
        &fixture.client,
        Arc::clone(&fixture.ledger_root),
        Arc::new(tokio::sync::Notify::new()),
        Arc::new(move || {
            let _ = wake.send(());
        }),
    )
    .unwrap();
    let mut controller = PluginPermissionController::new(files, &fixture.client).unwrap();
    controller
        .start(
            fixture.verified_with_policy(
                AnimationMode::PreRendered,
                Ceiling {
                    permissions: vec![Right {
                        id: Capability::DiskRead,
                        scope: Scope::Disk {
                            slot: "clip".into(),
                            selection: Selection::File,
                        },
                    }],
                },
            ),
            1,
        )
        .unwrap();
    let wake = fixture.wake_sender.clone();
    let mut backend = PluginBackend::new(
        fixture.quota.clone(),
        fixture.resources.clone(),
        Arc::clone(&fixture.review),
        Arc::new(move || {
            let _ = wake.send(());
        }),
        Arc::new(tokio::sync::Notify::new()),
    );
    backend.workflow = Some(Workflow {
        revision: 1,
        request: fixture.request.clone(),
        controller,
        picker: None,
        audio_picker: None,
        audio_picker_uncertain: false,
        qualified_audio: None,
        presentation: None,
        preparation_presentation: None,
        clip_root: Some(Arc::clone(&fixture.clip_root)),
        state_root: Arc::clone(&fixture.state_root),
        preparation: None,
        preparation_retention: None,
        preparation_stop: None,
        preparation_authority: None,
        player: None,
        playback_origin: None,
        last_playback_tick: None,
        last_playback_elapsed: None,
        playback_frozen: false,
        update: None,
        update_applied: false,
        cancellation: None,
        halted: false,
        unsettled_world_emissions: Default::default(),
        _setup_retention: setup_retention(fixture),
    });
    backend
}

/// Wait only on the fixture's real finite-completion channel, never fabricated
/// review tokens, bindings, asset IDs or successful permission resolutions.
fn await_selected_video_review(
    fixture: &Fixture,
    backend: &mut PluginBackend,
) -> Result<crate::animation_plugins::review_bridge::ReviewSession, String> {
    for _ in 0..20 {
        if let Some(session) = fixture.review.session()? {
            return Ok(session);
        }
        fixture
            .wake_receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| format!("Original selected Video review wake: {error}"))?;
        backend.on_native_completion()?;
    }
    Err("Actual selected Video review was not published".into())
}

#[test]
#[ignore = "requires matching ILIUM_ANIMATION_HELPER, FFmpeg and delegated Linux codec sandbox"]
fn actual_selected_video_picker_controller_replay_pixels_and_revocation() {
    use crossterm::event::KeyCode;
    let mut fixture = Fixture::new(SELECTED_VIDEO_SCRIPT, AnimationMode::PreRendered);
    fixture.archive = selected_video_archive();
    fixture.request.settings.source = crate::animation_plugins::AnimationSourceTab::Plugin;
    fixture.request.settings.plugin.selected = Some(crate::animation_plugins::PluginSelection {
        package_id: "native-replay-custody".into(),
        mode: AnimationMode::PreRendered,
        settings: json!({}),
    });
    fixture.request.settings.density_percent = 100;
    let path = fixture
        ._temporary
        .path()
        .join("synthetic-selected-white.ppm");
    // The selected file is distinct from the package archive. Real platform
    // pinning and broker-bound read must supply the decoder's immutable bytes.
    let mut bytes = b"P6\n2 2\n255\n".to_vec();
    bytes.extend_from_slice(&[255; 12]);
    std::fs::write(&path, &bytes).unwrap();
    let mut backend = selected_video_backend_before_review(&fixture);
    let outcome = (|| -> Result<_, String> {
        let mut review = await_selected_video_review(&fixture, &mut backend)?;
        review.handle_key(&fixture.review, KeyCode::Char('p'))?;
        for character in path.to_str().ok_or("Fixture path is not UTF-8")?.chars() {
            review.handle_key(&fixture.review, KeyCode::Char(character))?;
        }
        review.handle_key(&fixture.review, KeyCode::Enter)?;
        backend.on_review_intent()?;
        if backend
            .workflow
            .as_ref()
            .ok_or("Picker workflow missing")?
            .picker
            .is_none()
        {
            return Err("Selection did not submit the actual admitted picker".into());
        }
        if backend.workflow.as_ref().unwrap().presentation.is_some() {
            return Err("Helper creation preceded selected-resource approval".into());
        }
        let mut review = await_selected_video_review(&fixture, &mut backend)?;
        review.handle_key(&fixture.review, KeyCode::Char('1'))?;
        review.handle_key(&fixture.review, KeyCode::Enter)?;
        backend.on_review_intent()?;
        for _ in 0..40 {
            if backend
                .workflow
                .as_ref()
                .is_some_and(|workflow| workflow.player.is_some())
            {
                break;
            }
            fixture
                .wake_receiver
                .recv_timeout(Duration::from_secs(10))
                .map_err(|error| {
                    format!("Original selected Video creation/preparation wake: {error}")
                })?;
            backend.on_native_completion()?;
        }
        let workflow = backend
            .workflow
            .as_ref()
            .ok_or("Selected workflow missing")?;
        if workflow.player.is_none() {
            return Err("Selected Video did not reach RAM playback".into());
        }
        let owner = Arc::clone(
            workflow
                .controller
                .delegated_instance()
                .ok_or("Selected Video lost original delegated instance")?,
        );
        let physically_retired = owner
            .lock()
            .map_err(|_| "Selected owner poisoned")?
            .is_physically_retired();
        let mut request = fixture.request.clone();
        request.elapsed = Duration::from_millis(1);
        request.requested_at = Instant::now();
        let frame = backend
            .render(&request, 1, &StopToken::default())?
            .ok_or("Selected Video playback frame missing")?;
        let lease = frame
            .replay
            .as_ref()
            .ok_or("Selected Video original replay lease missing")?;
        let packed = lease.packed().map_err(|error| error.to_string())?;
        let masks = packed.masks.clone();
        let source_owned = packed.owners.iter().any(Option::is_some);
        let revoked = owner
            .lock()
            .map_err(|_| "Selected owner poisoned")?
            .revoke_activation()
            .map_err(|error| error.to_string())?
            .is_some();
        let stale_denied = lease.packed().is_err();
        Ok((
            physically_retired,
            masks,
            source_owned,
            revoked,
            stale_denied,
        ))
    })();
    // Settlement happens before result assertions so a failed oracle still
    // retains and retires the original helper, decoder and selected-read owner.
    retire_text_backend(&fixture, &mut backend)
        .expect("Original selected Video physical retirement");
    let (physically_retired, masks, source_owned, revoked, stale_denied) =
        outcome.expect("Actual UI picker/controller must produce selected Video playback");
    assert_eq!(
        masks,
        vec![255],
        "Literal white PPM must paint all eight Braille dots"
    );
    assert!(physically_retired && source_owned && revoked && stale_denied);
    assert!(backend.is_physically_settled());
}
