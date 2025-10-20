use chrono::{NaiveDate, Utc};
use noon::{LocationLookupError, resolve_city, solar_noon_datetime};
use std::env;

const DEFAULT_CITY: &str = "New York City";

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();

    let date_override = match args.last().cloned() {
        Some(candidate) => match NaiveDate::parse_from_str(&candidate, "%Y-%m-%d") {
            Ok(date) => {
                args.pop();
                Some(date)
            }
            Err(_) => None,
        },
        None => None,
    };

    let city_query = if args.is_empty() {
        DEFAULT_CITY.to_string()
    } else {
        args.join(" ")
    };

    let resolved = match resolve_city(&city_query) {
        Ok(location) => location,
        Err(err) => {
            report_city_error(&city_query, &err);
            std::process::exit(1);
        }
    };

    let timezone = resolved.location.timezone;
    let target_date =
        date_override.unwrap_or_else(|| Utc::now().with_timezone(&timezone).date_naive());

    let local_noon = solar_noon_datetime(resolved.location, target_date);

    println!(
        "Solar noon for {}, {} on {} occurs at {} ({}) in {}.",
        resolved.city_name,
        resolved.country_code,
        target_date.format("%Y-%m-%d"),
        local_noon.format("%H:%M:%S"),
        local_noon.format("%Z"),
        resolved.timezone_name
    );
}

fn report_city_error(query: &str, error: &LocationLookupError) {
    eprintln!("Unable to resolve city '{}': {}", query, error);

    match error {
        LocationLookupError::Ambiguous { suggestions, .. } if !suggestions.is_empty() => {
            eprintln!("Did you mean:");
            for suggestion in suggestions {
                eprintln!("  - {}", suggestion);
            }
        }
        LocationLookupError::NotFoundInCountry {
            available_countries,
            ..
        } if !available_countries.is_empty() => {
            eprintln!(
                "Matching cities exist in countries: {}",
                available_countries.join(", ")
            );
        }
        _ => {}
    }
}
