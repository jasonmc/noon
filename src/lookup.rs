use crate::Location;
use chrono_tz::Tz;
use cities_json::{CITIES, City as CityRecord};
use iso_country::data;
use once_cell::sync::Lazy;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
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

        let (name, country_input) = split_name_and_country(canonical_source);
        let name = if name.is_empty() {
            trimmed.to_string()
        } else {
            name
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

fn split_name_and_country(input: &str) -> (String, Option<String>) {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }

    if trimmed.contains(',') {
        let mut segments = trimmed
            .split(',')
            .map(|segment| segment.trim())
            .filter(|segment| !segment.is_empty());

        if let Some(name) = segments.next() {
            let rest: Vec<&str> = segments.collect();
            let country = if rest.is_empty() {
                None
            } else {
                Some(rest.join(", ").to_string())
            };
            return (name.to_string(), country);
        }
    }

    if let Some((open_idx, close_idx)) = trailing_paren_pair(trimmed) {
        let name_part = trimmed[..open_idx].trim();
        let country_part = trimmed[open_idx + 1..close_idx].trim();

        if !name_part.is_empty() && !country_part.is_empty() {
            let key = country_part.to_ascii_lowercase();
            if COUNTRY_LOOKUP.get(&key).is_some() {
                return (name_part.to_string(), Some(country_part.to_string()));
            }
        }
    }

    (trimmed.to_string(), None)
}

fn trailing_paren_pair(value: &str) -> Option<(usize, usize)> {
    let close_idx = value.rfind(')')?;
    if close_idx != value.len().saturating_sub(1) {
        return None;
    }
    let open_idx = value[..close_idx].rfind('(')?;
    Some((open_idx, close_idx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::America::New_York;

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
    fn resolves_with_country_hint_in_parentheses() {
        let city = resolve_city("Dublin (IE)").expect("Dublin with country hint");
        assert_eq!(city.city_name, "Dublin");
        assert_eq!(city.country_code, "IE");
    }
}
