use chrono::offset::Offset;
use chrono::{DateTime, Datelike, LocalResult, NaiveDate, TimeZone};
use chrono_tz::Tz;
use cities_json::{CITIES, City as CityRecord};
use iso_country::data;
use once_cell::sync::Lazy;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;
use std::fmt;
use std::str::FromStr;
use strsim::jaro_winkler;
use tzf_rs::Finder;

static TZ_FINDER: Lazy<Finder> = Lazy::new(Finder::new);
static COUNTRY_LOOKUP: Lazy<HashMap<String, &'static str>> = Lazy::new(build_country_lookup);
static CITY_ALIASES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut aliases = HashMap::new();
    aliases.insert("nyc", "New York City, US");
    aliases
});

#[derive(Copy, Clone, Debug)]
pub struct Location {
    pub timezone: Tz,
    pub longitude: f64,
}

impl Location {
    pub fn new(timezone: Tz, longitude: f64) -> Self {
        Self {
            timezone,
            longitude,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CityLocation {
    pub location: Location,
    pub city_name: String,
    pub country_code: String,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone_name: String,
}

#[derive(Debug, Clone)]
pub enum LocationLookupError {
    EmptyQuery,
    UnknownCountry {
        input: String,
    },
    NotFound {
        query: String,
    },
    NotFoundInCountry {
        query: String,
        country_code: String,
        available_countries: Vec<String>,
    },
    Ambiguous {
        query: String,
        suggestions: Vec<String>,
    },
    TimezoneUnavailable {
        city: String,
    },
    InvalidTimezone {
        tz_name: String,
    },
}

impl fmt::Display for LocationLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocationLookupError::EmptyQuery => write!(f, "city name cannot be empty"),
            LocationLookupError::UnknownCountry { input } => {
                write!(f, "unable to resolve country identifier '{input}'")
            }
            LocationLookupError::NotFound { query } => {
                write!(f, "could not locate a city matching '{query}'")
            }
            LocationLookupError::NotFoundInCountry {
                query,
                country_code,
                available_countries,
            } => {
                if available_countries.is_empty() {
                    write!(
                        f,
                        "could not find '{query}' in country code '{country_code}'"
                    )
                } else {
                    write!(
                        f,
                        "could not find '{query}' in country code '{country_code}'. Matches exist in: {}",
                        available_countries.join(", ")
                    )
                }
            }
            LocationLookupError::Ambiguous { query, suggestions } => {
                if suggestions.is_empty() {
                    write!(f, "the city query '{query}' is ambiguous")
                } else {
                    write!(
                        f,
                        "the city query '{query}' is ambiguous. Try one of: {}",
                        suggestions.join(", ")
                    )
                }
            }
            LocationLookupError::TimezoneUnavailable { city } => write!(
                f,
                "unable to determine timezone for coordinates of '{city}'"
            ),
            LocationLookupError::InvalidTimezone { tz_name } => {
                write!(f, "unrecognized timezone identifier '{tz_name}'")
            }
        }
    }
}

impl std::error::Error for LocationLookupError {}

/// Resolve a user-provided city description (e.g. `"Paris"`, `"NYC"`, `"Lima, Peru"`)
/// into timezone and longitude coordinates.
pub fn resolve_city(query: &str) -> Result<CityLocation, LocationLookupError> {
    let parsed = ParsedQuery::parse(query)?;
    let country_code = resolve_country_code(parsed.country_input.as_deref())?;
    let country_code_ref = country_code.as_deref();

    let city = match_city(&parsed, country_code_ref)?;

    let tz_name = TZ_FINDER.get_tz_name(city.lng, city.lat);
    if tz_name.is_empty() {
        return Err(LocationLookupError::TimezoneUnavailable {
            city: describe_city(city),
        });
    }

    let timezone = Tz::from_str(tz_name).map_err(|_| LocationLookupError::InvalidTimezone {
        tz_name: tz_name.to_string(),
    })?;

    Ok(CityLocation {
        location: Location::new(timezone, city.lng),
        city_name: city.name.clone(),
        country_code: city.country.clone(),
        latitude: city.lat,
        longitude: city.lng,
        timezone_name: tz_name.to_string(),
    })
}

fn build_country_lookup() -> HashMap<String, &'static str> {
    let mut lookup = HashMap::new();
    for entry in data::all() {
        let alpha2_lower = entry.alpha2.to_ascii_lowercase();
        let alpha3_lower = entry.alpha3.to_ascii_lowercase();
        lookup.insert(alpha2_lower, entry.alpha2);
        lookup.insert(alpha3_lower, entry.alpha2);
        insert_country_name_variants(&mut lookup, entry.name, entry.alpha2);
    }
    lookup
}

fn insert_country_name_variants(
    lookup: &mut HashMap<String, &'static str>,
    name: &'static str,
    code: &'static str,
) {
    let lower = name.to_ascii_lowercase();
    lookup.entry(lower).or_insert(code);

    if let Some((without_paren, _)) = name.split_once('(') {
        let trimmed = without_paren.trim();
        if !trimmed.is_empty() {
            lookup.entry(trimmed.to_ascii_lowercase()).or_insert(code);
        }
    }

    if let Some((before_comma, _)) = name.split_once(',') {
        let trimmed = before_comma.trim();
        if !trimmed.is_empty() {
            lookup.entry(trimmed.to_ascii_lowercase()).or_insert(code);
        }
    }
}

fn resolve_country_code(input: Option<&str>) -> Result<Option<String>, LocationLookupError> {
    match input {
        None => Ok(None),
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            let key = trimmed.to_ascii_lowercase();
            match COUNTRY_LOOKUP.get(&key) {
                Some(code) => Ok(Some((*code).to_string())),
                None => Err(LocationLookupError::UnknownCountry {
                    input: value.to_string(),
                }),
            }
        }
    }
}

fn match_city(
    parsed: &ParsedQuery,
    country_code: Option<&str>,
) -> Result<&'static CityRecord, LocationLookupError> {
    let mut fallback_error: Option<LocationLookupError> = None;

    let exact_matches: Vec<&CityRecord> = CITIES
        .iter()
        .filter(|city| city.name.eq_ignore_ascii_case(&parsed.name))
        .collect();

    if let Some(result) = attempt_match(
        &exact_matches,
        parsed,
        country_code,
        false,
        &mut fallback_error,
    )? {
        return Ok(result);
    }

    if !parsed.abbreviation_tokens.is_empty() {
        let abbreviation_matches: Vec<&CityRecord> = CITIES
            .iter()
            .filter(|city| abbreviation_match(parsed, city))
            .collect();

        if let Some(result) = attempt_match(
            &abbreviation_matches,
            parsed,
            country_code,
            true,
            &mut fallback_error,
        )? {
            return Ok(result);
        }
    }

    let mut scored: Vec<(&CityRecord, f64)> = CITIES
        .iter()
        .map(|city| {
            let normalized_city = city.name.to_ascii_lowercase();
            let score = jaro_winkler(&parsed.normalized_name, &normalized_city);
            (city, score)
        })
        .filter(|(_, score)| *score >= 0.8)
        .collect();

    if scored.is_empty() {
        return Err(LocationLookupError::NotFound {
            query: parsed.original.clone(),
        });
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    let best_score = scored[0].1;
    let mut fuzzy_candidates: Vec<&CityRecord> = Vec::new();

    for (city, score) in scored {
        if !fuzzy_candidates.is_empty() && (best_score - score) > 0.05 {
            break;
        }
        fuzzy_candidates.push(city);
        if fuzzy_candidates.len() >= 10 {
            break;
        }
    }

    if let Some(result) = attempt_match(
        &fuzzy_candidates,
        parsed,
        country_code,
        false,
        &mut fallback_error,
    )? {
        return Ok(result);
    }

    Err(fallback_error.unwrap_or(LocationLookupError::NotFound {
        query: parsed.original.clone(),
    }))
}

fn attempt_match<'a>(
    candidates: &[&'a CityRecord],
    parsed: &ParsedQuery,
    country_code: Option<&str>,
    allow_ambiguity: bool,
    fallback_error: &mut Option<LocationLookupError>,
) -> Result<Option<&'a CityRecord>, LocationLookupError> {
    if candidates.is_empty() {
        return Ok(None);
    }

    if let Some(code) = country_code {
        let filtered: Vec<&CityRecord> = candidates
            .iter()
            .copied()
            .filter(|city| city.country.eq_ignore_ascii_case(code))
            .collect();

        if filtered.is_empty() {
            let available = unique_countries(candidates);
            return Err(LocationLookupError::NotFoundInCountry {
                query: parsed.original.clone(),
                country_code: code.to_ascii_uppercase(),
                available_countries: available,
            });
        }

        return select_candidate(&filtered, parsed).map(Some);
    }

    match select_candidate(candidates, parsed) {
        Ok(city) => Ok(Some(city)),
        Err(err @ LocationLookupError::Ambiguous { .. }) if allow_ambiguity => {
            if fallback_error.is_none() {
                *fallback_error = Some(err.clone());
            }
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

fn select_candidate<'a>(
    candidates: &[&'a CityRecord],
    parsed: &ParsedQuery,
) -> Result<&'a CityRecord, LocationLookupError> {
    match candidates {
        [] => Err(LocationLookupError::NotFound {
            query: parsed.original.clone(),
        }),
        [single] => Ok(*single),
        many => Err(LocationLookupError::Ambiguous {
            query: parsed.original.clone(),
            suggestions: many.iter().map(|city| describe_city(city)).collect(),
        }),
    }
}

fn unique_countries(candidates: &[&CityRecord]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for city in candidates {
        if seen.insert(&city.country) {
            result.push(city.country.clone());
        }
    }
    result
}

fn abbreviation_tokens(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();

    let parts: Vec<&str> = value
        .split(|c: char| c.is_ascii_whitespace() || c == '-' || c == '_' || c == '/')
        .filter(|part| !part.is_empty())
        .collect();

    if !parts.is_empty() {
        let abbreviation: String = parts
            .iter()
            .filter_map(|part| part.chars().next())
            .map(|ch| ch.to_ascii_lowercase())
            .collect();
        if !abbreviation.is_empty() {
            tokens.push(abbreviation);
        }
    }

    let condensed: String = value
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if !condensed.is_empty() {
        tokens.push(condensed);
    }

    tokens.sort();
    tokens.dedup();
    tokens
}

fn abbreviation_match(parsed: &ParsedQuery, city: &CityRecord) -> bool {
    if parsed.abbreviation_tokens.is_empty() {
        return false;
    }

    let city_tokens = abbreviation_tokens(&city.name);
    if city_tokens.is_empty() {
        return false;
    }

    parsed
        .abbreviation_tokens
        .iter()
        .filter(|candidate| candidate.len() > 1 || parsed.condensed_length <= 1)
        .any(|candidate| city_tokens.iter().any(|token| token == candidate))
}

fn describe_city(city: &CityRecord) -> String {
    format!("{} ({})", city.name, city.country)
}

struct ParsedQuery {
    original: String,
    name: String,
    normalized_name: String,
    abbreviation_tokens: Vec<String>,
    condensed_length: usize,
    country_input: Option<String>,
}

impl ParsedQuery {
    fn parse(input: &str) -> Result<Self, LocationLookupError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(LocationLookupError::EmptyQuery);
        }

        let lowered = trimmed.to_ascii_lowercase();

        let canonical_source = CITY_ALIASES
            .get(lowered.as_str())
            .copied()
            .unwrap_or(trimmed);

        let segments: Vec<String> = canonical_source
            .split(',')
            .map(|segment| segment.trim())
            .filter(|segment| !segment.is_empty())
            .map(String::from)
            .collect();

        let name = segments
            .first()
            .cloned()
            .unwrap_or_else(|| trimmed.to_string());
        let country_input = if segments.len() > 1 {
            Some(segments[1..].join(", "))
        } else {
            None
        };

        let normalized_name = name.to_ascii_lowercase();
        let condensed: String = name
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        let condensed_length = condensed.chars().count();

        let mut abbreviation_tokens = abbreviation_tokens(&name);

        if condensed_length > 1
            && condensed_length <= 6
            && !abbreviation_tokens.iter().any(|token| token == &condensed)
        {
            abbreviation_tokens.push(condensed.clone());
            abbreviation_tokens.sort();
            abbreviation_tokens.dedup();
        }

        Ok(Self {
            original: trimmed.to_string(),
            name,
            normalized_name,
            abbreviation_tokens,
            condensed_length,
            country_input,
        })
    }
}

const MINUTES_PER_DAY: f64 = 1_440.0;
const SECONDS_PER_DAY: f64 = 86_400.0;
const MINUTES_PER_HOUR: f64 = 60.0;
const MINUTES_PER_DEGREE: f64 = 4.0;
const SOLAR_NOON_BASE_MINUTES: f64 = 720.0;
const SECONDS_PER_HOUR: i64 = 3_600;
const SECONDS_PER_MINUTE: i64 = 60;
const JULIAN_DAY_J2000: f64 = 2_451_545.0;
const JULIAN_DAYS_PER_CENTURY: f64 = 36_525.0;

/// Returns the solar noon instant for the provided date at the requested location.
pub fn solar_noon_datetime(location: Location, date: NaiveDate) -> DateTime<Tz> {
    let timezone_hours = timezone_offset_hours(location.timezone, date);
    let jd = julian_day(date);
    let sol_noon_minutes = calc_sol_noon(jd, location.longitude, timezone_hours);
    let (hour, minute, second) = minutes_to_hms(sol_noon_minutes);

    zoned_time(
        location.timezone,
        date,
        hour,
        minute,
        second,
        "unable to resolve solar noon",
    )
}

fn timezone_offset_hours(tz: Tz, date: NaiveDate) -> f64 {
    let local_midday = zoned_time(
        tz,
        date,
        12,
        0,
        0,
        "local noon is not representable when computing offset",
    );

    f64::from(local_midday.offset().fix().local_minus_utc()) / SECONDS_PER_HOUR as f64
}

fn zoned_time(
    tz: Tz,
    date: NaiveDate,
    hour: u32,
    minute: u32,
    second: u32,
    context: &str,
) -> DateTime<Tz> {
    match tz.with_ymd_and_hms(date.year(), date.month(), date.day(), hour, minute, second) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt,
        LocalResult::None => panic!("{context} on {}", date),
    }
}

fn minutes_to_hms(minutes: f64) -> (u32, u32, u32) {
    let total_seconds = (minutes * SECONDS_PER_MINUTE as f64)
        .round()
        .rem_euclid(SECONDS_PER_DAY) as i64;
    let hour = (total_seconds / SECONDS_PER_HOUR) as u32;
    let minute = ((total_seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE) as u32;
    let second = (total_seconds % SECONDS_PER_MINUTE) as u32;

    (hour, minute, second)
}

fn julian_day(date: NaiveDate) -> f64 {
    let mut y = date.year() as f64;
    let mut m = date.month() as f64;
    let d = date.day() as f64;

    if m <= 2.0 {
        y -= 1.0;
        m += 12.0;
    }

    let a = (y / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();

    (365.25 * (y + 4716.0)).floor() + (30.6001 * (m + 1.0)).floor() + d + b - 1524.5
}

/// Returns the solar-noon minute count (local time) for the provided day and longitude.
fn calc_sol_noon(jd: f64, longitude: f64, timezone: f64) -> f64 {
    let t_noon = calc_time_julian_cent(jd - longitude / 360.0);
    let mut eq_time = calc_equation_of_time(t_noon);
    let sol_noon_offset = SOLAR_NOON_BASE_MINUTES - (longitude * MINUTES_PER_DEGREE) - eq_time;

    let new_t = calc_time_julian_cent(jd - 0.5 + sol_noon_offset / MINUTES_PER_DAY);
    eq_time = calc_equation_of_time(new_t);

    (SOLAR_NOON_BASE_MINUTES - (longitude * MINUTES_PER_DEGREE) - eq_time
        + (timezone * MINUTES_PER_HOUR))
        .rem_euclid(MINUTES_PER_DAY)
}

fn calc_time_julian_cent(jd: f64) -> f64 {
    (jd - JULIAN_DAY_J2000) / JULIAN_DAYS_PER_CENTURY
}

fn calc_equation_of_time(t: f64) -> f64 {
    let epsilon = calc_obliquity_correction(t);
    let l0 = calc_geom_mean_long_sun(t);
    let e = calc_eccentricity_earth_orbit(t);
    let m = calc_geom_mean_anomaly_sun(t);

    let epsilon_rad = deg_to_rad(epsilon);
    let l0_rad = deg_to_rad(l0);
    let m_rad = deg_to_rad(m);

    let y = (epsilon_rad / 2.0).tan().powi(2);
    let sin2l0 = (2.0 * l0_rad).sin();
    let sinm = m_rad.sin();
    let cos2l0 = (2.0 * l0_rad).cos();
    let sin4l0 = (4.0 * l0_rad).sin();
    let sin2m = (2.0 * m_rad).sin();

    let e_time = y * sin2l0 - 2.0 * e * sinm + 4.0 * e * y * sinm * cos2l0
        - 0.5 * y * y * sin4l0
        - 1.25 * e * e * sin2m;

    rad_to_deg(e_time) * MINUTES_PER_DEGREE
}

fn calc_geom_mean_long_sun(t: f64) -> f64 {
    let l0 = 280.46646 + t * (36_000.76983 + t * 0.0003032);
    l0.rem_euclid(360.0)
}

fn calc_geom_mean_anomaly_sun(t: f64) -> f64 {
    357.52911 + t * (35_999.05029 - 0.0001537 * t)
}

fn calc_eccentricity_earth_orbit(t: f64) -> f64 {
    0.016_708_634 - t * (0.000_042_037 + 0.000_000_1267 * t)
}

fn calc_mean_obliquity_of_ecliptic(t: f64) -> f64 {
    let seconds = 21.448 - t * (46.8150 + t * (0.00059 - t * 0.001813));
    23.0 + (26.0 + seconds / 60.0) / 60.0
}

fn calc_obliquity_correction(t: f64) -> f64 {
    let e0 = calc_mean_obliquity_of_ecliptic(t);
    let omega = 125.04 - 1934.136 * t;
    e0 + 0.00256 * deg_to_rad(omega).cos()
}

fn deg_to_rad(degrees: f64) -> f64 {
    degrees * PI / 180.0
}

fn rad_to_deg(radians: f64) -> f64 {
    radians * 180.0 / PI
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use chrono_tz::America::New_York;

    fn nyc_location() -> Location {
        resolve_city("New York City")
            .expect("NYC should resolve")
            .location
    }

    fn assert_solar_noon(
        location: Location,
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) {
        let date = NaiveDate::from_ymd_opt(year, month, day).expect("valid date");
        let actual = solar_noon_datetime(location, date);
        let expected = New_York
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .expect("expected instant to exist");

        assert_eq!(
            actual,
            expected,
            "solar noon mismatch on {} (expected {:02}:{:02}:{:02}, got {})",
            date,
            hour,
            minute,
            second,
            actual.format("%H:%M:%S %Z")
        );
    }

    #[test]
    fn resolves_new_york_city() {
        let city = resolve_city("New York City").expect("NYC exact match");
        assert_eq!(city.city_name, "New York City");
        assert_eq!(city.country_code, "US");
        assert_eq!(city.location.timezone, New_York);
    }

    #[test]
    fn resolves_nyc_initialism() {
        let city = resolve_city("NYC").expect("NYC abbreviation");
        assert_eq!(city.city_name, "New York City");
        assert_eq!(city.country_code, "US");
    }

    #[test]
    fn matches_noaa_october_2025() {
        assert_solar_noon(nyc_location(), 2025, 10, 17, 12, 41, 22);
    }

    #[test]
    fn matches_noaa_december_2024() {
        assert_solar_noon(nyc_location(), 2024, 12, 21, 11, 54, 12);
    }

    #[test]
    fn matches_noaa_june_2024() {
        assert_solar_noon(nyc_location(), 2024, 6, 21, 12, 57, 53);
    }
}
