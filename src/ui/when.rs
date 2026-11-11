use gettextrs::gettext;

/// The time for the last 24 hours, then "Yesterday" or the weekday for the last week, then a
/// short date.
pub fn last_refresh(then: &glib::DateTime, now: &glib::DateTime) -> String {
    let age = now.difference(then);
    let day = glib::TimeSpan::from_days(1);
    let format = if age < day {
        // An empty AM/PM marker means the locale uses a 24-hour clock.
        if then.format("%p").is_ok_and(|p| !p.is_empty()) {
            "%l:%M %p"
        } else {
            "%H:%M"
        }
    } else if age < glib::TimeSpan::from_days(7) {
        let yesterday = now.add_days(-1).ok();
        if yesterday.is_some_and(|y| y.ymd() == then.ymd()) {
            return gettext("Yesterday");
        }
        "%A"
    } else {
        "%x"
    };
    then.format(format)
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: i32, hour: i32, minute: i32) -> glib::DateTime {
        glib::DateTime::from_utc(2026, 11, day, hour, minute, 0.0).unwrap()
    }

    #[test]
    fn boundaries() {
        // The C locale uses a 12-hour clock and English names.
        let now = at(10, 16, 20);
        assert_eq!(last_refresh(&at(10, 14, 5), &now), "2:05 PM");
        assert_eq!(last_refresh(&at(9, 16, 30), &now), "4:30 PM");
        assert_eq!(last_refresh(&at(9, 16, 20), &now), "Yesterday");
        assert_eq!(last_refresh(&at(8, 9, 0), &now), "Sunday");
        assert_eq!(last_refresh(&at(3, 16, 21), &now), "Tuesday");
        assert_eq!(last_refresh(&at(3, 16, 20), &now), "11/03/26");
    }
}
