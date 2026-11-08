use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use clap::Parser;
use noon::{
    DaylightWindow, LocationLookupError, SolarComputationError, render_solar_arc, resolve_city,
    solar_noon_datetime, sunrise_sunset_datetimes,
};
use std::io::IsTerminal;

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
    let now_in_tz = Utc::now().with_timezone(&timezone);
    let target_date = cli.date.unwrap_or_else(|| now_in_tz.date_naive());
    let current_time = (target_date == now_in_tz.date_naive()).then_some(now_in_tz);

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

    let use_color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let chart = render_solar_arc(
        resolved.location,
        target_date,
        &daylight,
        local_noon,
        current_time,
        use_color,
    );

    println!(
        "      {}, {}  •  {}  •  {} ({})",
        resolved.city_name,
        resolved.country_code,
        target_date.format("%Y-%m-%d"),
        resolved.timezone_name,
        local_noon.format("%Z"),
    );
    println!("      {}", describe_daylight(&daylight, &local_noon));
    println!();
    println!("{chart}");
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

fn describe_daylight(daylight: &DaylightWindow, local_noon: &DateTime<Tz>) -> String {
    let noon = local_noon.format("%H:%M:%S");
    match daylight {
        DaylightWindow::Daylight { sunrise, sunset } => format!(
            "Sunrise: {}  •  Solar noon: {noon}  •  Sunset: {}",
            sunrise.format("%H:%M:%S"),
            sunset.format("%H:%M:%S"),
        ),
        DaylightWindow::SunNeverRises => {
            format!("Solar noon: {noon}  •  Sun remains below the horizon all day")
        }
        DaylightWindow::SunNeverSets => {
            format!("Solar noon: {noon}  •  Sun remains above the horizon all day")
        }
    }
}
