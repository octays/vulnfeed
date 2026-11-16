use chrono::{DateTime, Local, NaiveDate};

/// Whether `date` parses to a local day at or after `min_date`. Callers pass a
/// rolling cutoff (e.g. yesterday) rather than the calendar today so items
/// published in the evening still qualify when scraped after local midnight.
pub fn eligible_for_push(date: &str, push_history: bool, min_date: NaiveDate) -> bool {
    if push_history {
        return true;
    }
    let date = date.trim();
    let parsed = DateTime::parse_from_rfc3339(date)
        .ok()
        .map(|d| d.with_timezone(&Local).date_naive())
        .or_else(|| {
            let date_part = date.split(['T', ' ']).next().unwrap_or(date);
            ["%Y-%m-%d", "%Y/%m/%d", "%Y年%m月%d日"]
                .iter()
                .find_map(|format| NaiveDate::parse_from_str(date_part, format).ok())
        })
        .or_else(|| {
            ["%d %B %Y", "%B %d, %Y", "%b %d, %Y", "%d %b %Y"]
                .iter()
                .find_map(|format| NaiveDate::parse_from_str(date, format).ok())
        });
    match parsed {
        Some(date) => date >= min_date,
        None => {
            log::warn!("Cannot determine publication date {date:?}; skipping automatic push");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_historical_and_unknown_dates() {
        let min_date = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        for date in ["2026-10-28", "28 October 2026", "", "2026年10月"] {
            assert!(!eligible_for_push(date, false, min_date), "{date}");
            assert!(eligible_for_push(date, true, min_date), "{date}");
        }
        for date in [
            "2026-10-29",
            "2026-10-30 12:34:56",
            "October 29, 2026",
            "2026年10月29日",
        ] {
            assert!(eligible_for_push(date, false, min_date), "{date}");
        }
    }
}
