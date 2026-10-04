//! Static viewing anchors, NOT new OSM geometry or a geocoder.
//! The ten bundle indices and centres remain owned by `catalogue::PLACES`.
//! Other anchors are transcribed from Wikidata P625 (CC0), 2026-10-03.
//! See `place-provenance.json` for the displayed DMS values and conversion.
use super::catalogue::PLACES;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Destination {
    pub id: &'static str,
    pub label: &'static str,
    /// WGS84 [latitude, longitude]; an approximate viewing anchor, not a boundary.
    pub center: [f64; 2],
    pub bundle_index: Option<usize>,
    pub coordinate_source: &'static str,
}

macro_rules! bundled {
    ($id:literal, $index:literal) => {
        Destination {
            id: $id,
            label: PLACES[$index].name,
            center: [PLACES[$index].latitude, PLACES[$index].longitude],
            bundle_index: Some($index),
            coordinate_source: "ilium-ambient/assets/openstreetmap/catalogue.json",
        }
    };
}

/// Never renumber a bundle to insert a new destination. Persist destination IDs.
pub const DESTINATIONS: &[Destination] = &[
    bundled!("paris", 0),
    bundled!("london", 1),
    bundled!("venice", 2),
    bundled!("new-york", 3),
    bundled!("tokyo", 4),
    bundled!("cape-town", 5),
    bundled!("sydney", 6),
    bundled!("rio-de-janeiro", 7),
    bundled!("singapore", 8),
    bundled!("reykjavik", 9),
    Destination {
        id: "bruges",
        label: "Bruges, Belgium",
        center: [51.2088889, 3.2241667],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q12994#P625",
    },
    Destination {
        id: "dubrovnik",
        label: "Dubrovnik, Croatia",
        center: [42.6402778, 18.1083333],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q1722#P625",
    },
    Destination {
        id: "tallinn",
        label: "Tallinn, Estonia",
        center: [59.4372222, 24.7450000],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q1770#P625",
    },
    Destination {
        id: "quebec-city",
        label: "Quebec City, Canada",
        center: [46.8161111, -71.2241694],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q2145#P625",
    },
    Destination {
        id: "taj-mahal",
        label: "Taj Mahal, Agra",
        center: [27.1750000, 78.0419444],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q9141#P625",
    },
    Destination {
        id: "athens-acropolis",
        label: "Acropolis, Athens",
        center: [37.9716667, 23.7261111],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q131013#P625",
    },
    Destination {
        id: "cologne-cathedral",
        label: "Cologne Cathedral",
        center: [50.9413889, 6.9583333],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q4176#P625",
    },
    Destination {
        id: "mont-saint-michel",
        label: "Mont-Saint-Michel",
        center: [48.6358333, -1.5102778],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q20892#P625",
    },
    Destination {
        id: "porto",
        label: "Porto, Portugal",
        center: [41.1500000, -8.6108333],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q36433#P625",
    },
    Destination {
        id: "willemstad",
        label: "Willemstad, Curacao",
        center: [12.1080556, -68.9350000],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q132679#P625",
    },
    Destination {
        id: "valparaiso",
        label: "Valparaiso, Chile",
        center: [-33.0461111, -71.6197222],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q33986#P625",
    },
    Destination {
        id: "central-park",
        label: "Central Park, New York",
        center: [40.7825000, -73.9661111],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q160409#P625",
    },
    Destination {
        id: "golden-gate-park",
        label: "Golden Gate Park, San Francisco",
        center: [37.7697222, -122.4769444],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q635559#P625",
    },
    Destination {
        id: "philadelphia",
        label: "Philadelphia street grid",
        center: [39.9527778, -75.1636111],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q1345#P625",
    },
    Destination {
        id: "chicago",
        label: "Chicago street grid",
        center: [41.8819444, -87.6277778],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q1297#P625",
    },
    Destination {
        id: "la-plata",
        label: "La Plata grid and diagonals",
        center: [-34.9333333, -57.9500000],
        bundle_index: None,
        coordinate_source: "https://www.wikidata.org/wiki/Q44059#P625",
    },
];

#[derive(Debug, Clone, Copy)]
pub struct PlaceList {
    pub id: &'static str,
    pub label: &'static str,
    pub members: &'static [&'static str],
}

pub const OFFLINE_LIST_ID: &str = "offline-world";
pub const BUNDLED_IDS: &[&str] = &[
    "paris",
    "london",
    "venice",
    "new-york",
    "tokyo",
    "cape-town",
    "sydney",
    "rio-de-janeiro",
    "singapore",
    "reykjavik",
];

pub const LISTS: &[PlaceList] = &[
    PlaceList {
        id: "offline-world",
        label: "Offline world (10 extracts)",
        members: BUNDLED_IDS,
    },
    PlaceList {
        id: "offline-europe",
        label: "Offline Europe",
        members: &["paris", "london", "venice", "reykjavik"],
    },
    PlaceList {
        id: "offline-asia-pacific",
        label: "Offline Asia-Pacific",
        members: &["tokyo", "singapore", "sydney"],
    },
    PlaceList {
        id: "offline-coastal",
        label: "Offline coastal cities",
        members: &[
            "venice",
            "cape-town",
            "sydney",
            "rio-de-janeiro",
            "singapore",
            "reykjavik",
        ],
    },
    PlaceList {
        id: "offline-americas",
        label: "Offline Americas",
        members: &["new-york", "rio-de-janeiro"],
    },
    PlaceList {
        id: "historic-towns",
        label: "Historic towns and centres",
        members: &["bruges", "dubrovnik", "tallinn", "quebec-city"],
    },
    PlaceList {
        id: "landmarks",
        label: "Landmarks and monuments",
        members: &[
            "taj-mahal",
            "athens-acropolis",
            "cologne-cathedral",
            "mont-saint-michel",
        ],
    },
    PlaceList {
        id: "harbours",
        label: "Harbours and coastal cities",
        members: &["porto", "willemstad", "valparaiso"],
    },
    PlaceList {
        id: "parks",
        label: "Urban parks",
        members: &["central-park", "golden-gate-park"],
    },
    PlaceList {
        id: "street-grids",
        label: "Street grids and diagonals",
        members: &["philadelphia", "chicago", "la-plata"],
    },
    PlaceList {
        id: "world-sampler",
        label: "World sampler (all destinations)",
        members: &[
            "paris",
            "london",
            "venice",
            "new-york",
            "tokyo",
            "cape-town",
            "sydney",
            "rio-de-janeiro",
            "singapore",
            "reykjavik",
            "bruges",
            "dubrovnik",
            "tallinn",
            "quebec-city",
            "taj-mahal",
            "athens-acropolis",
            "cologne-cathedral",
            "mont-saint-michel",
            "porto",
            "willemstad",
            "valparaiso",
            "central-park",
            "golden-gate-park",
            "philadelphia",
            "chicago",
            "la-plata",
        ],
    },
];

pub fn destination(id: &str) -> Option<&'static Destination> {
    DESTINATIONS.iter().find(|place| place.id == id)
}

pub fn list(id: &str) -> Option<&'static PlaceList> {
    LISTS.iter().find(|list| list.id == id)
}

pub fn offline_lists() -> impl Iterator<Item = &'static PlaceList> {
    LISTS.iter().filter(|list| list.is_bundled())
}

impl PlaceList {
    pub fn is_bundled(&self) -> bool {
        !self.members.is_empty()
            && self
                .members
                .iter()
                .all(|id| destination(id).is_some_and(|place| place.bundle_index.is_some()))
    }
    pub fn destination(&self, index: usize) -> Result<&'static Destination, String> {
        let id = self
            .members
            .get(index)
            .ok_or("OSM destination index is outside the chosen list")?;
        destination(id).ok_or_else(|| format!("Unknown OSM destination ID: {id}"))
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.members.iter().position(|member| *member == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn bundle_ids_names_centres_and_order_are_unchanged() {
        assert_eq!(BUNDLED_IDS.len(), PLACES.len());
        for (index, (id, original)) in BUNDLED_IDS.iter().zip(PLACES.iter()).enumerate() {
            let destination = destination(id).unwrap();
            assert_eq!(destination.bundle_index, Some(index));
            assert_eq!(destination.label, original.name);
            assert_eq!(destination.center, [original.latitude, original.longitude]);
        }
    }

    #[test]
    fn lists_have_unique_stable_members_and_real_bounded_anchors() {
        let mut ids = HashSet::new();
        for place in DESTINATIONS {
            assert!(ids.insert(place.id));
            assert!(!place.label.is_empty());
            assert!(place.center.iter().all(|n| n.is_finite()));
            assert!((-85.0..=85.0).contains(&place.center[0]));
            assert!((-180.0..=180.0).contains(&place.center[1]));
            assert!(!place.coordinate_source.is_empty());
        }
        let mut list_ids = HashSet::new();
        for list in LISTS {
            assert!(list_ids.insert(list.id));
            assert!(!list.members.is_empty());
            let mut members = HashSet::new();
            for (index, id) in list.members.iter().enumerate() {
                assert!(members.insert(*id));
                assert_eq!(list.destination(index).unwrap().id, *id);
                assert_eq!(list.index_of(id), Some(index));
            }
            assert!(list.destination(list.members.len()).is_err());
        }
        assert_eq!(destination("not-a-place"), None);
        assert!(list("not-a-list").is_none());
        assert!(LISTS.iter().any(|list| list.members.len() > PLACES.len()));
    }

    #[test]
    fn each_new_theme_adds_destinations_not_renamed_offline_subsets() {
        for id in [
            "historic-towns",
            "landmarks",
            "harbours",
            "parks",
            "street-grids",
        ] {
            let list = list(id).unwrap();
            assert!(list.members.len() >= 2);
            assert!(list
                .members
                .iter()
                .all(|id| destination(id).unwrap().bundle_index.is_none()));
        }
        assert_eq!(
            DESTINATIONS
                .iter()
                .filter(|place| place.bundle_index.is_none())
                .count(),
            16
        );
    }

    #[test]
    fn every_offline_theme_contains_only_existing_bundle_ids() {
        let lists: Vec<_> = offline_lists().collect();
        assert!(lists.len() >= 4);
        for list in lists {
            assert!(list.is_bundled(), "{}", list.id);
            for id in list.members {
                let place = destination(id).unwrap();
                let bundle = place.bundle_index.unwrap();
                assert_eq!(PLACES[bundle].name, place.label);
            }
        }
        assert!(!list("historic-towns").unwrap().is_bundled());
    }
}
