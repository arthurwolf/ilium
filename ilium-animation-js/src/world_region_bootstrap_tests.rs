//! Actual V8 validation of the complete SDK volume at the trusted JS boundary.
//! The test-only hook exposes an existing validator; it never mints a world,
//! service grant, source lease, ACK or terminal receipt.
use super::*;

fn region_engine(quota: QuotaGroup) -> Engine {
    let bootstrap = crate::TRUSTED_BOOTSTRAP;
    let offset = bootstrap.rfind("})();").unwrap();
    let mut instrumented = String::with_capacity(bootstrap.len() + 128);
    instrumented.push_str(&bootstrap[..offset]);
    instrumented
        .push_str("globalThis.__ilium_contract_world_region_result = world_region_result;\n");
    instrumented.push_str(&bootstrap[offset..]);
    engine(
        "export function plan(){return {}} export async function create(){return {render(){},dispose(){}}}",
        &instrumented,
        EngineLimits::default(),
        quota,
    )
}

#[test]
fn actual_v8_complete_region_accepts_sdk_shape_and_refuses_incomplete_volumes() {
    let (_serial, quota) = fixture_lock();
    let mut native = region_engine(quota);
    let result = native
        .evaluate_json(
            r#"
        (() => {
            const identity = "b".repeat(64);
            function complete() {
                return {origin:[-1,0,0],size:[2,1,1],blocks:new Uint16Array([0,1]),
                    palette:[{name:"minecraft:water",properties:{level:"7"}},
                             {name:"minecraft:stone",properties:{}}],identity};
            }
            function accepts(value) {
                try { __ilium_contract_world_region_result(value,{identity}); return true; }
                catch (_) { return false; }
            }
            return [
                accepts(complete()),
                accepts({blocks:new Uint16Array([0,1]),identity}),
                accepts({...complete(),size:[3,1,1]}),
                accepts({...complete(),blocks:new Uint16Array([0,2])}),
                accepts({...complete(),origin:[0.5,0,0]}),
                accepts({...complete(),palette:[{name:"minecraft:water"},
                    {name:"minecraft:stone",properties:{}}]}),
                accepts({...complete(),origin:[2147483647,0,0]}),
                accepts({...complete(),identity:"a".repeat(64)})
            ];
        })()
    "#,
        )
        .unwrap();
    assert_eq!(
        result,
        serde_json::json!([true, false, false, false, false, false, false, false])
    );
    assert!(!native.is_invalid());
}

#[test]
fn actual_v8_region_retains_binary_identity_and_freezes_semantic_metadata_without_getters() {
    let (_serial, quota) = fixture_lock();
    let mut native = region_engine(quota);
    let result = native.evaluate_json(r#"
        (() => {
            const identity="b".repeat(64), blocks=new Uint16Array([0,1]);
            const source={origin:[-1,0,0],size:[2,1,1],blocks,
                palette:[{name:"minecraft:water",properties:{level:"7"}},
                         {name:"minecraft:stone",properties:{}}],identity};
            const region=__ilium_contract_world_region_result(source,{identity});
            let traps=0, refused=false;
            const accessor={...source};
            Object.defineProperty(accessor,"origin",{enumerable:true,get(){traps++;return [-1,0,0]}});
            try { __ilium_contract_world_region_result(accessor,{identity}); } catch (_) { refused=true; }
            return {sameBinary:region.blocks===blocks,
                fields:Object.keys(region).sort(),
                origin:region.origin,size:region.size,level:region.palette[0].properties.level,
                frozen:[region,region.origin,region.size,region.palette,
                    region.palette[0],region.palette[0].properties].every(Object.isFrozen),
                traps,refused};
        })()
    "#).unwrap();
    assert_eq!(result["sameBinary"], true);
    assert_eq!(
        result["fields"],
        serde_json::json!(["blocks", "identity", "origin", "palette", "size"])
    );
    assert_eq!(result["origin"], serde_json::json!([-1, 0, 0]));
    assert_eq!(result["size"], serde_json::json!([2, 1, 1]));
    assert_eq!(result["level"], "7");
    assert_eq!(result["frozen"], true);
    assert_eq!(result["traps"], 0);
    assert_eq!(result["refused"], true);
    assert!(!native.is_invalid());
}
