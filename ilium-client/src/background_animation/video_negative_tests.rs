// Included by pre_render_custody_tests.rs so the real accepted-helper fixture
// remains the single source of package, activation, quota and retirement proof.
const UNBRANDED_VIDEO_ASSET_SCRIPT: &str = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}} export async function create(host){const denied=await host.media.video.open({asset:{kind:'asset',id:'selected-unissued'},relative_path:'clip.ppm',max_pixels:8,max_fps:2});if(denied.ok)throw Error('unbranded_video_asset_was_accepted');return {render(context,frame){frame.cells.set_cell(0,0,{mask:1});frame.present()},dispose(){}}}"#;

const UNREVIEWED_VIDEO_HTTP_SCRIPT: &str = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}} export async function create(host){const denied=await host.media.video.open({url:'https://example.invalid/clip.ppm',max_pixels:8,max_fps:2});if(denied.ok||denied.error?.code!=='permission_denied')throw Error('unreviewed_video_http_was_not_denied');return {render(context,frame){frame.cells.set_cell(0,0,{mask:1});frame.present()},dispose(){}}}"#;

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated helper sandbox"]
fn actual_public_video_http_without_original_network_right_is_denied_before_decoder() {
    let fixture = Fixture::new(UNREVIEWED_VIDEO_HTTP_SCRIPT, AnimationMode::PreRendered);
    let (instance, presentation) = fixture.accepted_direct(AnimationMode::PreRendered);
    assert_eq!(presentation.video.recorded_count(), 0);
    assert!(presentation.video.pre_render_video_attempted());
    assert!(presentation.video.finite_work_drained());
    let instance = Arc::new(Mutex::new(instance));
    let presentation = Arc::new(Mutex::new(presentation));
    let owner = PreRenderOwner {
        instance: Arc::clone(&instance),
        presentation: Arc::clone(&presentation),
        quota: fixture.quota.clone(),
        stop: StopToken::default(),
        custody: Mutex::new(None),
    };
    owner
        .retire()
        .expect("denied HTTP Video leaves no decoder or helper behind");
    assert!(instance.lock().unwrap().is_physically_retired());
    assert!(presentation_is_drained(&presentation.lock().unwrap()));
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated helper sandbox"]
fn actual_public_video_rejects_unbranded_selected_asset_before_decoder() {
    let fixture = Fixture::new(UNBRANDED_VIDEO_ASSET_SCRIPT, AnimationMode::PreRendered);
    let (instance, presentation) = fixture.accepted_direct(AnimationMode::PreRendered);
    assert_eq!(presentation.video.recorded_count(), 0);
    assert!(presentation.video.finite_work_drained());
    let instance = Arc::new(Mutex::new(instance));
    let presentation = Arc::new(Mutex::new(presentation));
    let owner = PreRenderOwner {
        instance: Arc::clone(&instance),
        presentation: Arc::clone(&presentation),
        quota: fixture.quota.clone(),
        stop: StopToken::default(),
        custody: Mutex::new(None),
    };
    owner
        .retire()
        .expect("unbranded Video asset leaves no helper behind");
    assert!(instance.lock().unwrap().is_physically_retired());
    assert!(presentation_is_drained(&presentation.lock().unwrap()));
}
