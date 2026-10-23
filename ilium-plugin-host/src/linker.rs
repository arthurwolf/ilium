//! Linker construction per world and grant set (reference-host-runtime.md, ABI-023).

use ilium_plugin_api::grants::Grant;

use crate::error::HostError;

/// The three worlds a component targets (reference-wit.md section 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum World {
    /// `ilium:server`.
    Server,
    /// `ilium:client`.
    Client,
    /// `ilium:anim`.
    Anim,
}

impl World {
    /// The WIT world name (reference-wit.md section 9).
    pub fn wit_name(self) -> &'static str {
        match self {
            World::Server => "ilium:server",
            World::Client => "ilium:client",
            World::Anim => "ilium:anim",
        }
    }
}

/// A linker with the imports the grants allow; unrequested imports trap with
/// `trap.import-not-requested` (reference-host-runtime.md).
#[derive(Debug)]
pub struct PluginLinker {
    /// The world this linker serves.
    pub world: World,
}

/// Builds the linker for a world and the granted capabilities (reference-host-runtime.md).
pub fn build_linker(_world: World, _grants: &[Grant]) -> Result<PluginLinker, HostError> {
    unimplemented!("host::linker::build_linker")
}

/// The cache key of a linker: side, tier and the sorted grants (reference-host-runtime.md HST-06x).
pub fn linker_cache_key(
    _side: ilium_plugin_api::Side,
    _tier: ilium_plugin_api::Tier,
    _grants: &[Grant],
) -> String {
    unimplemented!("host::linker::linker_cache_key")
}

#[cfg(test)]
mod tests {
    use ilium_plugin_api::grants::Grant;
    use ilium_plugin_api::{Side, Tier};

    use super::{build_linker, linker_cache_key, World};

    /// Parses a grant string through the API crate's documented constructor.
    fn grant(text: &str) -> Grant {
        Grant::parse(text).expect("the grant string is a valid grant of the API table")
    }

    /// REQ: PKG-041
    #[test]
    fn wit_name_of_each_world_is_its_package_name() {
        assert_eq!(World::Server.wit_name(), "ilium:server");
        assert_eq!(World::Client.wit_name(), "ilium:client");
        assert_eq!(World::Anim.wit_name(), "ilium:anim");
    }

    /// REQ: PKG-041
    #[test]
    fn wit_names_of_the_three_worlds_are_distinct() {
        let names = [
            World::Server.wit_name(),
            World::Client.wit_name(),
            World::Anim.wit_name(),
        ];
        assert_ne!(names[0], names[1]);
        assert_ne!(names[1], names[2]);
        assert_ne!(names[0], names[2]);
    }

    /// REQ: HST-071
    #[test]
    fn build_linker_with_empty_grants_succeeds_and_keeps_world() {
        let linker =
            build_linker(World::Server, &[]).expect("an empty grant set is a valid linker input");
        assert_eq!(linker.world, World::Server);
    }

    /// REQ: HST-070
    #[test]
    fn build_linker_records_the_requested_world_for_every_world() {
        let grants = [grant("tree.read")];
        for world in [World::Server, World::Client, World::Anim] {
            let linker = build_linker(world, &grants).expect("a valid grant set builds a linker");
            assert_eq!(linker.world, world);
        }
    }

    /// REQ: HST-070
    #[test]
    fn cache_key_is_the_same_for_the_same_inputs() {
        let grants = [grant("tree.read"), grant("http:api.example.com")];
        assert_eq!(
            linker_cache_key(Side::Server, Tier::T1, &grants),
            linker_cache_key(Side::Server, Tier::T1, &grants)
        );
    }

    /// REQ: HST-070
    #[test]
    fn cache_key_does_not_depend_on_grant_order() {
        let forward = [grant("tree.read"), grant("http:api.example.com")];
        let reversed = [grant("http:api.example.com"), grant("tree.read")];
        assert_eq!(
            linker_cache_key(Side::Client, Tier::T2, &forward),
            linker_cache_key(Side::Client, Tier::T2, &reversed)
        );
    }

    /// REQ: HST-070
    #[test]
    fn equal_grant_sets_on_equal_sides_share_a_key() {
        let first_plugin = [grant("tree.read")];
        let second_plugin = [grant("tree.read")];
        assert_eq!(
            linker_cache_key(Side::Server, Tier::T3, &first_plugin),
            linker_cache_key(Side::Server, Tier::T3, &second_plugin)
        );
    }

    /// REQ: HST-070
    #[test]
    fn cache_key_differs_when_the_side_differs() {
        let grants = [grant("tree.read")];
        let server_key = linker_cache_key(Side::Server, Tier::T1, &grants);
        let client_key = linker_cache_key(Side::Client, Tier::T1, &grants);
        let scene_key = linker_cache_key(Side::Scene, Tier::T1, &grants);
        assert_ne!(server_key, client_key);
        assert_ne!(server_key, scene_key);
        assert_ne!(client_key, scene_key);
    }

    /// REQ: HST-070
    #[test]
    fn cache_key_differs_when_the_tier_differs() {
        let grants = [grant("tree.read")];
        let key_t1 = linker_cache_key(Side::Server, Tier::T1, &grants);
        let key_t2 = linker_cache_key(Side::Server, Tier::T2, &grants);
        let key_t3 = linker_cache_key(Side::Server, Tier::T3, &grants);
        assert_ne!(key_t1, key_t2);
        assert_ne!(key_t1, key_t3);
        assert_ne!(key_t2, key_t3);
    }

    /// REQ: HST-070
    #[test]
    fn cache_key_differs_when_the_grant_set_differs() {
        let smaller = [grant("tree.read")];
        let larger = [grant("tree.read"), grant("http:api.example.com")];
        assert_ne!(
            linker_cache_key(Side::Server, Tier::T1, &smaller),
            linker_cache_key(Side::Server, Tier::T1, &larger)
        );
    }

    /// REQ: HST-071
    #[test]
    fn cache_key_accepts_an_empty_grant_set_and_is_stable() {
        let first = linker_cache_key(Side::Server, Tier::T1, &[]);
        let second = linker_cache_key(Side::Server, Tier::T1, &[]);
        assert_eq!(first, second);
        assert_eq!(
            first.len(),
            64,
            "an empty grant set still yields a SHA-256 digest, got {first:?}"
        );
        assert_ne!(
            first,
            linker_cache_key(Side::Server, Tier::T1, &[grant("tree.read")])
        );
    }

    /// REQ: HST-070
    #[test]
    fn cache_key_is_a_sha256_hex_digest() {
        let key = linker_cache_key(Side::Server, Tier::T1, &[grant("tree.read")]);
        assert_eq!(
            key.len(),
            64,
            "a SHA-256 digest is 64 hexadecimal characters, got {key:?}"
        );
        assert!(
            key.chars()
                .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase()),
            "the key must be lowercase hexadecimal, got {key:?}"
        );
    }
}
