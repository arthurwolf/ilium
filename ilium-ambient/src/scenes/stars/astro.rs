//! Positional astronomy for the star map: time scales, sidereal time,
//! coordinate transforms, precession and low-precision Sun, Moon and planet
//! ephemerides. Pure functions, no clock, no I/O.
//!
//! References:
//! * Meeus, "Astronomical Algorithms" 2nd ed.: ch. 7 (Julian day), ch. 12
//!   (sidereal time), ch. 21 (precession), ch. 13 (coordinate transforms).
//! * Standish, "Keplerian Elements for Approximate Positions of the Major
//!   Planets" (JPL, valid 1800-2050): <https://ssd.jpl.nasa.gov/planets/approx_pos.html>
//!   (public domain US government data).
//! * Moon: the low-precision series of the Astronomical Almanac ("Low-precision
//!   formulae for the Moon", accuracy about 0.3 degree).

pub const J2000_JD: f64 = 2_451_545.0;
/// Sidereal degrees gained per solar day.
pub const SIDEREAL_DEGREES_PER_DAY: f64 = 360.985_647_366_29;

pub type Vec3 = [f64; 3];
pub type Mat3 = [[f64; 3]; 3];

pub fn dot(a: &Vec3, b: &Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: &Vec3, b: &Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn normalize(v: Vec3) -> Vec3 {
    let length = dot(&v, &v).sqrt();
    if length < 1e-12 {
        return [0.0, 0.0, 1.0];
    }
    [v[0] / length, v[1] / length, v[2] / length]
}

pub fn mat_vec(matrix: &Mat3, v: &Vec3) -> Vec3 {
    [dot(&matrix[0], v), dot(&matrix[1], v), dot(&matrix[2], v)]
}

pub fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (column, cell) in out_row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[row][k] * b[k][column]).sum();
        }
    }
    out
}

pub const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Unit vector of an equatorial position, degrees.
pub fn equatorial_vector(ra_deg: f64, dec_deg: f64) -> Vec3 {
    let (ra, dec) = (ra_deg.to_radians(), dec_deg.to_radians());
    [dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()]
}

#[cfg(test)]
/// Right ascension (0..360) and declination of a vector.
pub fn vector_to_equatorial(v: &Vec3) -> (f64, f64) {
    let length = dot(v, v).sqrt().max(1e-15);
    (
        v[1].atan2(v[0]).to_degrees().rem_euclid(360.0),
        (v[2] / length).clamp(-1.0, 1.0).asin().to_degrees(),
    )
}

/// Julian date from Unix seconds (UTC; leap seconds and UT1-UTC ignored).
pub fn julian_date(unix_seconds: f64) -> f64 {
    2_440_587.5 + unix_seconds / 86_400.0
}

#[cfg(test)]
pub fn unix_seconds_from_julian_date(jd: f64) -> f64 {
    (jd - 2_440_587.5) * 86_400.0
}

pub fn julian_centuries(jd: f64) -> f64 {
    (jd - J2000_JD) / 36_525.0
}

/// Greenwich mean sidereal time in degrees, 0..360 (Meeus 12.4).
pub fn gmst_degrees(jd: f64) -> f64 {
    let t = julian_centuries(jd);
    (280.460_618_37 + SIDEREAL_DEGREES_PER_DAY * (jd - J2000_JD) + 0.000_387_933 * t * t
        - t * t * t / 38_710_000.0)
        .rem_euclid(360.0)
}

/// Local mean sidereal time in degrees for an east-positive longitude.
pub fn local_sidereal_degrees(jd: f64, longitude_east_deg: f64) -> f64 {
    (gmst_degrees(jd) + longitude_east_deg).rem_euclid(360.0)
}

#[cfg(test)]
/// Altitude and azimuth (degrees; azimuth from north through east) of an
/// equatorial position for an observer at `latitude_deg` with local sidereal
/// time `lst_deg`. Uses the true-of-date equator; refraction is not applied.
pub fn equatorial_to_horizontal(
    ra_deg: f64,
    dec_deg: f64,
    latitude_deg: f64,
    lst_deg: f64,
) -> (f64, f64) {
    let matrix = horizon_matrix(latitude_deg, lst_deg);
    let enu = mat_vec(&matrix, &equatorial_vector(ra_deg, dec_deg));
    enu_to_altitude_azimuth(&enu)
}

#[cfg(test)]
pub fn enu_to_altitude_azimuth(enu: &Vec3) -> (f64, f64) {
    let altitude = enu[2].clamp(-1.0, 1.0).asin().to_degrees();
    let azimuth = enu[0].atan2(enu[1]).to_degrees().rem_euclid(360.0);
    (altitude, azimuth)
}

/// Unit vector (east, north, up) of an altitude/azimuth pair.
pub fn altitude_azimuth_vector(altitude_deg: f64, azimuth_deg: f64) -> Vec3 {
    let (altitude, azimuth) = (altitude_deg.to_radians(), azimuth_deg.to_radians());
    [
        altitude.cos() * azimuth.sin(),
        altitude.cos() * azimuth.cos(),
        altitude.sin(),
    ]
}

/// Rotation from true-of-date equatorial coordinates to the local horizon
/// frame with axes (east, north, up), a right-handed system.
pub fn horizon_matrix(latitude_deg: f64, lst_deg: f64) -> Mat3 {
    let (lst_sin, lst_cos) = lst_deg.to_radians().sin_cos();
    let (lat_sin, lat_cos) = latitude_deg.to_radians().sin_cos();
    // Local equatorial frame: meridian point, east, pole.
    let meridian = [lst_cos, lst_sin, 0.0];
    let east = [-lst_sin, lst_cos, 0.0];
    let pole = [0.0, 0.0, 1.0];
    let combine = |a: f64, u: &Vec3, b: f64, v: &Vec3| -> Vec3 {
        [
            a * u[0] + b * v[0],
            a * u[1] + b * v[1],
            a * u[2] + b * v[2],
        ]
    };
    let north = combine(lat_cos, &pole, -lat_sin, &meridian);
    let up = combine(lat_sin, &pole, lat_cos, &meridian);
    [east, north, up]
}

/// Precession from J2000.0 mean equator to the mean equator of date
/// (IAU 1976, Meeus ch. 21).
pub fn precession_matrix(jd: f64) -> Mat3 {
    let t = julian_centuries(jd);
    let arcsecond = std::f64::consts::PI / 180.0 / 3600.0;
    let zeta = (2306.2181 * t + 0.30188 * t * t + 0.017998 * t * t * t) * arcsecond;
    let z = (2306.2181 * t + 1.09468 * t * t + 0.018203 * t * t * t) * arcsecond;
    let theta = (2004.3109 * t - 0.42665 * t * t - 0.041833 * t * t * t) * arcsecond;
    let rot_z = |angle: f64| -> Mat3 {
        let (s, c) = angle.sin_cos();
        [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
    };
    let (st, ct) = theta.sin_cos();
    let rot_y: Mat3 = [[ct, 0.0, -st], [0.0, 1.0, 0.0], [st, 0.0, ct]];
    mat_mul(&rot_z(z), &mat_mul(&rot_y, &rot_z(zeta)))
}

/// Mean obliquity of the ecliptic of date, degrees.
pub fn obliquity_degrees(jd: f64) -> f64 {
    23.439_291 - 0.013_004_2 * julian_centuries(jd)
}

/// Galactic pole and centre as J2000 unit vectors (IAU 1958 system).
pub fn galactic_axes() -> (Vec3, Vec3) {
    let pole = equatorial_vector(192.859_48, 27.128_25);
    let centre = equatorial_vector(266.405_10, -28.936_18);
    (pole, centre)
}

// ---------------------------------------------------------------- calendar

/// Days since 1970-01-01 of a proleptic Gregorian date.
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Inverse of `days_from_civil`: (year, month, day).
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Unix seconds of a UTC civil date-time.
pub fn unix_from_civil(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> f64 {
    (days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second) as f64
}

/// "2026-09-30 21:04:10" for Unix seconds (UTC).
pub fn format_utc(unix_seconds: f64) -> String {
    let whole = unix_seconds.floor() as i64;
    let (year, month, day) = civil_from_days(whole.div_euclid(86_400));
    let seconds_of_day = whole.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        seconds_of_day / 3600,
        seconds_of_day % 3600 / 60,
        seconds_of_day % 60
    )
}

/// Parse "YYYY-MM-DD", "YYYY-MM-DD HH:MM" or "YYYY-MM-DD HH:MM:SS" (UTC; a
/// `T` separator and a trailing `Z` are accepted) into Unix seconds.
pub fn parse_utc(text: &str) -> Option<f64> {
    let text = text.trim().trim_end_matches(['Z', 'z']).trim();
    let (date, time) = match text.split_once(['T', ' ']) {
        Some((date, time)) => (date, time.trim()),
        None => (text, ""),
    };
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }
    let mut time_parts = time.split(':');
    let hour: i64 = match time_parts.next() {
        Some("") | None => 0,
        Some(part) => part.parse().ok()?,
    };
    let minute: i64 = time_parts
        .next()
        .map_or(Some(0), |part| part.parse().ok())?;
    let second: i64 = time_parts
        .next()
        .map_or(Some(0), |part| part.parse().ok())?;
    if time_parts.next().is_some()
        || !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0..60).contains(&second)
    {
        return None;
    }
    let days_in_month = match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if !(1..=days_in_month).contains(&day) || !(1..=9999).contains(&year) {
        return None;
    }
    Some(unix_from_civil(year, month, day, hour, minute, second))
}

// --------------------------------------------------------------- ephemerides

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Planet {
    Mercury,
    Venus,
    Mars,
    Jupiter,
    Saturn,
}

impl Planet {
    pub const ALL: [Self; 5] = [
        Self::Mercury,
        Self::Venus,
        Self::Mars,
        Self::Jupiter,
        Self::Saturn,
    ];

    #[cfg(test)]
    pub fn name(self) -> &'static str {
        match self {
            Self::Mercury => "Mercury",
            Self::Venus => "Venus",
            Self::Mars => "Mars",
            Self::Jupiter => "Jupiter",
            Self::Saturn => "Saturn",
        }
    }
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

/// Ecliptic (J2000) rectangular to J2000 equatorial.
fn ecliptic_to_equatorial(v: &Vec3) -> Vec3 {
    let (sin_e, cos_e) = 23.439_281_f64.to_radians().sin_cos();
    [
        v[0],
        v[1] * cos_e - v[2] * sin_e,
        v[1] * sin_e + v[2] * cos_e,
    ]
}

#[derive(Debug, Clone, Copy)]
pub struct PlanetSight {
    /// J2000 equatorial unit vector, geocentric.
    pub direction: Vec3,
    pub magnitude: f64,
}

/// Geometry shared by the sight and the test-only diagnostics.
struct PlanetGeometry {
    geocentric: Vec3,
    distance: f64,
    sun_distance: f64,
    phase_deg: f64,
    #[cfg(test)]
    elongation_deg: f64,
}

fn planet_geometry(planet: Planet, jd: f64) -> PlanetGeometry {
    let earth = heliocentric(&EARTH_MOON_BARYCENTER, jd);
    let body = heliocentric(&planet_elements(planet), jd);
    let geocentric = [body[0] - earth[0], body[1] - earth[1], body[2] - earth[2]];
    let distance = dot(&geocentric, &geocentric).sqrt();
    let sun_distance = dot(&body, &body).sqrt();
    let earth_sun = dot(&earth, &earth).sqrt();
    let phase_cosine = ((sun_distance * sun_distance + distance * distance
        - earth_sun * earth_sun)
        / (2.0 * sun_distance * distance))
        .clamp(-1.0, 1.0);
    PlanetGeometry {
        geocentric,
        distance,
        sun_distance,
        phase_deg: phase_cosine.acos().to_degrees(),
        #[cfg(test)]
        elongation_deg: (dot(&geocentric, &[-earth[0], -earth[1], -earth[2]])
            / (distance * earth_sun))
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees(),
    }
}

/// Distance from the Earth in au and elongation from the Sun in degrees.
#[cfg(test)]
pub fn planet_distance_and_elongation(planet: Planet, jd: f64) -> (f64, f64) {
    let geometry = planet_geometry(planet, jd);
    (geometry.distance, geometry.elongation_deg)
}

/// Geocentric position and brightness of a planet (Standish elements; about
/// an arcminute accurate, no light-time or aberration).
pub fn planet_sight(planet: Planet, jd: f64) -> PlanetSight {
    let geometry = planet_geometry(planet, jd);
    let phase = geometry.phase_deg;
    let distance_term = 5.0 * (geometry.sun_distance * geometry.distance).log10();
    // Astronomical Almanac approximate visual magnitudes.
    let magnitude = match planet {
        Planet::Mercury => {
            -0.42 + distance_term + 0.038 * phase - 0.000_273 * phase.powi(2)
                + 0.000_002 * phase.powi(3)
        }
        Planet::Venus => {
            -4.40 + distance_term + 0.000_9 * phase + 0.000_239 * phase.powi(2)
                - 0.000_000_65 * phase.powi(3)
        }
        Planet::Mars => -1.52 + distance_term + 0.016 * phase,
        Planet::Jupiter => -9.40 + distance_term + 0.005 * phase,
        Planet::Saturn => -8.88 + distance_term,
    };
    PlanetSight {
        direction: normalize(ecliptic_to_equatorial(&geometry.geocentric)),
        magnitude,
    }
}

/// Geocentric ecliptic longitude of the Sun referred to the mean equinox of
/// date, degrees (from the Earth-Moon barycentre elements; about 0.01 degree).
pub fn sun_longitude_of_date(jd: f64) -> f64 {
    let earth = heliocentric(&EARTH_MOON_BARYCENTER, jd);
    let j2000_longitude = (-earth[1]).atan2(-earth[0]).to_degrees();
    // General precession in longitude, 1.3969713 degrees per century.
    (j2000_longitude + 1.396_971_3 * julian_centuries(jd)).rem_euclid(360.0)
}

/// The Sun as a J2000 equatorial unit vector (used for the Moon's phase).
pub fn sun_direction(jd: f64) -> Vec3 {
    let earth = heliocentric(&EARTH_MOON_BARYCENTER, jd);
    normalize(ecliptic_to_equatorial(&[-earth[0], -earth[1], -earth[2]]))
}

#[derive(Debug, Clone, Copy)]
pub struct MoonSight {
    /// Equatorial unit vector of date (geocentric; not precessed).
    pub direction: Vec3,
    pub horizontal_parallax_deg: f64,
    /// Angle Moon-Sun seen from the Earth: 0 new moon, 180 full moon.
    pub elongation_deg: f64,
}

/// Low-precision Moon (Astronomical Almanac series, about 0.3 degree).
pub fn moon_sight(jd: f64) -> MoonSight {
    let t = julian_centuries(jd);
    let sin_of = |base: f64, rate: f64| (base + rate * t).to_radians().sin();
    let cos_of = |base: f64, rate: f64| (base + rate * t).to_radians().cos();
    let longitude = 218.32 + 481_267.881 * t + 6.29 * sin_of(135.0, 477_198.87)
        - 1.27 * sin_of(259.3, -413_335.36)
        + 0.66 * sin_of(235.7, 890_534.22)
        + 0.21 * sin_of(269.9, 954_397.70)
        - 0.19 * sin_of(357.5, 35_999.05)
        - 0.11 * sin_of(186.5, 966_404.05);
    let latitude = 5.13 * sin_of(93.3, 483_202.02) + 0.28 * sin_of(228.2, 960_400.89)
        - 0.28 * sin_of(318.3, 6003.15)
        - 0.17 * sin_of(217.6, -407_332.21);
    let parallax = 0.9508
        + 0.0518 * cos_of(135.0, 477_198.87)
        + 0.0095 * cos_of(259.3, -413_335.36)
        + 0.0078 * cos_of(235.7, 890_534.22)
        + 0.0028 * cos_of(269.9, 954_397.70);
    let (lambda, beta) = (longitude.to_radians(), latitude.to_radians());
    let ecliptic = [
        beta.cos() * lambda.cos(),
        beta.cos() * lambda.sin(),
        beta.sin(),
    ];
    let (sin_e, cos_e) = obliquity_degrees(jd).to_radians().sin_cos();
    let direction = [
        ecliptic[0],
        ecliptic[1] * cos_e - ecliptic[2] * sin_e,
        ecliptic[1] * sin_e + ecliptic[2] * cos_e,
    ];
    let sun_longitude = sun_longitude_of_date(jd).to_radians();
    let elongation = (beta.cos() * (lambda - sun_longitude).cos())
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees();
    MoonSight {
        direction,
        horizontal_parallax_deg: parallax,
        elongation_deg: elongation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn angle_between(a: &Vec3, b: &Vec3) -> f64 {
        dot(a, b).clamp(-1.0, 1.0).acos().to_degrees()
    }

    fn jd_of(text: &str) -> f64 {
        julian_date(parse_utc(text).unwrap())
    }

    #[test]
    fn julian_date_of_j2000_and_unix_epoch() {
        assert!((julian_date(946_728_000.0) - J2000_JD).abs() < 1e-9);
        assert!((julian_date(0.0) - 2_440_587.5).abs() < 1e-9);
        assert!((unix_seconds_from_julian_date(julian_date(1.7e9)) - 1.7e9).abs() < 1e-3);
    }

    #[test]
    fn gmst_matches_meeus_example_12a() {
        // 1987 April 10, 0h UT: theta0 = 13h10m46.3668s = 197.693195 degrees.
        let jd = 2_446_895.5;
        assert!(
            (gmst_degrees(jd) - 197.693_195).abs() < 1e-4,
            "{}",
            gmst_degrees(jd)
        );
        assert!((gmst_degrees(J2000_JD) - 280.460_618_37).abs() < 1e-9);
    }

    #[test]
    fn sidereal_rotation_is_15_041_degrees_per_hour() {
        let jd = jd_of("2026-03-01 12:00:00");
        let mut difference = gmst_degrees(jd + 1.0 / 24.0) - gmst_degrees(jd);
        if difference < 0.0 {
            difference += 360.0;
        }
        assert!((difference - 15.041_068_6).abs() < 1e-6, "{difference}");
        // A sidereal day is 23h56m04.09s.
        let day = SIDEREAL_DEGREES_PER_DAY - 360.0;
        assert!((day - 0.985_647).abs() < 1e-5);
    }

    #[test]
    fn celestial_pole_altitude_equals_latitude() {
        for latitude in [-60.0, -10.0, 0.0, 35.0, 48.85, 89.0] {
            for lst in [0.0, 77.0, 200.0, 333.0] {
                let (altitude, azimuth) = equatorial_to_horizontal(12.0, 90.0, latitude, lst);
                assert!(
                    (altitude - latitude).abs() < 1e-9,
                    "lat {latitude} lst {lst}"
                );
                if latitude > 1.0 {
                    let off_north = azimuth.min(360.0 - azimuth);
                    assert!(off_north < 1e-6, "azimuth {azimuth}");
                }
            }
        }
    }

    #[test]
    fn polaris_altitude_is_close_to_latitude() {
        // Polaris J2000: RA 2h31m49s, Dec +89 15 51; 0.74 degrees from the pole.
        let (ra, dec) = (37.954_56, 89.264_1);
        for latitude in [20.0, 48.85, 65.0] {
            let mut lowest = f64::MAX;
            let mut highest = f64::MIN;
            for step in 0..96 {
                let lst = f64::from(step) * 3.75;
                let (altitude, _) = equatorial_to_horizontal(ra, dec, latitude, lst);
                lowest = lowest.min(altitude);
                highest = highest.max(altitude);
            }
            assert!(
                lowest > latitude - 0.8 && highest < latitude + 0.8,
                "{lowest} {highest}"
            );
            assert!((highest - lowest) > 1.0, "Polaris circles the pole");
        }
    }

    #[test]
    fn star_on_the_meridian_is_due_south_or_north_at_the_expected_altitude() {
        let latitude = 48.0;
        // Dec 20 south of the zenith: altitude 90 - (48 - 20) = 62, azimuth 180.
        let (altitude, azimuth) = equatorial_to_horizontal(100.0, 20.0, latitude, 100.0);
        assert!((altitude - 62.0).abs() < 1e-9);
        assert!((azimuth - 180.0).abs() < 1e-9);
        // Dec 70: north of the zenith, altitude 90 - (70 - 48) = 68, azimuth 0.
        let (altitude, azimuth) = equatorial_to_horizontal(100.0, 70.0, latitude, 100.0);
        assert!((altitude - 68.0).abs() < 1e-9);
        assert!(!(1e-6..=360.0 - 1e-6).contains(&azimuth));
    }

    #[test]
    fn hour_angle_sign_puts_rising_stars_in_the_east() {
        // Equatorial star, hour angle -90 degrees (LST = RA - 90): rising, due east.
        let (altitude, azimuth) = equatorial_to_horizontal(100.0, 0.0, 30.0, 10.0);
        assert!((azimuth - 90.0).abs() < 1e-6, "{azimuth}");
        assert!(altitude > 0.0 && altitude < 60.0 + 1e-9);
        let (_, azimuth) = equatorial_to_horizontal(100.0, 0.0, 30.0, 190.0);
        assert!((azimuth - 270.0).abs() < 1e-6, "{azimuth}");
    }

    #[test]
    fn horizon_matrix_agrees_with_direct_formula_and_is_orthonormal() {
        let matrix = horizon_matrix(-33.0, 123.0);
        for row in &matrix {
            assert!((dot(row, row) - 1.0).abs() < 1e-12);
        }
        assert!(dot(&matrix[0], &matrix[1]).abs() < 1e-12);
        // Right-handed: east x north = up.
        let up = cross(&matrix[0], &matrix[1]);
        for axis in 0..3 {
            assert!((up[axis] - matrix[2][axis]).abs() < 1e-12);
        }
        let (altitude, azimuth) = equatorial_to_horizontal(200.0, -40.0, -33.0, 123.0);
        let enu = mat_vec(&matrix, &equatorial_vector(200.0, -40.0));
        let (alt2, az2) = enu_to_altitude_azimuth(&enu);
        assert!((altitude - alt2).abs() < 1e-9 && (azimuth - az2).abs() < 1e-9);
        // Textbook: sin(alt) = sin(dec) sin(lat) + cos(dec) cos(lat) cos(H).
        let hour_angle = (123.0_f64 - 200.0).to_radians();
        let expected = (-40.0_f64.to_radians().sin() * 0.0
            + (-40.0_f64).to_radians().sin() * (-33.0_f64).to_radians().sin()
            + (-40.0_f64).to_radians().cos() * (-33.0_f64).to_radians().cos() * hour_angle.cos())
        .asin()
        .to_degrees();
        assert!((altitude - expected).abs() < 1e-9);
    }

    /// Times (hours after `start_jd`) at which a star crosses the horizon.
    fn crossings(
        ra: f64,
        dec: f64,
        latitude: f64,
        longitude: f64,
        start_jd: f64,
    ) -> (Vec<f64>, Vec<f64>) {
        let mut rises = Vec::new();
        let mut sets = Vec::new();
        let step = 1.0 / 1440.0;
        let altitude_at = |jd: f64| {
            equatorial_to_horizontal(ra, dec, latitude, local_sidereal_degrees(jd, longitude)).0
        };
        let mut previous = altitude_at(start_jd);
        for minute in 1..=1440 {
            let jd = start_jd + f64::from(minute) * step;
            let current = altitude_at(jd);
            if previous < 0.0 && current >= 0.0 {
                rises.push(f64::from(minute) / 60.0);
            }
            if previous >= 0.0 && current < 0.0 {
                sets.push(f64::from(minute) / 60.0);
            }
            previous = current;
        }
        (rises, sets)
    }

    #[test]
    fn sirius_rises_and_sets_at_plausible_times_for_paris() {
        // Sirius J2000: RA 6h45m08.9s, Dec -16 42 58. Paris 48.8566 N, 2.3522 E.
        // Around 30 December it culminates near midnight. Its geometric half
        // arc is acos(-tan(lat) tan(dec)) = 69.9 degrees = 4.65 sidereal hours,
        // so it rises near 19:20 UT and sets near 04:40 UT the next morning
        // (Paris is 9 minutes east of Greenwich).
        let (ra, dec) = (101.287_1, -16.716_1);
        let start = jd_of("2024-12-30 12:00:00");
        let (rises, sets) = crossings(ra, dec, 48.8566, 2.3522, start);
        assert_eq!((rises.len(), sets.len()), (1, 1), "{rises:?} {sets:?}");
        let half_arc_hours = (-(48.8566_f64).to_radians().tan() * dec.to_radians().tan())
            .acos()
            .to_degrees()
            / 15.041;
        assert!((half_arc_hours - 4.65).abs() < 0.03, "{half_arc_hours}");
        // Hours after 12:00 UT: rise 19:20 -> 7.3, set 04:40 -> 16.7.
        assert!((rises[0] - 7.3).abs() < 0.35, "rise {}", rises[0]);
        assert!((sets[0] - 16.7).abs() < 0.35, "set {}", sets[0]);
        assert!((sets[0] - rises[0] - 2.0 * half_arc_hours).abs() < 0.05);
    }

    #[test]
    fn sirius_transits_near_midnight_at_greenwich_on_30_december() {
        let (ra, _dec) = (101.287_1, -16.716_1);
        let mut best = (f64::MAX, 0.0);
        let start = jd_of("2024-12-30 12:00:00");
        for minute in 0..1440 {
            let jd = start + f64::from(minute) / 1440.0;
            let hour_angle =
                (local_sidereal_degrees(jd, 0.0) - ra + 540.0).rem_euclid(360.0) - 180.0;
            if hour_angle.abs() < best.0 {
                best = (hour_angle.abs(), f64::from(minute) / 60.0);
            }
        }
        // Minutes after 12:00 UT; midnight is 12.0 h. Equation of time is a few minutes.
        assert!(
            (best.1 - 12.0).abs() < 0.4,
            "transit at {} h after noon",
            best.1
        );
    }

    #[test]
    fn precession_moves_the_equinox_about_50_arcseconds_per_year() {
        let jd = J2000_JD + 36_525.0;
        let matrix = precession_matrix(jd);
        let before = equatorial_vector(0.0, 0.0);
        let after = mat_vec(&matrix, &before);
        let (ra, dec) = vector_to_equatorial(&after);
        // m = 46.1 arcsec/yr in RA at RA 0, Dec 0 -> 1.28 degrees per century,
        // n = 20.0 arcsec/yr in Dec -> 0.557 degrees per century.
        assert!((ra - 1.281).abs() < 0.01, "{ra}");
        assert!((dec - 0.557).abs() < 0.01, "{dec}");
        let identity = precession_matrix(J2000_JD);
        for (row, expected) in identity.iter().zip(IDENTITY.iter()) {
            for (a, b) in row.iter().zip(expected.iter()) {
                assert!((a - b).abs() < 1e-12);
            }
        }
        // Polaris gets closer to the pole until about 2100 (declination rises).
        let polaris = equatorial_vector(37.954_56, 89.264_1);
        let (_, dec_2100) = vector_to_equatorial(&mat_vec(&matrix, &polaris));
        assert!(dec_2100 > 89.264_1 + 0.1);
    }

    #[test]
    fn calendar_round_trips_and_parses() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for days in [-1000, 0, 11_016, 11_017, 20_000, 60_000] {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days);
        }
        assert_eq!(parse_utc("2000-01-01 12:00"), Some(946_728_000.0));
        assert_eq!(parse_utc("2000-01-01T12:00:00Z"), Some(946_728_000.0));
        assert_eq!(parse_utc("2000-01-01"), Some(946_684_800.0));
        assert_eq!(format_utc(946_728_000.0), "2000-01-01 12:00:00");
        for bad in [
            "",
            "yesterday",
            "2000-13-01",
            "2000-02-30",
            "2000-01-01 25:00",
            "2000-01-01 12:60",
            "2000-1",
        ] {
            assert_eq!(parse_utc(bad), None, "{bad}");
        }
        assert!(parse_utc("2024-02-29").is_some());
        assert!(parse_utc("2023-02-29").is_none());
    }

    #[test]
    fn sun_longitude_hits_equinox_and_solstice() {
        // 2024 March equinox 03:06 UTC and June solstice 20:51 UTC.
        let equinox = sun_longitude_of_date(jd_of("2024-03-20 03:06:00"));
        assert!(!(0.15..=359.85).contains(&equinox), "{equinox}");
        let solstice = sun_longitude_of_date(jd_of("2024-06-20 20:51:00"));
        assert!((solstice - 90.0).abs() < 0.15, "{solstice}");
    }

    fn ecliptic_longitude_of_date(direction_j2000: &Vec3, jd: f64) -> f64 {
        let dated = mat_vec(&precession_matrix(jd), direction_j2000);
        let (sin_e, cos_e) = obliquity_degrees(jd).to_radians().sin_cos();
        let y = dated[1] * cos_e + dated[2] * sin_e;
        y.atan2(dated[0]).to_degrees().rem_euclid(360.0)
    }

    #[test]
    fn outer_planets_are_opposite_the_sun_at_known_oppositions() {
        // Oppositions: Mars 2022-12-08, Jupiter 2023-11-03, Saturn 2023-08-27.
        for (planet, date) in [
            (Planet::Mars, "2022-12-08 06:00:00"),
            (Planet::Jupiter, "2023-11-03 12:00:00"),
            (Planet::Saturn, "2023-08-27 12:00:00"),
        ] {
            let jd = jd_of(date);
            let sight = planet_sight(planet, jd);
            let (_, elongation) = planet_distance_and_elongation(planet, jd);
            assert!(
                elongation > 176.0,
                "{} elongation {}",
                planet.name(),
                elongation
            );
            let planet_longitude = ecliptic_longitude_of_date(&sight.direction, jd);
            let sun_longitude = sun_longitude_of_date(jd);
            let difference =
                ((planet_longitude - sun_longitude - 180.0 + 540.0).rem_euclid(360.0)) - 180.0;
            assert!(difference.abs() < 1.5, "{} {difference}", planet.name());
        }
    }

    #[test]
    fn planet_brightness_and_distance_are_plausible() {
        let mars = planet_sight(Planet::Mars, jd_of("2022-12-08 06:00:00"));
        let (mars_distance, _) =
            planet_distance_and_elongation(Planet::Mars, jd_of("2022-12-08 06:00:00"));
        assert!((mars_distance - 0.55).abs() < 0.03, "{mars_distance}");
        assert!(
            mars.magnitude < -1.0 && mars.magnitude > -2.3,
            "{}",
            mars.magnitude
        );
        let jupiter = planet_sight(Planet::Jupiter, jd_of("2023-11-03 12:00:00"));
        assert!(
            jupiter.magnitude < -2.5 && jupiter.magnitude > -3.2,
            "{}",
            jupiter.magnitude
        );
        // Venus is never far from the Sun (elongation below 48 degrees).
        for offset in 0..40 {
            let jd = jd_of("2024-01-01 00:00:00") + f64::from(offset) * 15.0;
            let venus = planet_sight(Planet::Venus, jd);
            let (_, venus_elongation) = planet_distance_and_elongation(Planet::Venus, jd);
            assert!(venus_elongation < 48.5, "{venus_elongation}");
            assert!(venus.magnitude < -3.0 && venus.magnitude > -4.95);
            let (_, mercury_elongation) = planet_distance_and_elongation(Planet::Mercury, jd);
            assert!(mercury_elongation < 28.5, "{mercury_elongation}");
        }
    }

    #[test]
    fn moon_phases_match_known_lunations() {
        // 2024: new moon 01-11 11:57, first quarter 01-18 03:53, full 01-25 17:54,
        // last quarter 02-02 23:18 (UTC).
        for (date, expected) in [
            ("2024-01-11 11:57:00", 0.0),
            ("2024-01-18 03:53:00", 90.0),
            ("2024-01-25 17:54:00", 180.0),
            ("2024-02-02 23:18:00", 90.0),
        ] {
            let jd = jd_of(date);
            let sight = moon_sight(jd);
            // Phases are defined by the difference in ecliptic longitude.
            let (sin_e, cos_e) = obliquity_degrees(jd).to_radians().sin_cos();
            let v = sight.direction;
            let moon_longitude = (v[1] * cos_e + v[2] * sin_e).atan2(v[0]).to_degrees();
            let difference = (moon_longitude - sun_longitude_of_date(jd)).rem_euclid(360.0);
            let difference = if difference > 180.0 && expected < 5.0 {
                difference - 360.0
            } else {
                difference
            };
            let difference = if expected == 90.0 && date.contains("02-02") {
                360.0 - difference
            } else {
                difference
            };
            assert!((difference - expected).abs() < 2.5, "{date}: {difference}");
            assert!(sight.elongation_deg >= difference.abs().min(180.0) - 6.0);
        }
    }

    #[test]
    fn moon_covers_the_sun_at_the_2024_total_eclipse() {
        let jd = jd_of("2024-04-08 18:17:00");
        let moon = moon_sight(jd);
        // Compare in the equator of date (the Sun vector is J2000 -> precess it).
        let sun = mat_vec(&precession_matrix(jd), &sun_direction(jd));
        let separation = angle_between(&moon.direction, &sun);
        assert!(separation < 1.0, "separation {separation}");
        assert!((0.9..1.03).contains(&moon.horizontal_parallax_deg));
    }

    #[test]
    fn galactic_axes_are_orthogonal_and_the_centre_is_in_sagittarius() {
        let (pole, centre) = galactic_axes();
        assert!(dot(&pole, &centre).abs() < 0.002);
        let (ra, dec) = vector_to_equatorial(&centre);
        assert!((ra - 266.4).abs() < 0.1 && (dec + 28.94).abs() < 0.1);
    }
}
