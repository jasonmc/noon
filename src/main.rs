use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use clap::Parser;
use noon::{
    DaylightWindow, LocationLookupError, SolarComputationError, resolve_city, solar_noon_datetime,
    sunrise_sunset_datetimes,
};

const DEFAULT_CITY: &str = "New York City";

#[derive(Parser, Debug)]
#[command(
    name = "noon",
    about = "Report sunrise, solar noon, and sunset for a city and date"
)]
struct Cli {
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date)]
    date: Option<NaiveDate>,

    #[arg(value_name = "CITY", num_args = 0..)]
    city: Vec<String>,
}

fn main() {
    let cli = Cli::parse();
    let city_query = if cli.city.is_empty() {
        DEFAULT_CITY.to_string()
    } else {
        cli.city.join(" ")
    };

    let resolved = match resolve_city(&city_query) {
        Ok(location) => location,
        Err(err) => {
            report_city_error(&city_query, &err);
            std::process::exit(1);
        }
    };

    let timezone = resolved.location.timezone;
    let target_date = cli
        .date
        .unwrap_or_else(|| Utc::now().with_timezone(&timezone).date_naive());

    let daylight = match sunrise_sunset_datetimes(resolved.location, target_date) {
        Ok(daylight) => daylight,
        Err(err) => {
            report_solar_error(&resolved.city_name, target_date, &err);
            std::process::exit(1);
        }
    };
    let local_noon = match solar_noon_datetime(resolved.location, target_date) {
        Ok(local_noon) => local_noon,
        Err(err) => {
            report_solar_error(&resolved.city_name, target_date, &err);
            std::process::exit(1);
        }
    };

    let (sunrise_text, sunset_text) = describe_daylight(&daylight);

    println!(
        "Solar events for {}, {} on {} in {}:",
        resolved.city_name,
        resolved.country_code,
        target_date.format("%Y-%m-%d"),
        resolved.timezone_name
    );
    println!("  Sunrise: {}", sunrise_text);
    println!(
        "  Solar noon: {} ({})",
        local_noon.format("%H:%M:%S"),
        local_noon.format("%Z")
    );
    println!("  Sunset: {}", sunset_text);
}

fn parse_date(input: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(input, "%Y-%m-%d")
        .map_err(|_| format!("invalid date '{input}', expected YYYY-MM-DD"))
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

fn report_solar_error(city: &str, date: NaiveDate, error: &SolarComputationError) {
    eprintln!(
        "Unable to compute solar events for '{}' on {}: {}",
        city, date, error
    );
}

fn describe_daylight(daylight: &DaylightWindow) -> (String, String) {
    match daylight {
        DaylightWindow::Daylight { sunrise, sunset } => {
            (format_timestamp(sunrise), format_timestamp(sunset))
        }
        DaylightWindow::SunNeverRises => {
            let msg = "Sun remains below the horizon all day".to_string();
            (msg.clone(), msg)
        }
        DaylightWindow::SunNeverSets => {
            let msg = "Sun remains above the horizon all day".to_string();
            (msg.clone(), msg)
        }
    }
}

fn format_timestamp(timestamp: &DateTime<Tz>) -> String {
    format!(
        "{} ({})",
        timestamp.format("%H:%M:%S"),
        timestamp.format("%Z")
    )
}
