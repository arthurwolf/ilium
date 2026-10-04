#![cfg(all(feature = "native-host", feature = "native-network"))]
use ilium_animation_js::{
    error::{AnimationError, Result},
    http::HttpOptions,
    sources::*,
};
use ilium_execution::{QuotaGroup, QuotaLimits};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Default)]
struct State {
    requests: Arc<AtomicUsize>,
    denied: Arc<AtomicBool>,
    fail: Arc<AtomicBool>,
    defer: Arc<AtomicBool>,
    body: Arc<Mutex<Value>>,
    satellite: Arc<AtomicBool>,
    decodes: Arc<AtomicUsize>,
}
struct FakeClient(State);
fn error<T>() -> Result<T> {
    Err(AnimationError::Runtime("fixture rejection".into()))
}
impl NativeSourceClient for FakeClient {
    fn authorize_demand(&self, _: &SourceDemand) -> Result<()> {
        if self.0.denied.load(Ordering::SeqCst) {
            error()
        } else {
            Ok(())
        }
    }
    fn authorize_operation(&self, _: &SourceRequest) -> Result<()> {
        if self.0.denied.load(Ordering::SeqCst) {
            error()
        } else {
            Ok(())
        }
    }
    fn admit_refresh(&mut self, _: &str, _: u64, _: u64) -> Result<bool> {
        Ok(!self.0.defer.load(Ordering::SeqCst))
    }
    fn request_bytes(
        &mut self,
        options: &HttpOptions,
        quota: &QuotaGroup,
        stop: &AtomicBool,
    ) -> Result<SourceHttpResponse> {
        self.0.requests.fetch_add(1, Ordering::SeqCst);
        if self.0.fail.load(Ordering::SeqCst) {
            return error();
        }
        let (status, data) = if self.0.satellite.load(Ordering::SeqCst) {
            (
                if options.url.contains("GapFilled") {
                    404
                } else {
                    200
                },
                vec![137, 80, 78, 71],
            )
        } else {
            let body = self.0.body.lock().unwrap().clone();
            let value = if body.is_null() {
                json!({"type":"FeatureCollection","features":[]})
            } else {
                body
            };
            let bytes = match value {
                Value::String(text) => text.into_bytes(),
                value => serde_json::to_vec(&value)?,
            };
            (200, bytes)
        };
        SourceHttpResponse::read(
            status,
            &mut std::io::Cursor::new(data),
            quota,
            options.max_bytes,
            stop,
        )
    }
    fn stream_lines(
        &mut self,
        _options: &HttpOptions,
        _: &AtomicBool,
        _: usize,
        _: usize,
        callback: &mut dyn FnMut(&[u8]) -> Result<bool>,
    ) -> Result<u16> {
        let event = self.0.body.lock().unwrap().clone();
        if event["t"] != "featured" {
            return error();
        }
        callback(&serde_json::to_vec(&event)?)?;
        Ok(200)
    }
    fn admit_process_baseline(&mut self, _: &'static str, _: usize) -> Result<()> {
        Ok(())
    }
    fn decode_image(
        &mut self,
        _: &[u8],
        _: usize,
        quota: &QuotaGroup,
        _: &AtomicBool,
    ) -> Result<NativeSourceImage> {
        if !self.0.satellite.load(Ordering::SeqCst) {
            return error();
        }
        self.0.decodes.fetch_add(1, Ordering::SeqCst);
        // Synthetic fixture pixels encoded and decoded through the REAL native
        // media owner; no fabricated production handle/metadata.
        use image::ImageEncoder;
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&[255, 0, 0, 255], 1, 1, image::ColorType::Rgba8.into())
            .unwrap();
        let mut media = ilium_animation_js::native_media::NativeMedia::new(
            quota.clone(),
            ilium_animation_js::native_media::MediaLimits::default(),
        )?;
        let handle = media.decode(&png, &ilium_platform::owned_worker::StopToken::default())?;
        NativeSourceImage::from_native(&media, handle)
    }
    fn terrain(
        &mut self,
        body: &str,
        seed: u32,
        quota: &QuotaGroup,
        stop: &AtomicBool,
    ) -> Result<Arc<NativeSourceHeightfield>> {
        let world = serde_json::from_value(json!(body))?;
        NativeSourceHeightfield::load_native(world, seed, quota, stop)
    }
}
fn bounds() -> GeographicBounds {
    GeographicBounds {
        west: -180.,
        east: 180.,
        south: -90.,
        north: 90.,
    }
}
fn demand() -> SourceDemand {
    SourceDemand::Earthquakes(GeoOptions {
        bounds: bounds(),
        max_entities: 10,
        max_hz: 60.,
        fields: vec![],
        credential: None,
        provider: None,
    })
}
fn clock(monotonic_ms: u64) -> SourceClock {
    SourceClock {
        monotonic_ms,
        epoch_ms: 1_700_000_000_000,
    }
}
#[test]
fn independent_clock_cadence_cancellation_and_revoked_cache() {
    let state = State::default();
    let mut dispatcher = SourceDispatcher::new(FakeClient(state.clone()), root_quota()).unwrap();
    let stop = AtomicBool::new(false);
    let handle = dispatcher.open(demand(), clock(10)).unwrap();
    assert!(dispatcher.poll(handle, clock(10), &stop).unwrap().is_some());
    dispatcher.poll(handle, clock(100), &stop).unwrap();
    assert_eq!(state.requests.load(Ordering::SeqCst), 1);
    assert_eq!(dispatcher.next_due_ms(handle), Some(60_010));
    stop.store(true, Ordering::SeqCst);
    assert!(dispatcher.poll(handle, clock(60_010), &stop).is_err());
    assert_eq!(state.requests.load(Ordering::SeqCst), 1);
    state.denied.store(true, Ordering::SeqCst);
    assert!(dispatcher.latest(handle).is_err());
    dispatcher.close(handle).unwrap();
    assert!(dispatcher.latest(handle).is_err());
}
#[test]
fn broker_cadence_rejects_refresh_and_failure_retains_last_good() {
    let state = State::default();
    let mut dispatcher = SourceDispatcher::new(FakeClient(state.clone()), root_quota()).unwrap();
    let stop = AtomicBool::new(false);
    let handle = dispatcher.open(demand(), clock(0)).unwrap();
    state.defer.store(true, Ordering::SeqCst);
    assert!(dispatcher.poll(handle, clock(0), &stop).unwrap().is_none());
    assert_eq!(state.requests.load(Ordering::SeqCst), 0);
    state.defer.store(false, Ordering::SeqCst);
    dispatcher.poll(handle, clock(60_000), &stop).unwrap();
    state.fail.store(true, Ordering::SeqCst);
    assert!(dispatcher.poll(handle, clock(120_000), &stop).is_err());
    let latest = dispatcher.latest(handle).unwrap().unwrap();
    let value = serde_json::to_value(&*latest).unwrap();
    assert_eq!(value["revision"], 1);
    assert_eq!(value["status"], "error");
    assert!(value["available"].as_bool().unwrap());
    assert!(dispatcher.next_due_ms(handle).unwrap() > 180_000);
}
#[test]
fn all_seven_families_have_real_catalogue_defaults_and_reject_cross_family() {
    for provider in [
        SeriesProvider::Crypto,
        SeriesProvider::Ecb,
        SeriesProvider::NoaaSolarWind,
        SeriesProvider::Iss,
        SeriesProvider::Usgs,
        SeriesProvider::Wikipedia,
        SeriesProvider::DrandQuicknet,
    ] {
        assert!(
            validate_demand(&SourceDemand::Series(SeriesOptions {
                provider,
                source_id: None,
                max_samples: 20,
                interval_ms: 1,
                window_minutes: 1440
            }))
            .unwrap()
                >= 1000
        );
    }
    assert!(validate_demand(&SourceDemand::Series(SeriesOptions {
        provider: SeriesProvider::Crypto,
        source_id: Some("iss_altitude".into()),
        max_samples: 20,
        interval_ms: 1,
        window_minutes: 1440
    }))
    .is_err());
}
#[test]
fn invalid_demands_never_authorize_network_and_dateline_bounds_work() {
    let state = State::default();
    let mut dispatcher = SourceDispatcher::new(FakeClient(state.clone()), root_quota()).unwrap();
    let invalid = SourceDemand::Aircraft(GeoOptions {
        bounds: bounds(),
        max_entities: 10,
        max_hz: f64::NAN,
        fields: vec![],
        credential: None,
        provider: None,
    });
    assert!(dispatcher.open(invalid, clock(0)).is_err());
    assert_eq!(state.requests.load(Ordering::SeqCst), 0);
    let wrap = GeographicBounds {
        west: 170.,
        east: -170.,
        south: -10.,
        north: 10.,
    };
    assert!(wrap.validate().is_ok());
    assert!(wrap.contains(0., 179.));
    assert!(wrap.contains(0., -179.));
    assert!(!wrap.contains(0., 0.));
}
#[test]
fn astronomy_is_admitted_and_declares_physical_and_directional_frames() {
    let mut dispatcher = SourceDispatcher::new(FakeClient(State::default()), root_quota()).unwrap();
    let stop = AtomicBool::new(false);
    let catalogue = dispatcher
        .dispatch(
            SourceRequest::AstronomyCatalogue {
                name: "bright_stars".into(),
                max_stars: 12,
            },
            clock(0),
            &stop,
        )
        .unwrap();
    assert_eq!(catalogue["stars"].as_array().unwrap().len(), 12);
    assert_eq!(catalogue["frame"], "J2000_equatorial_unit_direction");
    let observed = dispatcher
        .dispatch(
            SourceRequest::AstronomyObserve {
                epoch_ms: 1_700_000_000_000,
                latitude: 48.,
                longitude: 2.,
            },
            clock(0),
            &stop,
        )
        .unwrap();
    assert_eq!(
        observed["heliocentric"]["bodies"].as_array().unwrap().len(),
        8
    );
    assert_eq!(observed["bodies"].as_array().unwrap().len(), 7);
    assert_eq!(observed["heliocentric"]["units"], "astronomical_units");
}

#[test]
fn selected_fields_keep_real_zero_magnitude_and_omit_unrequested_details() {
    let state = State::default();
    *state.body.lock().unwrap() = json!({"type":"FeatureCollection","features":[{"id":"fixture-quake","geometry":{"type":"Point","coordinates":[2,48,9]},"properties":{"time":1700000000000_i64,"mag":0,"place":"fixture place"}}]});
    let mut dispatcher = SourceDispatcher::new(FakeClient(state), root_quota()).unwrap();
    let stop = AtomicBool::new(false);
    let options = GeoOptions {
        bounds: bounds(),
        max_entities: 1,
        max_hz: 1.,
        fields: vec!["magnitude".into()],
        credential: None,
        provider: None,
    };
    let handle = dispatcher
        .open(SourceDemand::Earthquakes(options), clock(0))
        .unwrap();
    let result = dispatcher.poll(handle, clock(0), &stop).unwrap().unwrap();
    let value = serde_json::to_value(&*result).unwrap();
    assert_eq!(value["entities"][0]["magnitude"], 0.);
    assert_eq!(value["entities"][0]["id"], "fixture-quake");
    assert!(value["entities"][0].get("depth_km").is_none());
    assert!(value["entities"][0].get("epoch_ms").is_none());
    assert_eq!(value["observed_at_ms"], 1700000000000_i64);
}
#[test]
fn provider_selection_has_distinct_native_cadence_and_no_silent_fallback() {
    let state = State::default();
    *state.body.lock().unwrap() = json!({"type":"FeatureCollection","features":[{"mmsi":123,"geometry":{"type":"Point","coordinates":[24,60]},"properties":{"timestampExternal":1700000000000_i64,"heading":511,"cog":90,"sog":10}}]});
    let mut dispatcher = SourceDispatcher::new(FakeClient(state), root_quota()).unwrap();
    let options = GeoOptions {
        bounds: bounds(),
        max_entities: 1,
        max_hz: 60.,
        fields: vec!["heading".into(), "speed".into(), "epoch_ms".into()],
        credential: None,
        provider: Some(BoatProvider::Digitraffic),
    };
    assert_eq!(
        validate_demand(&SourceDemand::Boats(options.clone())).unwrap(),
        30000
    );
    assert!(validate_demand(&SourceDemand::Aircraft(options.clone())).is_err());
    let handle = dispatcher
        .open(SourceDemand::Boats(options), clock(0))
        .unwrap();
    let result = dispatcher
        .poll(handle, clock(0), &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    let value = serde_json::to_value(&*result).unwrap();
    assert!(value["coverage"].as_str().unwrap().contains("Finnish"));
    assert!(value["entities"][0].get("heading_degrees").is_none());
    assert!(
        (value["entities"][0]["speed_mps"].as_f64().unwrap() - 10. * 1852. / 3600.).abs() < 1e-8
    );
}
fn weather_options(layer: &str) -> WeatherOptions {
    WeatherOptions {
        geographic: GeoOptions {
            bounds: bounds(),
            max_entities: 1,
            max_hz: 1.,
            fields: vec![],
            credential: None,
            provider: None,
        },
        layers: vec![layer.into()],
        image_width: 512,
        image_height: 512,
        max_frames: 1,
        max_tiles: 9,
        history_hours: 0,
        anchor_epoch_ms: None,
    }
}
#[test]
fn night_daily_checks_five_dates_then_declares_static_fallback() {
    let state = State::default();
    state.satellite.store(true, Ordering::SeqCst);
    let mut dispatcher = SourceDispatcher::new(FakeClient(state.clone()), root_quota()).unwrap();
    let handle = dispatcher
        .open(
            SourceDemand::Weather(weather_options("night_lights_daily")),
            clock(0),
        )
        .unwrap();
    let result = dispatcher
        .poll(handle, clock(0), &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    let value = serde_json::to_value(&*result).unwrap();
    assert_eq!(value["layers"][0]["layer"], "black_marble");
    assert_eq!(value["layers"][0]["fallback"], true);
    assert_eq!(value["layers"][0]["epoch_ms"], 1451606400000_i64);
    assert!(state.requests.load(Ordering::SeqCst) >= 6);
    assert!(state.decodes.load(Ordering::SeqCst) > 0);
}
#[test]
fn weather_options_reject_oversized_history_and_unknown_fields() {
    let mut options = weather_options("goes_east");
    options.max_frames = 25;
    assert!(options.validate().is_err());
    options.max_frames = 24;
    options.max_tiles = 50;
    assert!(options.validate().is_err());
    let mut geo = options.geographic;
    geo.fields = vec!["unknown_production_field".into()];
    assert!(geo.validate().is_err());
    geo.fields = vec!["altitude".into(), "depth".into()];
    assert!(geo.validate().is_ok());
    assert!(geo.requests_field("altitude_m"));
    assert!(geo.requests_field("depth_km"));
}
#[test]
fn real_moon_grid_retains_native_provenance_and_fictional_identity_is_explicit() {
    let mut dispatcher = SourceDispatcher::new(FakeClient(State::default()), root_quota()).unwrap();
    let stop = AtomicBool::new(false);
    let result = dispatcher
        .dispatch(
            SourceRequest::GeographyElevation {
                body: "moon".into(),
                bounds: bounds(),
                width: 2,
                height: 2,
                seed: None,
            },
            clock(0),
            &stop,
        )
        .unwrap();
    assert_eq!(result["elevations"].as_array().unwrap().len(), 4);
    assert_eq!(result["fictional"], false);
    assert!(result["provenance"]["credit"]
        .as_str()
        .unwrap()
        .contains("LOLA"));
    assert!(dispatcher
        .dispatch(
            SourceRequest::GeographyElevation {
                body: "unsupported".into(),
                bounds: bounds(),
                width: 2,
                height: 2,
                seed: None
            },
            clock(0),
            &stop
        )
        .is_err());
    let fictional = dispatcher
        .dispatch(
            SourceRequest::GeographyElevation {
                body: "pangaea".into(),
                bounds: bounds(),
                width: 2,
                height: 2,
                seed: Some(42),
            },
            clock(0),
            &stop,
        )
        .unwrap();
    assert_eq!(fictional["fictional"], true);
    assert_eq!(fictional["seed"], 42);
    assert_eq!(fictional["provenance"]["fictional"], true);
}

#[test]
fn bounded_earth_contours_declare_zero_level_and_cancellation_prevents_loading() {
    let mut dispatcher = SourceDispatcher::new(FakeClient(State::default()), root_quota()).unwrap();
    let stop = AtomicBool::new(false);
    let result = dispatcher
        .dispatch(
            SourceRequest::GeographyCoastlines {
                body: "earth".into(),
                bounds: bounds(),
                max_points: 2,
                seed: None,
            },
            clock(0),
            &stop,
        )
        .unwrap();
    assert_eq!(
        result["semantics"],
        "zero_elevation_contour_not_implied_water_boundary"
    );
    assert!(result["paths"].as_array().unwrap().len() <= 1);
    stop.store(true, Ordering::SeqCst);
    assert!(dispatcher
        .dispatch(
            SourceRequest::GeographyElevation {
                body: "moon".into(),
                bounds: bounds(),
                width: 2,
                height: 2,
                seed: None
            },
            clock(0),
            &stop
        )
        .is_err());
}

#[test]
fn tv_demand_follows_featured_identity_and_retains_real_clocks() {
    let state = State::default();
    let event = |id: &str| json!({"t":"featured","d":{"id":id,"fen":"4k3/8/8/8/8/8/8/4K3 w - - 0 1","players":[{"color":"white","seconds":30},{"color":"black","seconds":40}]}});
    *state.body.lock().unwrap() = event("firstgame");
    let mut dispatcher = SourceDispatcher::new(FakeClient(state.clone()), root_quota()).unwrap();
    let handle = dispatcher
        .open(
            SourceDemand::Chess(ChessOptions {
                game_id: "tv".into(),
                max_hz: 1.,
            }),
            clock(0),
        )
        .unwrap();
    let stop = AtomicBool::new(false);
    let first = dispatcher.poll(handle, clock(0), &stop).unwrap().unwrap();
    let first = serde_json::to_value(&*first).unwrap();
    assert_eq!(first["game_id"], "firstgame");
    assert_eq!(first["white_seconds"], 30);
    *state.body.lock().unwrap() = event("nextgame");
    let second = dispatcher
        .poll(handle, clock(1000), &stop)
        .unwrap()
        .unwrap();
    let second = serde_json::to_value(&*second).unwrap();
    assert_eq!(second["game_id"], "nextgame");
    assert_eq!(second["black_seconds"], 40);
    assert!(second.get("observed_at_ms").is_none());
}
#[test]
fn weather_json_contract_preserves_flattened_fields_and_caps() {
    let options = weather_options("black_marble");
    let encoded = serde_json::to_value(options).unwrap();
    let decoded: WeatherOptions = serde_json::from_value(encoded).unwrap();
    assert!(decoded.validate().is_ok());
}

fn quota_with_bytes(worker_bytes: usize) -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 0,
        worker_bytes,
    })
}
fn root_quota() -> QuotaGroup {
    quota_with_bytes(2048 * 1024 * 1024)
}

#[test]
fn escaped_snapshot_debit_survives_cache_close_dispatcher_drop_and_all_but_last_arc() {
    let quota = root_quota();
    let state = State::default();
    let mut dispatcher = SourceDispatcher::new(FakeClient(state), quota.clone()).unwrap();
    let stop = AtomicBool::new(false);
    let handle = dispatcher.open(demand(), clock(0)).unwrap();
    let demand_charge = quota.snapshot().worker_bytes;
    assert!(demand_charge > 0);
    let first = dispatcher.poll(handle, clock(0), &stop).unwrap().unwrap();
    let snapshot_charge = quota.snapshot().worker_bytes - demand_charge;
    assert!(snapshot_charge > 0);
    let escaped = dispatcher.latest(handle).unwrap().unwrap();
    assert!(Arc::ptr_eq(&first, &escaped));
    assert_eq!(
        quota.snapshot().worker_bytes,
        demand_charge + snapshot_charge
    );
    dispatcher.close(handle).unwrap();
    drop(dispatcher);
    assert_eq!(quota.snapshot().worker_bytes, snapshot_charge);
    drop(first);
    assert_eq!(quota.snapshot().worker_bytes, snapshot_charge);
    drop(escaped);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn error_status_copy_has_its_own_debit_while_old_escaped_payload_stays_immutable() {
    let quota = root_quota();
    let state = State::default();
    let mut dispatcher = SourceDispatcher::new(FakeClient(state.clone()), quota.clone()).unwrap();
    let stop = AtomicBool::new(false);
    let handle = dispatcher.open(demand(), clock(0)).unwrap();
    let demand_charge = quota.snapshot().worker_bytes;
    let old = dispatcher.poll(handle, clock(0), &stop).unwrap().unwrap();
    let snapshot_charge = quota.snapshot().worker_bytes - demand_charge;
    state.fail.store(true, Ordering::SeqCst);
    assert!(dispatcher.poll(handle, clock(60_000), &stop).is_err());
    let current = dispatcher.latest(handle).unwrap().unwrap();
    assert_eq!(serde_json::to_value(&*old).unwrap()["status"], "ready");
    assert_eq!(serde_json::to_value(&*current).unwrap()["status"], "error");
    assert_eq!(
        quota.snapshot().worker_bytes,
        demand_charge + 2 * snapshot_charge
    );
    dispatcher.close_all();
    drop(dispatcher);
    drop(old);
    assert_eq!(quota.snapshot().worker_bytes, snapshot_charge);
    drop(current);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn root_peak_refusal_precedes_provider_fetch_and_releases_unpublished_result_guard() {
    let quota = quota_with_bytes(4 * 1024 * 1024);
    let state = State::default();
    let mut dispatcher = SourceDispatcher::new(FakeClient(state.clone()), quota.clone()).unwrap();
    let stop = AtomicBool::new(false);
    let handle = dispatcher.open(demand(), clock(0)).unwrap();
    let demand_charge = quota.snapshot().worker_bytes;
    assert!(matches!(
        dispatcher.poll(handle, clock(0), &stop),
        Err(AnimationError::Budget(_))
    ));
    assert_eq!(state.requests.load(Ordering::SeqCst), 0);
    assert!(dispatcher.latest(handle).unwrap().is_none());
    assert_eq!(quota.snapshot().worker_bytes, demand_charge);
    dispatcher.close(handle).unwrap();
    drop(dispatcher);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn one_shot_result_retains_original_root_debit_until_final_reader_drops() {
    let quota = root_quota();
    let mut dispatcher =
        SourceDispatcher::new(FakeClient(State::default()), quota.clone()).unwrap();
    let result = dispatcher
        .dispatch(
            SourceRequest::GeographyProject {
                latitude: 0.,
                longitude: 0.,
                projection: "equirectangular".into(),
            },
            clock(0),
            &AtomicBool::new(false),
        )
        .unwrap();
    let escaped = Arc::clone(&result);
    assert!(result["x"].is_number());
    drop(dispatcher);
    let charge = quota.snapshot().worker_bytes;
    assert!(charge > 0);
    drop(result);
    assert_eq!(quota.snapshot().worker_bytes, charge);
    drop(escaped);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
#[test]
fn raw_response_admission_precedes_first_reader_call_and_follows_owned_bytes() {
    struct CountReader(usize);
    impl std::io::Read for CountReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            self.0 += 1;
            Ok(0)
        }
    }
    let stop = AtomicBool::new(false);
    let mut reader = CountReader(0);
    let too_small = quota_with_bytes(8);
    assert!(matches!(
        SourceHttpResponse::read(200, &mut reader, &too_small, 1024, &stop),
        Err(AnimationError::Budget(_))
    ));
    assert_eq!(reader.0, 0);
    assert_eq!(too_small.snapshot().worker_bytes, 0);
    let quota = quota_with_bytes(4096);
    let response =
        SourceHttpResponse::read(200, &mut std::io::Cursor::new(b"abc"), &quota, 1024, &stop)
            .unwrap();
    assert!(quota.snapshot().worker_bytes >= 1025);
    drop(response);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
#[test]
fn json_cardinality_and_string_preflight_rejects_before_native_feed_projection() {
    let state = State::default();
    let quota = root_quota();
    let mut dispatcher = SourceDispatcher::new(FakeClient(state.clone()), quota.clone()).unwrap();
    let handle = dispatcher.open(demand(), clock(0)).unwrap();
    let baseline = quota.snapshot().worker_bytes;
    *state.body.lock().unwrap() = json!({"features":[],"ignored":vec![Value::Null;16_384]});
    assert!(matches!(
        dispatcher.poll(handle, clock(0), &AtomicBool::new(false)),
        Err(AnimationError::Budget(_))
    ));
    assert!(dispatcher.latest(handle).unwrap().is_none());
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    *state.body.lock().unwrap() = json!({"features":[],"ignored":"a".repeat(8193)});
    assert!(matches!(
        dispatcher.poll(handle, clock(120_000), &AtomicBool::new(false)),
        Err(AnimationError::Budget(_))
    ));
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}
#[test]
fn bounded_native_article_preserves_content_attribution_and_escaped_result_custody() {
    let state = State::default();
    *state.body.lock().unwrap() = json!("<html><head><meta property='mw:revisionId' content='42'></head><body><div class='mw-parser-output'><p>Earth <b>native</b> content.</p></div></body></html>");
    let quota = root_quota();
    let mut dispatcher = SourceDispatcher::new(FakeClient(state.clone()), quota.clone()).unwrap();
    let baseline = quota.snapshot().worker_bytes;
    let result = dispatcher
        .dispatch(
            SourceRequest::WikipediaArticle {
                title: "Earth".into(),
                max_bytes: 16384,
                max_images: 0,
            },
            clock(0),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(state.requests.load(Ordering::SeqCst), 1);
    assert_eq!(result["title"], "Earth");
    assert_eq!(result["revision"], "42");
    assert_eq!(result["blocks"][0]["kind"], "paragraph");
    assert!(result["blocks"][0]["spans"]
        .as_array()
        .unwrap()
        .iter()
        .any(|span| span["text"].as_str().unwrap().contains("native") && span["bold"] == true));
    assert!(result["attribution"].as_str().unwrap().contains("CC BY-SA"));
    assert!(quota.snapshot().worker_bytes > baseline);
    let escaped = result.clone();
    drop(result);
    drop(dispatcher);
    assert!(quota.snapshot().worker_bytes > 0);
    drop(escaped);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
#[test]
fn article_peak_refusal_precedes_transport_and_dense_dom_errors_release_all_admissions() {
    let state = State::default();
    let quota = quota_with_bytes(4 * 1024 * 1024);
    let mut dispatcher = SourceDispatcher::new(FakeClient(state.clone()), quota.clone()).unwrap();
    let baseline = quota.snapshot().worker_bytes;
    let request = || SourceRequest::WikipediaArticle {
        title: "Earth".into(),
        max_bytes: 1024 * 1024,
        max_images: 0,
    };
    assert!(matches!(
        dispatcher.dispatch(request(), clock(0), &AtomicBool::new(false)),
        Err(AnimationError::Budget(_))
    ));
    assert_eq!(state.requests.load(Ordering::SeqCst), 0);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    drop(dispatcher);
    let quota = root_quota();
    let mut dispatcher = SourceDispatcher::new(FakeClient(state.clone()), quota.clone()).unwrap();
    let baseline = quota.snapshot().worker_bytes;
    *state.body.lock().unwrap() = json!(format!(
        "<body><p>Content</p>{}</body>",
        "<div></div>".repeat(32768)
    ));
    assert!(
        matches!(dispatcher.dispatch(request(), clock(0), &AtomicBool::new(false)), Err(AnimationError::Runtime(message)) if message.contains("DOM"))
    );
    assert_eq!(state.requests.load(Ordering::SeqCst), 1);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    *state.body.lock().unwrap() = json!(format!(
        "<body><p>{}</p></body>",
        "<b>a</b><i>b</i>".repeat(2000)
    ));
    assert!(
        matches!(dispatcher.dispatch(request(), clock(0), &AtomicBool::new(false)), Err(AnimationError::Budget(message)) if message.contains("JSON expansion"))
    );
    assert_eq!(state.requests.load(Ordering::SeqCst), 2);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    drop(dispatcher);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn oversized_wire_and_unknown_native_image_handle_leave_original_root_unchanged() {
    let quota = root_quota();
    assert!(SourceHttpResponse::read(
        200,
        &mut std::io::Cursor::new(vec![0u8; 1025]),
        &quota,
        1024,
        &AtomicBool::new(false)
    )
    .is_err());
    assert_eq!(quota.snapshot().worker_bytes, 0);
    let media = ilium_animation_js::native_media::NativeMedia::new(
        quota.clone(),
        ilium_animation_js::native_media::MediaLimits::default(),
    )
    .unwrap();
    let baseline = quota.snapshot().worker_bytes;
    assert!(NativeSourceImage::from_native(
        &media,
        ilium_animation_js::native_media::ImageHandle::from_id(999)
    )
    .is_err());
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    drop(media);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
#[test]
fn weather_snapshot_retains_actual_native_image_debit_after_dispatcher_retirement() {
    let quota = root_quota();
    let state = State::default();
    state.satellite.store(true, Ordering::SeqCst);
    let mut dispatcher = SourceDispatcher::new(FakeClient(state), quota.clone()).unwrap();
    let handle = dispatcher
        .open(
            SourceDemand::Weather(weather_options("black_marble")),
            clock(0),
        )
        .unwrap();
    let result = dispatcher
        .poll(handle, clock(0), &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    let encoded = serde_json::to_value(&*result).unwrap();
    assert!(encoded["layers"].is_array());
    drop(dispatcher);
    let charge = quota.snapshot().worker_bytes;
    assert!(
        charge > 32 * 1024 * 1024,
        "snapshot also owns real pixel admissions"
    );
    let escaped = Arc::clone(&result);
    drop(result);
    assert_eq!(quota.snapshot().worker_bytes, charge);
    drop(escaped);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn native_embedded_heightfield_keeps_original_root_admission_through_last_arc() {
    let quota = root_quota();
    let field = NativeSourceHeightfield::load_native(
        ilium_ambient::animation_services::topography::WorldId::Moon,
        0,
        &quota,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!field.view().meters.is_empty());
    let escaped = Arc::clone(&field);
    let charge = quota.snapshot().worker_bytes;
    assert!(charge >= 8 * 1024 * 1024);
    drop(field);
    assert_eq!(quota.snapshot().worker_bytes, charge);
    drop(escaped);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn osm_repeated_way_references_are_bounded_before_native_geometry_expansion() {
    let state = State::default();
    *state.body.lock().unwrap() = json!({"elements":[
        {"type":"way","id":1,"geometry":[{"lat":0,"lon":0},{"lat":0,"lon":0.01}]},
        {"type":"relation","id":2,"tags":{"type":"multipolygon","building":"yes"},"members":vec![json!({"type":"way","ref":1,"role":"outer"});1000]}
    ]});
    let quota = root_quota();
    let mut dispatcher = SourceDispatcher::new(FakeClient(state.clone()), quota.clone()).unwrap();
    let baseline = quota.snapshot().worker_bytes;
    assert!(matches!(
        dispatcher.dispatch(
            SourceRequest::OsmTile {
                x: 8192,
                y: 8192,
                zoom: 14,
                format: "vector".into()
            },
            clock(0),
            &AtomicBool::new(false)
        ),
        Err(AnimationError::Budget(_))
    ));
    assert_eq!(state.requests.load(Ordering::SeqCst), 1);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}
