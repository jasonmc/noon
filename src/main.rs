use chrono::{NaiveDate, Utc};
use chrono_tz::America::New_York;
use noon::solar_noon_datetime;
use std::env;

fn main() {
    let tz = New_York;
    let args: Vec<String> = env::args().collect();
    let target_date = if args.len() > 1 {
        match NaiveDate::parse_from_str(&args[1], "%Y-%m-%d") {
            Ok(date) => date,
            Err(err) => {
                eprintln!(
                    "Unable to parse date '{}': {}. Use YYYY-MM-DD format.",
                    args[1], err
                );
                std::process::exit(1);
            }
        }
    } else {
        Utc::now().with_timezone(&tz).date_naive()
    };

    let local_noon = solar_noon_datetime(target_date);

    println!(
        "Solar noon for NYC on {} occurs at {} ({}).",
        target_date.format("%Y-%m-%d"),
        local_noon.format("%H:%M:%S"),
        local_noon.format("%Z")
    );
}
