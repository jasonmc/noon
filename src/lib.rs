mod lookup;
mod solar;

use chrono_tz::Tz;

#[derive(Copy, Clone, Debug)]
pub struct Location {
    pub timezone: Tz,
    pub longitude: f64,
}

impl Location {
    pub fn new(timezone: Tz, longitude: f64) -> Self {
        Self {
            timezone,
            longitude,
        }
    }
}

pub use lookup::{CityLocation, LocationLookupError, resolve_city};
pub use solar::solar_noon_datetime;
