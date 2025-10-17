use chrono::offset::Offset;
use chrono::{DateTime, Datelike, LocalResult, NaiveDate, TimeZone};
use chrono_tz::{America::New_York, Tz};
use std::f64::consts::PI;

pub const LONGITUDE_NYC: f64 = -77.006;
const MINUTES_PER_DAY: f64 = 1_440.0;
const SECONDS_PER_DAY: f64 = 86_400.0;

pub fn solar_noon_datetime(date: NaiveDate) -> DateTime<Tz> {
    let tz: Tz = New_York;
    let timezone_hours = timezone_offset_hours(tz, date);
    let jd = julian_day(date.year(), date.month() as i32, date.day() as i32);
    let sol_noon_minutes = calc_sol_noon(jd, LONGITUDE_NYC, timezone_hours);
    let (hour, minute, second) = minutes_to_hms(sol_noon_minutes);

    build_local_datetime(tz, date, hour, minute, second)
}

fn timezone_offset_hours(tz: Tz, date: NaiveDate) -> f64 {
    let local_midday = match tz.with_ymd_and_hms(date.year(), date.month(), date.day(), 12, 0, 0) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt,
        LocalResult::None => panic!("local noon is not representable for {}", date),
    };

    local_midday.offset().fix().local_minus_utc() as f64 / 3600.0
}

fn build_local_datetime(tz: Tz, date: NaiveDate, hour: u32, minute: u32, second: u32) -> DateTime<Tz> {
    match tz.with_ymd_and_hms(date.year(), date.month(), date.day(), hour, minute, second) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt,
        LocalResult::None => panic!("unable to resolve solar noon in local time for {}", date),
    }
}

fn minutes_to_hms(minutes: f64) -> (u32, u32, u32) {
    let total_seconds = (minutes * 60.0).round().rem_euclid(SECONDS_PER_DAY) as i64;
    let hour = (total_seconds / 3_600) as u32;
    let minute = ((total_seconds % 3_600) / 60) as u32;
    let second = (total_seconds % 60) as u32;

    (hour, minute, second)
}

fn julian_day(year: i32, month: i32, day: i32) -> f64 {
    let mut y = year as f64;
    let mut m = month as f64;
    let d = day as f64;

    if m <= 2.0 {
        y -= 1.0;
        m += 12.0;
    }

    let a = (y / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();

    (365.25 * (y + 4716.0)).floor()
        + (30.6001 * (m + 1.0)).floor()
        + d
        + b
        - 1524.5
}

fn calc_sol_noon(jd: f64, longitude: f64, timezone: f64) -> f64 {
    let t_noon = calc_time_julian_cent(jd - longitude / 360.0);
    let mut eq_time = calc_equation_of_time(t_noon);
    let sol_noon_offset = 720.0 - (longitude * 4.0) - eq_time;

    let new_t = calc_time_julian_cent(jd - 0.5 + sol_noon_offset / MINUTES_PER_DAY);
    eq_time = calc_equation_of_time(new_t);

    (720.0 - (longitude * 4.0) - eq_time + (timezone * 60.0)).rem_euclid(MINUTES_PER_DAY)
}

fn calc_time_julian_cent(jd: f64) -> f64 {
    (jd - 2_451_545.0) / 36_525.0
}

fn calc_equation_of_time(t: f64) -> f64 {
    let epsilon = calc_obliquity_correction(t);
    let l0 = calc_geom_mean_long_sun(t);
    let e = calc_eccentricity_earth_orbit(t);
    let m = calc_geom_mean_anomaly_sun(t);

    let y = (deg_to_rad(epsilon) / 2.0).tan().powi(2);
    let sin2l0 = (2.0 * deg_to_rad(l0)).sin();
    let sinm = deg_to_rad(m).sin();
    let cos2l0 = (2.0 * deg_to_rad(l0)).cos();
    let sin4l0 = (4.0 * deg_to_rad(l0)).sin();
    let sin2m = (2.0 * deg_to_rad(m)).sin();

    let e_time = y * sin2l0
        - 2.0 * e * sinm
        + 4.0 * e * y * sinm * cos2l0
        - 0.5 * y * y * sin4l0
        - 1.25 * e * e * sin2m;

    rad_to_deg(e_time) * 4.0
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

    fn assert_solar_noon(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) {
        let date = NaiveDate::from_ymd_opt(year, month, day).expect("valid date");
        let actual = solar_noon_datetime(date);
        let expected = New_York
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .expect("expected instant to exist");

        assert_eq!(
            actual, expected,
            "solar noon mismatch on {} (expected {:02}:{:02}:{:02}, got {})",
            date,
            hour,
            minute,
            second,
            actual.format("%H:%M:%S %Z")
        );
    }

    #[test]
    fn matches_noaa_october_2025() {
        assert_solar_noon(2025, 10, 17, 12, 53, 22);
    }

    #[test]
    fn matches_noaa_december_2024() {
        assert_solar_noon(2024, 12, 21, 12, 6, 12);
    }

    #[test]
    fn matches_noaa_june_2024() {
        assert_solar_noon(2024, 6, 21, 13, 9, 53);
    }
}
