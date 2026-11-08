use crate::Location;
use crate::solar::{
    DaylightWindow, equation_of_time_minutes, solar_declination_degrees, solar_elevation_degrees,
};
use chrono::{DateTime, Datelike, NaiveDate, Timelike};
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

    out
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
}
