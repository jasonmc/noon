mod lookup;
mod solar;
mod viz;

use chrono_tz::Tz;

#[derive(Copy, Clone, Debug)]
pub struct Location {
    pub timezone: Tz,
    pub latitude: f64,
    pub longitude: f64,
}

impl Location {
    pub fn new(timezone: Tz, latitude: f64, longitude: f64) -> Self {
        Self {
            timezone,
            latitude,
            longitude,
        }
    }
}

pub use lookup::{CityLocation, LocationLookupError, resolve_city};
pub use solar::{
    DaylightWindow, SolarComputationError, solar_elevation_degrees, solar_noon_datetime,
    sunrise_sunset_datetimes,
};
pub use viz::render_solar_arc;
