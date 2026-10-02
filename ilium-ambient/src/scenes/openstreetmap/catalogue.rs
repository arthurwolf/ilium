//! Finite embedded real-OSM catalogue. No network is needed for world tours.
pub struct Place {
    pub name: &'static str,
    pub latitude: f64,
    pub longitude: f64,
    pub data: &'static [u8],
}
pub const PLACES: [Place; 10] = [
    Place {
        name: "Paris",
        latitude: 48.8584,
        longitude: 2.2945,
        data: include_bytes!("../../../assets/openstreetmap/paris.compact.json.gz"),
    },
    Place {
        name: "London",
        latitude: 51.5074,
        longitude: -0.1278,
        data: include_bytes!("../../../assets/openstreetmap/london.compact.json.gz"),
    },
    Place {
        name: "Venice",
        latitude: 45.434,
        longitude: 12.338,
        data: include_bytes!("../../../assets/openstreetmap/venice.compact.json.gz"),
    },
    Place {
        name: "New York",
        latitude: 40.7128,
        longitude: -74.006,
        data: include_bytes!("../../../assets/openstreetmap/new_york.compact.json.gz"),
    },
    Place {
        name: "Tokyo",
        latitude: 35.6812,
        longitude: 139.7671,
        data: include_bytes!("../../../assets/openstreetmap/tokyo.compact.json.gz"),
    },
    Place {
        name: "Cape Town",
        latitude: -33.9249,
        longitude: 18.4241,
        data: include_bytes!("../../../assets/openstreetmap/cape_town.compact.json.gz"),
    },
    Place {
        name: "Sydney",
        latitude: -33.8568,
        longitude: 151.2153,
        data: include_bytes!("../../../assets/openstreetmap/sydney.compact.json.gz"),
    },
    Place {
        name: "Rio de Janeiro",
        latitude: -22.9068,
        longitude: -43.1729,
        data: include_bytes!("../../../assets/openstreetmap/rio.compact.json.gz"),
    },
    Place {
        name: "Singapore",
        latitude: 1.2838,
        longitude: 103.8591,
        data: include_bytes!("../../../assets/openstreetmap/singapore.compact.json.gz"),
    },
    Place {
        name: "Reykjavik",
        latitude: 64.1466,
        longitude: -21.9426,
        data: include_bytes!("../../../assets/openstreetmap/reykjavik.compact.json.gz"),
    },
];
