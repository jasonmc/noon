use crate::Location;
use chrono_tz::Tz;
use iso_country::data;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;
use strsim::jaro_winkler;

const CITIES_TSV: &str = include_str!(concat!(env!("OUT_DIR"), "/cities.tsv"));

static COUNTRY_LOOKUP: LazyLock<HashMap<String, &'static str>> =
    LazyLock::new(build_country_lookup);
static CITY_ALIASES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut aliases = HashMap::new();
    aliases.insert("nyc", "New York City, US");
    aliases
});

#[derive(Debug, Clone, Copy)]
struct CityRecord {
    name: &'static str,
    country: &'static str,
    admin1: &'static str,
    admin2: &'static str,
    lat: f64,
    lng: f64,
    tz: &'static str,
}

impl CityRecord {
    fn from_tsv_line(line: &'static str) -> Option<Self> {
        let mut fields = line.split('\t');
        Some(Self {
            name: fields.next()?,
            country: fields.next()?,
            admin1: fields.next()?,
            admin2: fields.next()?,
            lat: fields.next()?.parse().ok()?,
            lng: fields.next()?.parse().ok()?,
            tz: fields.next()?,
        })
    }
}

fn city_lines() -> impl Iterator<Item = (&'static str, &'static str)> {
    CITIES_TSV
        .lines()
        .filter_map(|line| Some((line.split_once('\t')?.0, line)))
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
            LocationLookupError::TimezoneUnavailable { city } => {
                write!(
                    f,
                    "unable to determine timezone for coordinates of '{city}'"
                )
            }
            LocationLookupError::InvalidTimezone { tz_name } => {
                write!(f, "unrecognized timezone identifier '{tz_name}'")
            }
        }
    }
}

impl std::error::Error for LocationLookupError {}

pub fn resolve_city(query: &str) -> Result<CityLocation, LocationLookupError> {
    let parsed = ParsedQuery::parse(query)?;
    let city = match_city(&parsed)?;

    if city.tz.is_empty() {
        return Err(LocationLookupError::TimezoneUnavailable {
            city: describe_city(&city),
        });
    }

    let timezone = Tz::from_str(city.tz).map_err(|_| LocationLookupError::InvalidTimezone {
        tz_name: city.tz.to_string(),
    })?;

    Ok(CityLocation {
        location: Location::new(timezone, city.lat, city.lng),
        city_name: city.name.to_string(),
        country_code: city.country.to_string(),
        latitude: city.lat,
        longitude: city.lng,
        timezone_name: city.tz.to_string(),
    })
}

fn build_country_lookup() -> HashMap<String, &'static str> {
    let mut lookup = HashMap::new();
    for entry in data::all() {
        lookup.insert(entry.alpha2.to_ascii_lowercase(), entry.alpha2);
        lookup.insert(entry.alpha3.to_ascii_lowercase(), entry.alpha2);
        insert_country_name_variants(&mut lookup, entry.name, entry.alpha2);
    }
    lookup
}

fn insert_country_name_variants(
    lookup: &mut HashMap<String, &'static str>,
    name: &'static str,
    code: &'static str,
) {
    let mut add = |value: &str| {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            lookup.entry(trimmed.to_ascii_lowercase()).or_insert(code);
        }
    };

    add(name);

    if let Some((before_paren, _)) = name.split_once('(') {
        add(before_paren);
    }
    if let Some((before_comma, _)) = name.split_once(',') {
        add(before_comma);
    }
    if name.contains(" of ") {
        add(&name.replace(" of ", " "));
        add(&name.replace(" of ", ""));
    }
}

fn match_city(parsed: &ParsedQuery) -> Result<CityRecord, LocationLookupError> {
    let mut fallback = None;

    let name_bytes = parsed.name.as_bytes();
    let name_len = name_bytes.len();
    let exact: Vec<CityRecord> = if name_bytes.contains(&b'\t') {
        Vec::new()
    } else {
        CITIES_TSV
            .lines()
            .filter(|line| {
                let bytes = line.as_bytes();
                bytes.get(name_len) == Some(&b'\t')
                    && bytes[..name_len].eq_ignore_ascii_case(name_bytes)
            })
            .filter_map(CityRecord::from_tsv_line)
            .collect()
    };

    if let Some(result) = attempt_match(&exact, parsed, false, &mut fallback)? {
        return Ok(result);
    }

    if !parsed.abbreviation_tokens.is_empty() {
        let abbreviation_matches: Vec<CityRecord> = city_lines()
            .filter(|(name, _)| abbreviation_match(parsed, name))
            .filter_map(|(_, line)| CityRecord::from_tsv_line(line))
            .collect();

        if let Some(result) = attempt_match(&abbreviation_matches, parsed, true, &mut fallback)? {
            return Ok(result);
        }
    }

    let fuzzy = best_fuzzy_matches(parsed);
    if let Some(result) = attempt_match(&fuzzy, parsed, false, &mut fallback)? {
        return Ok(result);
    }

    Err(fallback.unwrap_or_else(|| LocationLookupError::NotFound {
        query: parsed.original.clone(),
    }))
}

fn best_fuzzy_matches(parsed: &ParsedQuery) -> Vec<CityRecord> {
    let mut scored: Vec<(CityRecord, f64)> = city_lines()
        .filter_map(|(name, line)| {
            let score = jaro_winkler(&parsed.normalized_name, &name.to_ascii_lowercase());
            (score >= 0.8).then(|| Some((CityRecord::from_tsv_line(line)?, score)))?
        })
        .collect();

    if scored.is_empty() {
        return Vec::new();
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    let best = scored.first().map(|(_, score)| *score).unwrap_or(0.0);

    scored
        .into_iter()
        .take_while(|(_, score)| best - *score <= 0.05)
        .take(10)
        .map(|(city, _)| city)
        .collect()
}

fn attempt_match(
    candidates: &[CityRecord],
    parsed: &ParsedQuery,
    allow_ambiguity: bool,
    fallback: &mut Option<LocationLookupError>,
) -> Result<Option<CityRecord>, LocationLookupError> {
    if candidates.is_empty() {
        return Ok(None);
    }

    let candidates = if let Some(code) = parsed.country_code.as_deref() {
        let filtered: Vec<CityRecord> = candidates
            .iter()
            .copied()
            .filter(|city| city.country.eq_ignore_ascii_case(code))
            .collect();

        if filtered.is_empty() {
            return Err(LocationLookupError::NotFoundInCountry {
                query: parsed.original.clone(),
                country_code: code.to_ascii_uppercase(),
                available_countries: unique_countries(candidates),
            });
        }

        let regional = refine_by_region(&filtered, &parsed.region_hints);
        if regional.is_empty() {
            filtered
        } else {
            regional
        }
    } else {
        candidates.to_vec()
    };

    match select_candidate(&candidates, parsed) {
        Ok(city) => Ok(Some(city)),
        Err(err @ LocationLookupError::Ambiguous { .. }) if allow_ambiguity => {
            if fallback.is_none() {
                *fallback = Some(err.clone());
            }
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

fn select_candidate(
    candidates: &[CityRecord],
    parsed: &ParsedQuery,
) -> Result<CityRecord, LocationLookupError> {
    match candidates {
        [] => Err(LocationLookupError::NotFound {
            query: parsed.original.clone(),
        }),
        [single] => Ok(*single),
        many => Err(LocationLookupError::Ambiguous {
            query: parsed.original.clone(),
            suggestions: many.iter().map(describe_city).collect(),
        }),
    }
}

fn unique_countries(candidates: &[CityRecord]) -> Vec<String> {
    let mut seen = HashSet::new();
    candidates
        .iter()
        .filter_map(|city| {
            if seen.insert(city.country) {
                Some(city.country.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn refine_by_region(candidates: &[CityRecord], region_hints: &[String]) -> Vec<CityRecord> {
    let hints: Vec<String> = region_hints
        .iter()
        .map(|hint| hint.trim().to_ascii_lowercase())
        .filter(|hint| !hint.is_empty())
        .collect();

    if hints.is_empty() {
        return Vec::new();
    }

    candidates
        .iter()
        .copied()
        .filter(|city| admin_matches_any_hint(city, &hints))
        .collect()
}

fn admin_matches_any_hint(city: &CityRecord, hints: &[String]) -> bool {
    let admin1 = city.admin1.to_ascii_lowercase();
    let admin2 = city.admin2.to_ascii_lowercase();

    hints.iter().any(|hint| {
        admin1 == *hint
            || (!admin1.is_empty() && admin1.starts_with(hint))
            || (!admin2.is_empty() && (admin2 == *hint || admin2.starts_with(hint)))
    })
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

fn abbreviation_match(parsed: &ParsedQuery, city_name: &str) -> bool {
    if parsed.abbreviation_tokens.is_empty() {
        return false;
    }

    let cond_len = parsed.condensed_name.len();
    let city_tokens = abbreviation_tokens(city_name);
    if city_tokens.is_empty() {
        return false;
    }

    parsed
        .abbreviation_tokens
        .iter()
        .filter(|candidate| candidate.len() > 1 || cond_len <= 1)
        .any(|candidate| city_tokens.iter().any(|token| token == candidate))
}

fn describe_city(city: &CityRecord) -> String {
    let country = city.country;
    let admin1 = city.admin1.trim();
    let admin2 = city.admin2.trim();

    match (admin1.is_empty(), admin2.is_empty()) {
        (true, _) => format!("{} ({})", city.name, country),
        (false, true) => format!("{} ({}-{})", city.name, country, admin1),
        (false, false) => format!("{} ({}-{}-{})", city.name, country, admin1, admin2),
    }
}

struct ParsedQuery {
    original: String,
    name: String,
    normalized_name: String,
    condensed_name: String,
    abbreviation_tokens: Vec<String>,
    country_code: Option<String>,
    region_hints: Vec<String>,
}

impl ParsedQuery {
    fn parse(input: &str) -> Result<Self, LocationLookupError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(LocationLookupError::EmptyQuery);
        }

        let canonical_source = CITY_ALIASES
            .get(trimmed.to_ascii_lowercase().as_str())
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

        let mut abbreviation_tokens = abbreviation_tokens(&name);
        if condensed.len() > 1
            && condensed.len() <= 6
            && !abbreviation_tokens.iter().any(|token| token == &condensed)
        {
            abbreviation_tokens.push(condensed.clone());
            abbreviation_tokens.sort();
            abbreviation_tokens.dedup();
        }

        let (country_code, region_hints) = parse_country_segments(country_input.as_deref())?;

        Ok(Self {
            original: trimmed.to_string(),
            name,
            normalized_name,
            condensed_name: condensed,
            abbreviation_tokens,
            country_code,
            region_hints,
        })
    }
}

fn split_name_and_country(input: &str) -> (String, Option<String>) {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }

    if let Some((open_idx, close_idx)) = trailing_paren_pair(trimmed) {
        let name = trimmed[..open_idx].trim();
        let country = trimmed[open_idx + 1..close_idx].trim();

        if !name.is_empty() && !country.is_empty() {
            return (name.to_string(), Some(country.to_string()));
        }
    }

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

    (trimmed.to_string(), None)
}

fn parse_country_segments(
    input: Option<&str>,
) -> Result<(Option<String>, Vec<String>), LocationLookupError> {
    let Some(raw) = input else {
        return Ok((None, Vec::new()));
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok((None, Vec::new()));
    }

    let lowered_full = trimmed.to_ascii_lowercase();
    if let Some(code) = COUNTRY_LOOKUP.get(&lowered_full) {
        return Ok((Some((*code).to_string()), Vec::new()));
    }

    let tokens = tokenize_hint(trimmed);
    if tokens.is_empty() {
        return Err(LocationLookupError::UnknownCountry {
            input: trimmed.to_string(),
        });
    }

    for prefix_len in (1..=tokens.len()).rev() {
        let candidate = tokens[..prefix_len].join(" ").to_ascii_lowercase();
        if let Some(code) = COUNTRY_LOOKUP.get(&candidate) {
            let mut hints: Vec<String> = tokens[prefix_len..]
                .iter()
                .map(|hint| hint.trim().to_ascii_uppercase())
                .filter(|hint| !hint.is_empty())
                .collect();
            hints.sort();
            hints.dedup();
            return Ok((Some((*code).to_string()), hints));
        }
    }

    Err(LocationLookupError::UnknownCountry {
        input: trimmed.to_string(),
    })
}

fn tokenize_hint(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for segment in value.split(|c: char| c == ',' || c == ';') {
        for token in segment.split(|c: char| c.is_ascii_whitespace() || c == '/') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            if token.contains('-') {
                tokens.extend(
                    token
                        .split('-')
                        .map(str::trim)
                        .filter(|part| !part.is_empty())
                        .map(|part| part.to_string()),
                );
            } else {
                tokens.push(token.to_string());
            }
        }
    }
    tokens
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

    #[test]
    fn resolves_with_state_hint() {
        let city = resolve_city("Dublin (US, OH)").expect("Dublin with state hint");
        assert_eq!(city.city_name, "Dublin");
        assert_eq!(city.country_code, "US");
        assert_eq!(city.latitude.round() as i32, 40); // Ohio location ~40.09923
    }
}
