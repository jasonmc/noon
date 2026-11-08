use crate::Location;
use crate::solar::{
    DaylightWindow, equation_of_time_minutes, solar_declination_degrees, solar_elevation_degrees,
    sunrise_sunset_datetimes,
};
use chrono::{DateTime, Datelike, Days, NaiveDate, Timelike};
use chrono_tz::Tz;
use std::fmt::Write;

const COLS: usize = 48;
const ROWS: usize = 13;
const STEPS_PER_HEMISPHERE: usize = 6;
const DEG_PER_ROW: f64 = 90.0 / STEPS_PER_HEMISPHERE as f64;
const HORIZON_ROW: usize = STEPS_PER_HEMISPHERE;
const ANA_COLS: usize = 25;
const ANA_MID: usize = ANA_COLS / 2;
const MAX_DECL: f64 = 23.44;
const Y_LABELS: [&str; ROWS] = [
    "  +90° ┤",
    "       │",
    "  +60° ┤",
    "       │",
    "  +30° ┤",
    "       │",
    "    0° ┼",
    "       │",
    "  -30° ┤",
    "       │",
    "  -60° ┤",
    "       │",
    "  -90° ┤",
];

/// Renders a 24-hour solar elevation arc alongside the annual solar analemma.
pub fn render_solar_arc(
    location: Location,
    date: NaiveDate,
    daylight: &DaylightWindow,
    solar_noon: DateTime<Tz>,
    current_time: Option<DateTime<Tz>>,
    use_color: bool,
) -> String {
    let noon_min = to_minutes(&solar_noon);
    let noon_col = to_col(noon_min);
    let elev_at = |min: f64| solar_elevation_degrees(location.latitude, date, noon_min, min);
    let noon_elev = elev_at(noon_min);

    let daylight_cols = match daylight {
        DaylightWindow::Daylight { sunrise, sunset } => {
            let sr = to_col(to_minutes(sunrise)).min(noon_col.saturating_sub(1));
            let ss = to_col(to_minutes(sunset)).max((noon_col + 1).min(COLS - 1));
            Some((sr, ss))
        }
        _ => None,
    };

    let mut points = [(HORIZON_ROW, " ", ""); COLS];
    for (col, point) in points.iter_mut().enumerate() {
        if daylight_cols.is_some_and(|(sr, _)| col == sr) {
            *point = (HORIZON_ROW, "▲", "\x1b[1;38;5;214m");
            continue;
        }
        if daylight_cols.is_some_and(|(_, ss)| col == ss) {
            *point = (HORIZON_ROW, "▼", "\x1b[1;38;5;208m");
            continue;
        }

        let is_day = match (daylight, daylight_cols) {
            (DaylightWindow::Daylight { .. }, Some((sr, ss))) => col > sr && col < ss,
            (DaylightWindow::SunNeverSets, _) => true,
            _ => false,
        };

        let sample_min = if col == noon_col {
            noon_min
        } else {
            (col + 1) as f64 * 30.0
        };
        let step = ((elev_at(sample_min).abs() / DEG_PER_ROW).round() as usize)
            .clamp(1, STEPS_PER_HEMISPHERE);
        let row = if is_day {
            HORIZON_ROW - step
        } else {
            HORIZON_ROW + step
        };

        *point = if col == noon_col {
            (row, "●", "\x1b[1;93m")
        } else if is_day {
            (row, "•", "\x1b[38;5;220m")
        } else {
            (row, "·", "\x1b[38;5;68m")
        };
    }

    if let Some(now) = current_time {
        let col = to_col(to_minutes(&now));
        points[col].1 = "◉";
        points[col].2 = "\x1b[1;96m";
    }

    let sign = if location.latitude < 0.0 { -1.0 } else { 1.0 };
    let mut ana = [[(" ", ""); ANA_COLS]; ROWS];
    for row in &mut ana {
        row[ANA_MID] = ("│", "\x1b[90m");
    }
    for (c, cell) in ana[HORIZON_ROW].iter_mut().enumerate() {
        *cell = if c == ANA_MID {
            ("┼", "\x1b[90m")
        } else {
            ("─", "\x1b[90m")
        };
    }

    let ana_coords = |d: NaiveDate| {
        let r = ((1.0 - sign * solar_declination_degrees(d) / MAX_DECL) * HORIZON_ROW as f64)
            .round()
            .clamp(0.0, (ROWS - 1) as f64) as usize;
        let c = (ANA_MID as f64 + equation_of_time_minutes(d) / 1.5)
            .round()
            .clamp(0.0, (ANA_COLS - 1) as f64) as usize;
        (r, c)
    };
    for d in (1..=366).filter_map(|ord| NaiveDate::from_yo_opt(date.year(), ord)) {
        let (r, c) = ana_coords(d);
        ana[r][c] = if r < HORIZON_ROW {
            ("•", "\x1b[38;5;220m")
        } else {
            ("·", "\x1b[38;5;68m")
        };
    }
    let (tr, tc) = ana_coords(date);
    ana[tr][tc] = ("◉", "\x1b[1;96m");

    let mut out = String::new();
    for (row, label) in Y_LABELS.iter().enumerate() {
        paint(&mut out, label, "\x1b[90m", use_color);
        for &(pt_row, glyph, color) in &points {
            if pt_row == row {
                paint(&mut out, glyph, color, use_color);
            } else if row == HORIZON_ROW {
                paint(&mut out, "─", "\x1b[90m", use_color);
            } else {
                out.push(' ');
            }
        }

        let decl = sign * MAX_DECL * (1.0 - row as f64 / HORIZON_ROW as f64);
        let ana_elev = 90.0 - (location.latitude - decl).abs();
        let ana_label = if row == HORIZON_ROW {
            format!("     {ana_elev:+3.0}° ┼")
        } else if row % 2 == 0 {
            format!("     {ana_elev:+3.0}° ┤")
        } else {
            "          │".to_string()
        };
        paint(&mut out, &ana_label, "\x1b[90m", use_color);
        for &(glyph, color) in &ana[row] {
            paint(&mut out, glyph, color, use_color);
        }
        out.push('\n');
    }

    paint(
        &mut out,
        "       └─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┴          └──┴────┴────┼────┴────┴──\n",
        "\x1b[90m",
        use_color,
    );
    paint(
        &mut out,
        "     00:00 03:00 06:00 09:00 12:00 15:00 18:00 21:00 24:00         -15m  -7m   0m  +7m  +15m\n",
        "\x1b[90m",
        use_color,
    );

    let eot = equation_of_time_minutes(date);
    out.push_str("       ");
    match daylight {
        DaylightWindow::Daylight { sunrise, sunset } => {
            paint(&mut out, "▲", "\x1b[1;38;5;214m", use_color);
            out.push_str(" Sunrise  ");
            paint(&mut out, "●", "\x1b[1;93m", use_color);
            let _ = write!(out, " Solar noon ({noon_elev:+.1}°, EoT {eot:+.1}m)  ");
            paint(&mut out, "▼", "\x1b[1;38;5;208m", use_color);
            let mins = (*sunset - *sunrise).num_minutes().max(0);
            let _ = write!(
                out,
                " Sunset  •  Daylight: {}h {:02}m",
                mins / 60,
                mins % 60
            );
        }
        DaylightWindow::SunNeverRises => {
            paint(&mut out, "●", "\x1b[1;93m", use_color);
            let _ = write!(
                out,
                " Solar noon ({noon_elev:+.1}°, EoT {eot:+.1}m)  •  Polar night (0h 00m daylight)"
            );
        }
        DaylightWindow::SunNeverSets => {
            paint(&mut out, "●", "\x1b[1;93m", use_color);
            let _ = write!(
                out,
                " Solar noon ({noon_elev:+.1}°, EoT {eot:+.1}m)  •  Midnight sun (24h 00m daylight)"
            );
        }
    }

    if let Some(now) = current_time {
        let now_elev = elev_at(to_minutes(&now));
        out.push_str("  ");
        paint(&mut out, "◉", "\x1b[1;96m", use_color);
        let _ = write!(out, " Now ({}, {now_elev:+.1}°)", now.format("%H:%M"));
    }

    if sign * solar_declination_degrees(date) < 0.0
        && let DaylightWindow::Daylight { sunrise, sunset } = daylight
    {
        append_winter_outlook(&mut out, location, date, sunrise, sunset, sign, use_color);
    }

    out
}

fn append_winter_outlook(
    out: &mut String,
    location: Location,
    date: NaiveDate,
    sunrise: &DateTime<Tz>,
    sunset: &DateTime<Tz>,
    sign: f64,
    use_color: bool,
) {
    let day_span = |d: NaiveDate| -> Option<(i64, i64)> {
        match sunrise_sunset_datetimes(location, d).ok()? {
            DaylightWindow::Daylight { sunrise, sunset } => {
                let dur = (sunset - sunrise).num_seconds().max(0);
                let set_sec = i64::from(sunset.num_seconds_from_midnight());
                Some((dur, set_sec))
            }
            _ => None,
        }
    };

    let today_secs = (*sunset - *sunrise).num_seconds().max(0);
    let today_set_sec = i64::from(sunset.num_seconds_from_midnight());

    // Locate the winter solstice within ±100 days of `date`.
    let solstice = (-100i64..=100)
        .filter_map(|off| {
            if off >= 0 {
                date.checked_add_days(Days::new(off as u64))
            } else {
                date.checked_sub_days(Days::new((-off) as u64))
            }
        })
        .min_by(|&a, &b| {
            let da = sign
                * (solar_declination_degrees(a)
                    + solar_declination_degrees(a.succ_opt().unwrap_or(a)));
            let db = sign
                * (solar_declination_degrees(b)
                    + solar_declination_degrees(b.succ_opt().unwrap_or(b)));
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(date);

    let Some((min_secs, _)) = day_span(solstice) else {
        return;
    };

    let summer_solstice = solstice
        .checked_sub_days(Days::new(182))
        .unwrap_or(solstice);
    let max_secs = day_span(summer_solstice)
        .map(|(s, _)| s)
        .unwrap_or(today_secs);

    // Earliest sunset occurs within ~20 days before the winter solstice.
    let (earliest_set_date, earliest_set_sec) = (-25i64..=5)
        .filter_map(|off| {
            let d = if off >= 0 {
                solstice.checked_add_days(Days::new(off as u64))?
            } else {
                solstice.checked_sub_days(Days::new((-off) as u64))?
            };
            let (_, set_sec) = day_span(d)?;
            Some((d, set_sec))
        })
        .min_by_key(|&(_, set_sec)| set_sec)
        .unwrap_or((solstice, today_set_sec));

    out.push_str("\n       ");
    paint(out, "❄", "\x1b[38;5;68m", use_color);

    if date < solstice {
        if date < earliest_set_date {
            let days = (earliest_set_date - date).num_days();
            let _ = write!(
                out,
                " Earliest sunset: {} at {:02}:{:02} (in {days}d)  •  ",
                earliest_set_date.format("%b %d"),
                earliest_set_sec / 3600,
                (earliest_set_sec % 3600) / 60,
            );
        } else {
            let gained_sec = (today_set_sec - earliest_set_sec).max(0);
            if gained_sec >= 60 {
                let _ = write!(
                    out,
                    " Earliest sunset passed {} (+{}m later now)  •  ",
                    earliest_set_date.format("%b %d"),
                    gained_sec / 60,
                );
            } else {
                let _ = write!(
                    out,
                    " Earliest sunset passed {} (+{gained_sec}s later now)  •  ",
                    earliest_set_date.format("%b %d"),
                );
            }
        }

        let days_to_sol = (solstice - date).num_days();
        let rem_mins = ((today_secs - min_secs).max(0) + 59) / 60;
        let pct_done = if max_secs > min_secs {
            ((max_secs - today_secs) as f64 / (max_secs - min_secs) as f64 * 100.0)
                .round()
                .clamp(0.0, 99.0) as u32
        } else {
            100
        };
        let min_total_mins = min_secs / 60;
        let _ = write!(
            out,
            "Solstice: {} ({}h {:02}m, in {days_to_sol}d — {rem_mins}m left, {pct_done}% of drop done)",
            solstice.format("%b %d"),
            min_total_mins / 60,
            min_total_mins % 60,
        );

        if let Some(mirror) = (1u64..=100)
            .filter_map(|off| solstice.checked_add_days(Days::new(off)))
            .find(|&d| day_span(d).is_some_and(|(s, _)| s >= today_secs))
        {
            let days_to_mirror = (mirror - date).num_days();
            let today_mins = today_secs / 60;
            let _ = write!(
                out,
                "  •  {}h {:02}m returns: {} (in {days_to_mirror}d)",
                today_mins / 60,
                today_mins % 60,
                mirror.format("%b %d"),
            );
        }
    } else {
        let gained_mins = (today_secs - min_secs).max(0) / 60;
        let gained_set = (today_set_sec - earliest_set_sec).max(0) / 60;
        let _ = write!(
            out,
            " Solstice passed {} (+{}h {:02}m daylight gained, sunset +{gained_set}m later)",
            solstice.format("%b %d"),
            gained_mins / 60,
            gained_mins % 60,
        );

        if let Some(equinox) = (1u64..=100)
            .filter_map(|off| date.checked_add_days(Days::new(off)))
            .find(|&d| {
                sign * (solar_declination_degrees(d)
                    + solar_declination_degrees(d.succ_opt().unwrap_or(d)))
                    >= 0.0
            })
        {
            let days_to_eq = (equinox - date).num_days();
            let _ = write!(
                out,
                "  •  Spring equinox: {} (in {days_to_eq}d)",
                equinox.format("%b %d"),
            );
        }
    }
}

fn to_minutes(dt: &DateTime<Tz>) -> f64 {
    f64::from(dt.hour()) * 60.0 + f64::from(dt.minute()) + f64::from(dt.second()) / 60.0
}

fn to_col(minutes: f64) -> usize {
    ((minutes / 30.0).round() as usize)
        .saturating_sub(1)
        .min(COLS - 1)
}

fn paint(out: &mut String, text: &str, color: &str, use_color: bool) {
    if use_color && !color.is_empty() {
        let _ = write!(out, "{color}{text}\x1b[0m");
    } else {
        out.push_str(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solar::{solar_noon_datetime, sunrise_sunset_datetimes};
    use chrono::TimeZone;
    use chrono_tz::UTC;

    #[test]
    fn renders_equatorial_equinox_arc_without_color() {
        let location = Location::new(UTC, 0.0, 0.0);
        let date = NaiveDate::from_ymd_opt(2024, 3, 20).expect("valid date");
        let daylight = sunrise_sunset_datetimes(location, date).expect("daylight");
        let noon = solar_noon_datetime(location, date).expect("solar noon");

        let rendered = render_solar_arc(location, date, &daylight, noon, None, false);

        assert!(rendered.contains('▲'));
        assert!(rendered.contains('●'));
        assert!(rendered.contains('▼'));
        assert!(rendered.contains("00:00 03:00 06:00 09:00 12:00 15:00 18:00 21:00 24:00"));
        assert!(rendered.contains("-15m  -7m   0m  +7m  +15m"));
        assert!(!rendered.contains("\x1b["));
    }

    #[test]
    fn includes_now_marker_when_current_time_provided() {
        let location = Location::new(UTC, 40.7128, -74.0060);
        let date = NaiveDate::from_ymd_opt(2024, 6, 21).expect("valid date");
        let daylight = sunrise_sunset_datetimes(location, date).expect("daylight");
        let noon = solar_noon_datetime(location, date).expect("solar noon");
        let now = UTC.with_ymd_and_hms(2024, 6, 21, 15, 30, 0).single();

        let rendered = render_solar_arc(location, date, &daylight, noon, now, false);
        assert!(rendered.contains('◉'));
        assert!(rendered.contains("Now (15:30,"));
    }

    #[test]
    fn handles_polar_night_and_midnight_sun() {
        let date = NaiveDate::from_ymd_opt(2024, 6, 21).expect("valid date");

        let arctic = Location::new(UTC, 78.0, 15.0);
        let arctic_daylight = sunrise_sunset_datetimes(arctic, date).expect("daylight");
        let arctic_noon = solar_noon_datetime(arctic, date).expect("noon");
        let midnight_sun_chart =
            render_solar_arc(arctic, date, &arctic_daylight, arctic_noon, None, false);
        assert!(midnight_sun_chart.contains("Midnight sun (24h 00m daylight)"));
        assert!(!midnight_sun_chart.contains('▲'));

        let antarctic = Location::new(UTC, -78.0, 15.0);
        let antarctic_daylight = sunrise_sunset_datetimes(antarctic, date).expect("daylight");
        let antarctic_noon = solar_noon_datetime(antarctic, date).expect("noon");
        let polar_night_chart = render_solar_arc(
            antarctic,
            date,
            &antarctic_daylight,
            antarctic_noon,
            None,
            false,
        );
        assert!(polar_night_chart.contains("Polar night (0h 00m daylight)"));
        assert!(!polar_night_chart.contains('▲'));
    }

    #[test]
    fn aligns_hours_with_x_axis_ticks() {
        assert_eq!(to_col(3.0 * 60.0), 5);
        assert_eq!(to_col(6.0 * 60.0), 11);
        assert_eq!(to_col(12.0 * 60.0), 23);
        assert_eq!(to_col(18.0 * 60.0), 35);
        assert_eq!(to_col(24.0 * 60.0), 47);
    }

    #[test]
    fn places_solar_noon_on_expected_y_axis_row() {
        let nyc = Location::new(UTC, 40.7128, -74.0060);
        let noon_row_for = |location: Location, y: i32, m: u32, d: u32| -> usize {
            let date = NaiveDate::from_ymd_opt(y, m, d).expect("valid date");
            let daylight = sunrise_sunset_datetimes(location, date).expect("daylight");
            let noon = solar_noon_datetime(location, date).expect("noon");
            let rendered = render_solar_arc(location, date, &daylight, noon, None, false);
            rendered
                .lines()
                .position(|line| line.contains('●'))
                .expect("chart should contain solar noon marker")
        };

        // Equator equinox (~90°) -> Row 0 (+90°)
        assert_eq!(noon_row_for(Location::new(UTC, 0.0, 0.0), 2024, 3, 20), 0);
        // NYC summer solstice (~72.7°) -> Row 1 (+75°)
        assert_eq!(noon_row_for(nyc, 2024, 6, 21), 1);
        // NYC vernal equinox (~49.1°) -> Row 3 (+45°)
        assert_eq!(noon_row_for(nyc, 2024, 3, 20), 3);
        // NYC winter solstice (~25.9°) -> Row 4 (+30°)
        assert_eq!(noon_row_for(nyc, 2024, 12, 21), 4);
    }

    #[test]
    fn includes_winter_outlook_only_in_autumn_and_winter() {
        let nyc = Location::new(chrono_tz::America::New_York, 40.71427, -74.00597);
        let render_on = |y: i32, m: u32, d: u32| {
            let date = NaiveDate::from_ymd_opt(y, m, d).expect("valid date");
            let daylight = sunrise_sunset_datetimes(nyc, date).expect("daylight");
            let noon = solar_noon_datetime(nyc, date).expect("noon");
            render_solar_arc(nyc, date, &daylight, noon, None, false)
        };

        // Phase 1: Before earliest sunset (Nov 7) -> counts down to earliest sunset, solstice, and mirror date.
        let nov = render_on(2026, 11, 7);
        assert!(nov.contains("Earliest sunset: Dec 08 at 16:28 (in 31d)"));
        assert!(nov.contains("Solstice: Dec 21 (9h 15m, in 44d — 59m left, 83% of drop done)"));
        assert!(nov.contains("10h 13m returns: Feb 05 (in 90d)"));

        // Phase 2: Between earliest sunset and winter solstice (Dec 15) -> earliest sunset already passed!
        let dec = render_on(2026, 12, 15);
        assert!(dec.contains("Earliest sunset passed Dec 08 (+51s later now)"));
        assert!(dec.contains("Solstice: Dec 21 (9h 15m, in 6d — 2m left, 99% of drop done)"));
        assert!(dec.contains("9h 16m returns: Dec 29 (in 14d)"));

        // Phase 3: After winter solstice (Jan 15) -> reports daylight gained and counts down to spring equinox.
        let jan = render_on(2027, 1, 15);
        assert!(
            jan.contains("Solstice passed Dec 21 (+0h 19m daylight gained, sunset +23m later)")
        );
        assert!(jan.contains("Spring equinox: Mar 21 (in 65d)"));

        // Summer (Jun 21) -> no winter outlook line rendered.
        let jun = render_on(2026, 6, 21);
        assert!(!jun.contains("Earliest sunset"));
        assert!(!jun.contains("Solstice"));
    }

    #[test]
    fn winter_outlook_and_analemma_adapt_to_southern_hemisphere() {
        let sydney = Location::new(chrono_tz::Australia::Sydney, -33.8688, 151.2093);
        let render_sydney = |y: i32, m: u32, d: u32| {
            let date = NaiveDate::from_ymd_opt(y, m, d).expect("valid date");
            let daylight = sunrise_sunset_datetimes(sydney, date).expect("daylight");
            let noon = solar_noon_datetime(sydney, date).expect("noon");
            render_solar_arc(sydney, date, &daylight, noon, None, false)
        };

        // In Sydney, May is late autumn (approaching the June winter solstice).
        let may = render_sydney(2026, 5, 15);
        assert!(may.contains("+80° ┤"));
        assert!(may.contains("+33° ┤"));
        assert!(may.contains("Earliest sunset: Jun"));
        assert!(may.contains("Solstice: Jun 21"));

        // July in Sydney is after the June winter solstice, climbing toward the September spring equinox.
        let jul = render_sydney(2026, 7, 15);
        assert!(jul.contains("Solstice passed Jun 21"));
        assert!(jul.contains("Spring equinox: Sep 23"));

        // December in Sydney is summer -> no winter outlook.
        let dec = render_sydney(2026, 12, 21);
        assert!(!dec.contains("Solstice"));
    }

    #[test]
    fn analemma_places_target_marker_on_expected_row_and_lobe() {
        let nyc = Location::new(UTC, 40.7128, -74.0060);
        let analemma_marker_pos = |y: i32, m: u32, d: u32| -> (usize, usize) {
            let date = NaiveDate::from_ymd_opt(y, m, d).expect("valid date");
            let daylight = sunrise_sunset_datetimes(nyc, date).expect("daylight");
            let noon = solar_noon_datetime(nyc, date).expect("noon");
            let rendered = render_solar_arc(nyc, date, &daylight, noon, None, false);
            rendered
                .lines()
                .take(ROWS)
                .enumerate()
                .find_map(|(r, line)| line.find('◉').map(|byte_idx| (r, byte_idx)))
                .expect("analemma should contain ◉ target date marker")
        };

        let (jun_row, _) = analemma_marker_pos(2026, 6, 21);
        let (dec_row, _) = analemma_marker_pos(2026, 12, 21);
        let (_, feb_byte) = analemma_marker_pos(2026, 2, 11);
        let (_, nov_byte) = analemma_marker_pos(2026, 11, 3);

        // June solstice sits at the top of the analemma (row 0); December solstice sits at the bottom (row 12).
        assert_eq!(jun_row, 0);
        assert_eq!(dec_row, ROWS - 1);
        // February EoT (-14.2m) is on the left lobe; November EoT (+16.4m) is on the right lobe.
        assert!(feb_byte < nov_byte);
    }
}
