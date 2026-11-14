//! Approximate heliocentric positions of the eight planets from the JPL "Keplerian elements
//! for approximate positions" (Standish, 1800-2050), as the Solar system scene draws them.
//! The elements and the Kepler solver are copied from the observer-sky ephemeris of the host
//! (`ilium-ambient` stars scene), which keeps its own copy for the Stars scene.

/// Rectangular position in au.
pub type Vec3 = [f64; 3];

/// Julian date of J2000.0.
const J2000_JD: f64 = 2_451_545.0;

fn julian_centuries(jd: f64) -> f64 {
    (jd - J2000_JD) / 36_525.0
}

/// The five planets whose elements are shared with the Earth-Moon barycentre row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Planet {
    Mercury,
    Venus,
    Mars,
    Jupiter,
    Saturn,
}

/// Standish Table 1 (1800-2050): value at J2000 and rate per Julian century for
/// a (au), e, I (deg), L (deg), longitude of perihelion (deg), node (deg).
struct Elements {
    values: [f64; 6],
    rates: [f64; 6],
}

const EARTH_MOON_BARYCENTER: Elements = Elements {
    values: [
        1.000_002_61,
        0.016_711_23,
        -0.000_015_31,
        100.464_571_66,
        102.937_681_93,
        0.0,
    ],
    rates: [
        0.000_005_62,
        -0.000_043_92,
        -0.012_946_68,
        35_999.372_449_81,
        0.323_273_64,
        0.0,
    ],
};

fn planet_elements(planet: Planet) -> Elements {
    match planet {
        Planet::Mercury => Elements {
            values: [
                0.387_099_27,
                0.205_635_93,
                7.004_979_02,
                252.250_323_50,
                77.457_796_28,
                48.330_765_93,
            ],
            rates: [
                0.000_000_37,
                0.000_019_06,
                -0.005_947_49,
                149_472.674_111_75,
                0.160_476_89,
                -0.125_340_81,
            ],
        },
        Planet::Venus => Elements {
            values: [
                0.723_335_66,
                0.006_776_72,
                3.394_676_05,
                181.979_099_50,
                131.602_467_18,
                76.679_842_55,
            ],
            rates: [
                0.000_003_90,
                -0.000_041_07,
                -0.000_788_90,
                58_517.815_387_29,
                0.002_683_29,
                -0.277_694_18,
            ],
        },
        Planet::Mars => Elements {
            values: [
                1.523_710_34,
                0.093_394_10,
                1.849_691_42,
                -4.553_432_05,
                -23.943_629_59,
                49.559_538_91,
            ],
            rates: [
                0.000_018_47,
                0.000_078_82,
                -0.008_131_31,
                19_140.302_684_99,
                0.444_410_88,
                -0.292_573_43,
            ],
        },
        Planet::Jupiter => Elements {
            values: [
                5.202_887_00,
                0.048_386_24,
                1.304_396_95,
                34.396_440_51,
                14.728_479_83,
                100.473_909_09,
            ],
            rates: [
                -0.000_116_07,
                -0.000_132_53,
                -0.001_837_14,
                3_034.746_127_75,
                0.212_526_68,
                0.204_691_06,
            ],
        },
        Planet::Saturn => Elements {
            values: [
                9.536_675_94,
                0.053_861_79,
                2.485_991_87,
                49.954_244_23,
                92.598_878_31,
                113.662_424_48,
            ],
            rates: [
                -0.001_250_60,
                -0.000_509_91,
                0.001_936_09,
                1_222.493_622_01,
                -0.418_972_16,
                -0.288_677_94,
            ],
        },
    }
}

/// Eight planets in conventional order, using the same JPL solver as the
/// observer sky. Earth uses the Earth-Moon barycenter approximation.
fn solar_elements(index: usize) -> Option<Elements> {
    Some(match index {
        0 => planet_elements(Planet::Mercury),
        1 => planet_elements(Planet::Venus),
        2 => EARTH_MOON_BARYCENTER,
        3 => planet_elements(Planet::Mars),
        4 => planet_elements(Planet::Jupiter),
        5 => planet_elements(Planet::Saturn),
        6 => Elements {
            values: [
                19.18916464,
                0.04725744,
                0.77263783,
                313.23810451,
                170.95427630,
                74.01692503,
            ],
            rates: [
                -0.00196176,
                -0.00004397,
                -0.00242939,
                428.48202785,
                0.40805281,
                0.04240589,
            ],
        },
        7 => Elements {
            values: [
                30.06992276,
                0.00859048,
                1.77004347,
                -55.12002969,
                44.96476227,
                131.78422574,
            ],
            rates: [
                0.00026291,
                0.00005105,
                0.00035372,
                218.45945325,
                -0.32241464,
                -0.00508664,
            ],
        },
        _ => return None,
    })
}

pub fn solar_heliocentric(index: usize, jd: f64) -> Vec3 {
    solar_elements(index).map_or([0.0; 3], |elements| heliocentric(&elements, jd))
}

/// Closed path at fixed J2000 orbital elements, rather than advancing centuries
/// to sample Neptune and accidentally introducing secular element drift.
pub fn solar_orbit_position(index: usize, fraction: f64) -> Vec3 {
    let Some(mut elements) = solar_elements(index) else {
        return [0.0; 3];
    };
    elements.values[3] = elements.values[4] + fraction * 360.0;
    elements.rates = [0.0; 6];
    heliocentric(&elements, 2451545.0)
}

/// Heliocentric ecliptic (J2000) rectangular position in au.
fn heliocentric(elements: &Elements, jd: f64) -> Vec3 {
    let t = julian_centuries(jd);
    let value = |index: usize| elements.values[index] + elements.rates[index] * t;
    let (a, e) = (value(0), value(1));
    let inclination = value(2).to_radians();
    let mean_longitude = value(3);
    let perihelion_longitude = value(4);
    let node = value(5).to_radians();
    let argument = perihelion_longitude.to_radians() - node;
    let mean_anomaly = (mean_longitude - perihelion_longitude + 180.0).rem_euclid(360.0) - 180.0;
    let mean_anomaly = mean_anomaly.to_radians();
    let mut eccentric = mean_anomaly + e * mean_anomaly.sin();
    for _ in 0..12 {
        let delta = (eccentric - e * eccentric.sin() - mean_anomaly) / (1.0 - e * eccentric.cos());
        eccentric -= delta;
        if delta.abs() < 1e-12 {
            break;
        }
    }
    let x_orbit = a * (eccentric.cos() - e);
    let y_orbit = a * (1.0 - e * e).sqrt() * eccentric.sin();
    let (sin_w, cos_w) = argument.sin_cos();
    let (sin_n, cos_n) = node.sin_cos();
    let (sin_i, cos_i) = inclination.sin_cos();
    [
        (cos_w * cos_n - sin_w * sin_n * cos_i) * x_orbit
            + (-sin_w * cos_n - cos_w * sin_n * cos_i) * y_orbit,
        (cos_w * sin_n + sin_w * cos_n * cos_i) * x_orbit
            + (-sin_w * sin_n + cos_w * cos_n * cos_i) * y_orbit,
        sin_w * sin_i * x_orbit + cos_w * sin_i * y_orbit,
    ]
}
