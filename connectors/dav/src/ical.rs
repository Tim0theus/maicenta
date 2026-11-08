//! Mapping between MAICENTA calendar events and tasks and iCalendar
//! `VEVENT`/`VTODO` components (RFC 5545).
//!
//! Timed values are stored as absolute instants: a `DTSTART` with a `TZID`
//! is resolved through the IANA database, a trailing `Z` means UTC, and a
//! floating time is read in the local zone. All-day values (`VALUE=DATE`)
//! become local midnight boundaries, matching what the client stores.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use maicenta_domain::{
    CalendarEvent, Recurrence, RecurrenceFrequency, SyncMetadata, TaskItem, TaskPriority,
    TaskStatus, WorkspaceItemId,
};

use crate::text::{Component, ParseError, Property, escape_text, parse_components};

/// Product identifier written into generated calendars.
pub const PRODUCT_ID: &str = "-//MAICENTA//Workspace//DE";

/// Errors raised while converting iCalendar data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IcalError {
    Parse(ParseError),
    /// The document contains no usable `VEVENT` or `VTODO`.
    MissingComponent(&'static str),
    /// A required property is absent or has an unreadable value.
    InvalidProperty(String),
}

impl From<ParseError> for IcalError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl std::fmt::Display for IcalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "iCalendar parse error: {error}"),
            Self::MissingComponent(name) => write!(formatter, "no {name} component found"),
            Self::InvalidProperty(detail) => write!(formatter, "invalid property: {detail}"),
        }
    }
}

impl std::error::Error for IcalError {}

/// A parsed `DTSTART`/`DTEND`/`DUE` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Moment {
    /// Unix milliseconds of the instant (local midnight for dates).
    pub at_ms: i64,
    /// The property carried a `DATE` value rather than a `DATE-TIME`.
    pub date_only: bool,
}

/// Reads a date or date-time property into an instant.
///
/// # Errors
///
/// Returns [`IcalError::InvalidProperty`] when the value cannot be read.
pub fn parse_moment(property: &Property) -> Result<Moment, IcalError> {
    let value = property.value.trim();
    let date_only = property
        .param("VALUE")
        .is_some_and(|kind| kind.eq_ignore_ascii_case("DATE"))
        || (value.len() == 8 && !value.contains('T'));
    if date_only {
        let date = NaiveDate::parse_from_str(value, "%Y%m%d")
            .map_err(|_| IcalError::InvalidProperty(format!("{}: {value}", property.name)))?;
        return Ok(Moment {
            at_ms: local_midnight_ms(date),
            date_only: true,
        });
    }
    let (naive, utc) = match value.strip_suffix('Z') {
        Some(stripped) => (stripped, true),
        None => (value, false),
    };
    let naive = NaiveDateTime::parse_from_str(naive, "%Y%m%dT%H%M%S")
        .map_err(|_| IcalError::InvalidProperty(format!("{}: {value}", property.name)))?;
    let at_ms = if utc {
        Utc.from_utc_datetime(&naive).timestamp_millis()
    } else if let Some(tzid) = property.param("TZID") {
        match tzid.parse::<Tz>() {
            Ok(zone) => zone
                .from_local_datetime(&naive)
                .earliest()
                .map_or_else(
                    || Utc.from_utc_datetime(&naive),
                    |value| value.with_timezone(&Utc),
                )
                .timestamp_millis(),
            // Unknown zone names (e.g. Windows names) fall back to local time.
            Err(_) => local_datetime_ms(naive),
        }
    } else {
        local_datetime_ms(naive)
    };
    Ok(Moment {
        at_ms,
        date_only: false,
    })
}

fn local_midnight_ms(date: NaiveDate) -> i64 {
    let naive = date.and_hms_opt(0, 0, 0).expect("midnight");
    local_datetime_ms(naive)
}

fn local_datetime_ms(naive: NaiveDateTime) -> i64 {
    Local.from_local_datetime(&naive).earliest().map_or_else(
        || Utc.from_utc_datetime(&naive).timestamp_millis(),
        |value| value.timestamp_millis(),
    )
}

fn local_date(at_ms: i64) -> NaiveDate {
    DateTime::<Utc>::from_timestamp_millis(at_ms)
        .unwrap_or_default()
        .with_timezone(&Local)
        .date_naive()
}

fn utc_stamp(at_ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(at_ms)
        .unwrap_or_default()
        .format("%Y%m%dT%H%M%SZ")
        .to_string()
}

fn date_stamp(at_ms: i64) -> String {
    local_date(at_ms).format("%Y%m%d").to_string()
}

fn iso_date(at_ms: i64) -> String {
    local_date(at_ms).format("%Y-%m-%d").to_string()
}

fn weekday_token(weekday: u8) -> &'static str {
    match weekday {
        1 => "MO",
        2 => "TU",
        3 => "WE",
        4 => "TH",
        5 => "FR",
        6 => "SA",
        _ => "SU",
    }
}

fn weekday_from_token(token: &str) -> Option<u8> {
    // Tokens may carry an ordinal such as `-1FR`; only the plain form is
    // supported and others are ignored.
    match token.trim().to_ascii_uppercase().as_str() {
        "MO" => Some(1),
        "TU" => Some(2),
        "WE" => Some(3),
        "TH" => Some(4),
        "FR" => Some(5),
        "SA" => Some(6),
        "SU" => Some(7),
        _ => None,
    }
}

/// Reads an `RRULE` value. Unsupported parts are ignored so the event still
/// appears, with the frequency and interval the server sent.
fn parse_rrule(value: &str) -> Option<Recurrence> {
    let mut frequency = None;
    let mut interval = 1;
    let mut weekdays = Vec::new();
    let mut until_ms = None;
    let mut count = None;
    for part in value.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                frequency = match value.trim().to_ascii_uppercase().as_str() {
                    "DAILY" => Some(RecurrenceFrequency::Daily),
                    "WEEKLY" => Some(RecurrenceFrequency::Weekly),
                    "MONTHLY" => Some(RecurrenceFrequency::Monthly),
                    "YEARLY" => Some(RecurrenceFrequency::Yearly),
                    _ => return None,
                };
            }
            "INTERVAL" => interval = value.trim().parse().unwrap_or(1),
            "BYDAY" => {
                weekdays = value.split(',').filter_map(weekday_from_token).collect();
                weekdays.sort_unstable();
                weekdays.dedup();
            }
            "UNTIL" => {
                let property = Property::new("UNTIL", value.trim());
                until_ms = parse_moment(&property).ok().map(|moment| {
                    if moment.date_only {
                        // A date-only UNTIL includes the whole day.
                        moment.at_ms + 86_400_000 - 1
                    } else {
                        moment.at_ms
                    }
                });
            }
            "COUNT" => count = value.trim().parse().ok(),
            _ => {}
        }
    }
    Some(Recurrence {
        frequency: frequency?,
        interval: interval.max(1),
        weekdays,
        until_ms,
        count,
    })
}

fn rrule_value(recurrence: &Recurrence) -> String {
    let mut parts = vec![format!(
        "FREQ={}",
        match recurrence.frequency {
            RecurrenceFrequency::Daily => "DAILY",
            RecurrenceFrequency::Weekly => "WEEKLY",
            RecurrenceFrequency::Monthly => "MONTHLY",
            RecurrenceFrequency::Yearly => "YEARLY",
        }
    )];
    if recurrence.interval > 1 {
        parts.push(format!("INTERVAL={}", recurrence.interval));
    }
    if !recurrence.weekdays.is_empty() {
        parts.push(format!(
            "BYDAY={}",
            recurrence
                .weekdays
                .iter()
                .map(|day| weekday_token(*day))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if let Some(until) = recurrence.until_ms {
        parts.push(format!("UNTIL={}", utc_stamp(until)));
    }
    if let Some(count) = recurrence.count {
        parts.push(format!("COUNT={count}"));
    }
    parts.join(";")
}

/// Reads a `TRIGGER` duration such as `-PT15M` or `-P1D` into minutes
/// before the start. Triggers after the start are clamped to zero.
fn trigger_minutes_before(value: &str) -> Option<u32> {
    let value = value.trim();
    let (negative, rest) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let rest = rest.strip_prefix('P')?;
    let mut minutes: u64 = 0;
    let mut number = String::new();
    let mut in_time = false;
    for character in rest.chars() {
        match character {
            'T' => in_time = true,
            digit if digit.is_ascii_digit() => number.push(digit),
            unit => {
                let amount: u64 = number.parse().ok()?;
                number.clear();
                minutes += match (unit, in_time) {
                    ('W', _) => amount * 7 * 24 * 60,
                    ('D', _) => amount * 24 * 60,
                    ('H', true) => amount * 60,
                    ('M', true) => amount,
                    ('S', true) => amount / 60,
                    _ => return None,
                };
            }
        }
    }
    if !negative {
        return Some(0);
    }
    u32::try_from(minutes).ok()
}

fn trigger_value(minutes_before: u32) -> String {
    if minutes_before == 0 {
        return "PT0S".into();
    }
    if minutes_before % 1440 == 0 {
        return format!("-P{}D", minutes_before / 1440);
    }
    if minutes_before % 60 == 0 {
        return format!("-PT{}H", minutes_before / 60);
    }
    format!("-PT{minutes_before}M")
}

fn optional_text(component: &Component, name: &str) -> Option<String> {
    component
        .text(name)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Reads the first `VEVENT` of an iCalendar document into an event with the
/// given local identifier and synchronization metadata.
///
/// # Errors
///
/// Returns an [`IcalError`] when the document has no usable `VEVENT`.
pub fn event_from_ical(
    text: &str,
    id: WorkspaceItemId,
    sync: SyncMetadata,
) -> Result<CalendarEvent, IcalError> {
    let roots = parse_components(text)?;
    let calendar = roots
        .iter()
        .find(|component| component.name == "VCALENDAR")
        .ok_or(IcalError::MissingComponent("VCALENDAR"))?;
    // Recurrence exceptions (RECURRENCE-ID) are not edited separately; the
    // master event is the one without a RECURRENCE-ID.
    let event = calendar
        .children
        .iter()
        .filter(|child| child.name == "VEVENT")
        .find(|child| child.property("RECURRENCE-ID").is_none())
        .or_else(|| calendar.child("VEVENT"))
        .ok_or(IcalError::MissingComponent("VEVENT"))?;
    let start = parse_moment(
        event
            .property("DTSTART")
            .ok_or_else(|| IcalError::InvalidProperty("DTSTART missing".into()))?,
    )?;
    let end = match (event.property("DTEND"), event.property("DURATION")) {
        (Some(end), _) => parse_moment(end)?,
        (None, Some(duration)) => Moment {
            at_ms: start.at_ms + duration_ms(&duration.value).unwrap_or(0),
            date_only: start.date_only,
        },
        (None, None) => Moment {
            at_ms: if start.date_only {
                start.at_ms + 86_400_000
            } else {
                start.at_ms + 3_600_000
            },
            date_only: start.date_only,
        },
    };
    let mut ends_at_ms = end.at_ms;
    if ends_at_ms <= start.at_ms {
        ends_at_ms = start.at_ms
            + if start.date_only {
                86_400_000
            } else {
                1_800_000
            };
    }
    let reminder_minutes_before = event
        .children
        .iter()
        .filter(|child| child.name == "VALARM")
        .filter_map(|alarm| alarm.property("TRIGGER"))
        .filter(|trigger| {
            !trigger
                .param("VALUE")
                .is_some_and(|kind| kind.eq_ignore_ascii_case("DATE-TIME"))
        })
        .find_map(|trigger| trigger_minutes_before(&trigger.value));
    let mut exception_dates: Vec<String> = event
        .properties_named("EXDATE")
        .flat_map(|property| {
            let template = property.clone();
            property
                .value
                .split(',')
                .filter_map(|part| {
                    let mut single = template.clone();
                    part.trim().clone_into(&mut single.value);
                    parse_moment(&single).ok()
                })
                .map(|moment| iso_date(moment.at_ms))
                .collect::<Vec<_>>()
        })
        .collect();
    exception_dates.sort();
    exception_dates.dedup();
    let mut sync = sync;
    if sync.uid.is_none() {
        sync.uid = optional_text(event, "UID");
    }
    Ok(CalendarEvent {
        id,
        title: optional_text(event, "SUMMARY").unwrap_or_else(|| "(Ohne Titel)".into()),
        starts_at_ms: start.at_ms,
        ends_at_ms,
        all_day: start.date_only,
        location: optional_text(event, "LOCATION"),
        description: optional_text(event, "DESCRIPTION"),
        reminder_minutes_before,
        reminder_dismissed_through_ms: event
            .property("X-MAICENTA-REMINDER-DISMISSED-THROUGH")
            .and_then(|property| property.value.trim().parse().ok()),
        recurrence: event
            .property("RRULE")
            .and_then(|property| parse_rrule(&property.value)),
        exception_dates,
        sync,
    })
}

/// Reads an iCalendar `DURATION` into milliseconds.
fn duration_ms(value: &str) -> Option<i64> {
    let value = value.trim();
    let (negative, rest) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let rest = rest.strip_prefix('P')?;
    let mut total = Duration::zero();
    let mut number = String::new();
    let mut in_time = false;
    for character in rest.chars() {
        match character {
            'T' => in_time = true,
            digit if digit.is_ascii_digit() => number.push(digit),
            unit => {
                let amount: i64 = number.parse().ok()?;
                number.clear();
                total += match (unit, in_time) {
                    ('W', _) => Duration::weeks(amount),
                    ('D', _) => Duration::days(amount),
                    ('H', true) => Duration::hours(amount),
                    ('M', true) => Duration::minutes(amount),
                    ('S', true) => Duration::seconds(amount),
                    _ => return None,
                };
            }
        }
    }
    let milliseconds = total.num_milliseconds();
    Some(if negative {
        -milliseconds
    } else {
        milliseconds
    })
}

/// Serializes an event as a complete iCalendar document.
#[must_use]
pub fn ical_from_event(event: &CalendarEvent, uid: &str) -> String {
    let mut vevent = Component::new("VEVENT");
    vevent.push(Property::new("UID", uid));
    vevent.push(Property::new(
        "DTSTAMP",
        utc_stamp(Utc::now().timestamp_millis()),
    ));
    if event.all_day {
        vevent.push(
            Property::new("DTSTART", date_stamp(event.starts_at_ms)).with_param("VALUE", "DATE"),
        );
        vevent
            .push(Property::new("DTEND", date_stamp(event.ends_at_ms)).with_param("VALUE", "DATE"));
    } else {
        vevent.push(Property::new("DTSTART", utc_stamp(event.starts_at_ms)));
        vevent.push(Property::new("DTEND", utc_stamp(event.ends_at_ms)));
    }
    vevent.push(Property::new("SUMMARY", escape_text(&event.title)));
    if let Some(location) = &event.location {
        vevent.push(Property::new("LOCATION", escape_text(location)));
    }
    if let Some(description) = &event.description {
        vevent.push(Property::new("DESCRIPTION", escape_text(description)));
    }
    if let Some(recurrence) = &event.recurrence {
        vevent.push(Property::new("RRULE", rrule_value(recurrence)));
    }
    for date in &event.exception_dates {
        let compact = date.replace('-', "");
        if event.all_day {
            vevent.push(Property::new("EXDATE", compact).with_param("VALUE", "DATE"));
        } else if let Ok(day) = NaiveDate::parse_from_str(date, "%Y-%m-%d") {
            // Timed occurrences are excluded by the instant they would start.
            let start_of_day = local_midnight_ms(day);
            let start_time = event.starts_at_ms - local_midnight_ms(local_date(event.starts_at_ms));
            vevent.push(Property::new(
                "EXDATE",
                utc_stamp(start_of_day + start_time),
            ));
        }
    }
    if let Some(through) = event.reminder_dismissed_through_ms {
        vevent.push(Property::new(
            "X-MAICENTA-REMINDER-DISMISSED-THROUGH",
            through.to_string(),
        ));
    }
    if let Some(minutes) = event.reminder_minutes_before {
        let mut alarm = Component::new("VALARM");
        alarm.push(Property::new("ACTION", "DISPLAY"));
        alarm.push(Property::new("DESCRIPTION", escape_text(&event.title)));
        alarm
            .push(Property::new("TRIGGER", trigger_value(minutes)).with_param("VALUE", "DURATION"));
        vevent.children.push(alarm);
    }
    wrap_calendar(vevent)
}

fn wrap_calendar(component: Component) -> String {
    let mut calendar = Component::new("VCALENDAR");
    calendar.push(Property::new("VERSION", "2.0"));
    calendar.push(Property::new("PRODID", PRODUCT_ID));
    calendar.children.push(component);
    calendar.serialize()
}

/// Reads the first `VTODO` of an iCalendar document into a task.
///
/// # Errors
///
/// Returns an [`IcalError`] when the document has no usable `VTODO`.
pub fn task_from_ical(
    text: &str,
    id: WorkspaceItemId,
    sync: SyncMetadata,
) -> Result<TaskItem, IcalError> {
    let roots = parse_components(text)?;
    let calendar = roots
        .iter()
        .find(|component| component.name == "VCALENDAR")
        .ok_or(IcalError::MissingComponent("VCALENDAR"))?;
    let todo = calendar
        .child("VTODO")
        .ok_or(IcalError::MissingComponent("VTODO"))?;
    let starts_at_ms = todo
        .property("DTSTART")
        .and_then(|property| parse_moment(property).ok())
        .map(|moment| moment.at_ms);
    let due_at_ms = todo
        .property("DUE")
        .and_then(|property| parse_moment(property).ok())
        .map(|moment| moment.at_ms);
    let completed_at_ms = todo
        .property("COMPLETED")
        .and_then(|property| parse_moment(property).ok())
        .map(|moment| moment.at_ms);
    let status = match todo
        .property("X-MAICENTA-STATUS")
        .map(|property| property.value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("waiting") => TaskStatus::Waiting,
        Some("deferred") => TaskStatus::Deferred,
        _ => match todo
            .property("STATUS")
            .map(|property| property.value.trim().to_ascii_uppercase())
            .as_deref()
        {
            Some("COMPLETED") => TaskStatus::Completed,
            Some("IN-PROCESS") => TaskStatus::InProgress,
            Some("CANCELLED") => TaskStatus::Deferred,
            _ if completed_at_ms.is_some() => TaskStatus::Completed,
            _ => TaskStatus::NotStarted,
        },
    };
    let priority = match todo
        .property("PRIORITY")
        .and_then(|property| property.value.trim().parse::<u8>().ok())
        .unwrap_or(0)
    {
        1..=4 => TaskPriority::High,
        6..=9 => TaskPriority::Low,
        _ => TaskPriority::Normal,
    };
    let reminder_at_ms = todo
        .children
        .iter()
        .filter(|child| child.name == "VALARM")
        .filter_map(|alarm| alarm.property("TRIGGER"))
        .find_map(|trigger| {
            if trigger
                .param("VALUE")
                .is_some_and(|kind| kind.eq_ignore_ascii_case("DATE-TIME"))
            {
                parse_moment(trigger).ok().map(|moment| moment.at_ms)
            } else {
                // Relative triggers count from DUE, or DTSTART when no due
                // date exists.
                let anchor = due_at_ms.or(starts_at_ms)?;
                let minutes = trigger_minutes_before(&trigger.value)?;
                Some(anchor - i64::from(minutes) * 60_000)
            }
        });
    let mut sync = sync;
    if sync.uid.is_none() {
        sync.uid = optional_text(todo, "UID");
    }
    Ok(TaskItem {
        id,
        title: optional_text(todo, "SUMMARY").unwrap_or_else(|| "(Ohne Titel)".into()),
        description: optional_text(todo, "DESCRIPTION"),
        starts_at_ms,
        due_at_ms,
        status,
        priority,
        reminder_at_ms,
        reminder_dismissed: todo
            .property("X-MAICENTA-REMINDER-DISMISSED")
            .is_some_and(|property| property.value.trim() == "1"),
        completed_at_ms: if status == TaskStatus::Completed {
            completed_at_ms.or(Some(Utc::now().timestamp_millis()))
        } else {
            None
        },
        sync,
    })
}

/// Serializes a task as a complete iCalendar document.
#[must_use]
pub fn ical_from_task(task: &TaskItem, uid: &str) -> String {
    let mut todo = Component::new("VTODO");
    todo.push(Property::new("UID", uid));
    todo.push(Property::new(
        "DTSTAMP",
        utc_stamp(Utc::now().timestamp_millis()),
    ));
    todo.push(Property::new("SUMMARY", escape_text(&task.title)));
    if let Some(description) = &task.description {
        todo.push(Property::new("DESCRIPTION", escape_text(description)));
    }
    if let Some(start) = task.starts_at_ms {
        todo.push(Property::new("DTSTART", date_stamp(start)).with_param("VALUE", "DATE"));
    }
    if let Some(due) = task.due_at_ms {
        todo.push(Property::new("DUE", date_stamp(due)).with_param("VALUE", "DATE"));
    }
    let (status, custom) = match task.status {
        TaskStatus::NotStarted => ("NEEDS-ACTION", None),
        TaskStatus::InProgress => ("IN-PROCESS", None),
        TaskStatus::Waiting => ("NEEDS-ACTION", Some("waiting")),
        TaskStatus::Deferred => ("NEEDS-ACTION", Some("deferred")),
        TaskStatus::Completed => ("COMPLETED", None),
    };
    todo.push(Property::new("STATUS", status));
    if let Some(custom) = custom {
        todo.push(Property::new("X-MAICENTA-STATUS", custom));
    }
    todo.push(Property::new(
        "PRIORITY",
        match task.priority {
            TaskPriority::High => "1",
            TaskPriority::Normal => "5",
            TaskPriority::Low => "9",
        },
    ));
    if task.status == TaskStatus::Completed {
        todo.push(Property::new("PERCENT-COMPLETE", "100"));
        if let Some(completed) = task.completed_at_ms {
            todo.push(Property::new("COMPLETED", utc_stamp(completed)));
        }
    }
    if let Some(reminder) = task.reminder_at_ms {
        let mut alarm = Component::new("VALARM");
        alarm.push(Property::new("ACTION", "DISPLAY"));
        alarm.push(Property::new("DESCRIPTION", escape_text(&task.title)));
        alarm.push(Property::new("TRIGGER", utc_stamp(reminder)).with_param("VALUE", "DATE-TIME"));
        todo.children.push(alarm);
        if task.reminder_dismissed {
            todo.push(Property::new("X-MAICENTA-REMINDER-DISMISSED", "1"));
        }
    }
    wrap_calendar(todo)
}

/// Local calendar date (`YYYY-MM-DD`) of an instant; shared with callers
/// that need the same day boundary as this module.
#[must_use]
pub fn local_iso_date(at_ms: i64) -> String {
    iso_date(at_ms)
}

/// Year of an instant in local time, used by tests and the sync report.
#[must_use]
pub fn local_year(at_ms: i64) -> i32 {
    local_date(at_ms).year()
}

#[cfg(test)]
mod tests {
    use chrono::{Local, NaiveDate, TimeZone};
    use maicenta_domain::{
        Recurrence, RecurrenceFrequency, SyncMetadata, TaskPriority, TaskStatus, WorkspaceItemId,
    };

    use super::{
        duration_ms, event_from_ical, ical_from_event, ical_from_task, task_from_ical,
        trigger_minutes_before,
    };

    fn event_id() -> WorkspaceItemId {
        WorkspaceItemId::parse("dav.test.event").expect("id")
    }

    #[test]
    fn reads_a_timed_event_with_zone_recurrence_alarm_and_exceptions() {
        let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:test\r\nBEGIN:VEVENT\r\n\
                    UID:abc-123\r\nSUMMARY:Team-Stand-up\r\nLOCATION:Raum Nord\r\n\
                    DESCRIPTION:Kurzer Abgleich\\nzum Stand\r\n\
                    DTSTART;TZID=Europe/Berlin:20260728T093000\r\nDTEND;TZID=Europe/Berlin:20260728T100000\r\n\
                    RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,TH;COUNT=10\r\n\
                    EXDATE;TZID=Europe/Berlin:20260811T093000,20260813T093000\r\n\
                    BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\n\
                    END:VEVENT\r\nEND:VCALENDAR\r\n";
        let event = event_from_ical(text, event_id(), SyncMetadata::local()).expect("event");
        assert_eq!(event.title, "Team-Stand-up");
        assert_eq!(event.location.as_deref(), Some("Raum Nord"));
        assert_eq!(
            event.description.as_deref(),
            Some("Kurzer Abgleich\nzum Stand")
        );
        // 09:30 Berlin summer time is 07:30 UTC.
        assert_eq!(event.starts_at_ms, 1_785_223_800_000);
        assert_eq!(event.ends_at_ms - event.starts_at_ms, 1_800_000);
        assert!(!event.all_day);
        assert_eq!(event.reminder_minutes_before, Some(15));
        assert_eq!(
            event.recurrence,
            Some(Recurrence {
                frequency: RecurrenceFrequency::Weekly,
                interval: 2,
                weekdays: vec![2, 4],
                until_ms: None,
                count: Some(10),
            })
        );
        assert_eq!(event.exception_dates, vec!["2026-08-11", "2026-08-13"]);
        assert_eq!(event.sync.uid.as_deref(), Some("abc-123"));
    }

    #[test]
    fn round_trips_an_all_day_event() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:day\r\nSUMMARY:Urlaub\r\n\
                    DTSTART;VALUE=DATE:20260803\r\nDTEND;VALUE=DATE:20260806\r\n\
                    END:VEVENT\r\nEND:VCALENDAR\r\n";
        let event = event_from_ical(text, event_id(), SyncMetadata::local()).expect("event");
        assert!(event.all_day);
        let start = Local.from_local_datetime(
            &NaiveDate::from_ymd_opt(2026, 8, 3)
                .expect("date")
                .and_hms_opt(0, 0, 0)
                .expect("time"),
        );
        assert_eq!(
            event.starts_at_ms,
            start.earliest().expect("local").timestamp_millis()
        );
        assert_eq!(event.ends_at_ms - event.starts_at_ms, 3 * 86_400_000);

        let serialized = ical_from_event(&event, "day");
        assert!(serialized.contains("DTSTART;VALUE=DATE:20260803"));
        assert!(serialized.contains("DTEND;VALUE=DATE:20260806"));
        let again = event_from_ical(&serialized, event_id(), SyncMetadata::local()).expect("again");
        assert_eq!(again.starts_at_ms, event.starts_at_ms);
        assert_eq!(again.ends_at_ms, event.ends_at_ms);
    }

    #[test]
    fn serializes_recurrence_reminder_and_exceptions() {
        let mut event =
            event_from_ical(
                "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x\r\nSUMMARY:Serie\r\n\
                 DTSTART:20260728T073000Z\r\nDTEND:20260728T080000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
                event_id(),
                SyncMetadata::local(),
            )
            .expect("event");
        event.recurrence = Some(Recurrence {
            frequency: RecurrenceFrequency::Monthly,
            interval: 3,
            weekdays: Vec::new(),
            until_ms: Some(1_800_000_000_000),
            count: None,
        });
        event.reminder_minutes_before = Some(1440);
        event.exception_dates = vec!["2026-10-28".into()];
        event.reminder_dismissed_through_ms = Some(1_785_828_600_000);
        let serialized = ical_from_event(&event, "x");
        assert!(serialized.contains("RRULE:FREQ=MONTHLY;INTERVAL=3;UNTIL=20270115T080000Z"));
        assert!(serialized.contains("TRIGGER;VALUE=DURATION:-P1D"));
        assert!(serialized.contains("EXDATE:"));
        let again = event_from_ical(&serialized, event_id(), SyncMetadata::local()).expect("again");
        assert_eq!(again.recurrence, event.recurrence);
        assert_eq!(again.reminder_minutes_before, Some(1440));
        assert_eq!(again.exception_dates, vec!["2026-10-28"]);
        assert_eq!(again.reminder_dismissed_through_ms, Some(1_785_828_600_000));
    }

    #[test]
    fn maps_tasks_in_both_directions() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:todo-1\r\nSUMMARY:Rechnung\r\n\
                    DUE;VALUE=DATE:20260805\r\nSTATUS:IN-PROCESS\r\nPRIORITY:2\r\n\
                    BEGIN:VALARM\r\nTRIGGER;VALUE=DATE-TIME:20260804T070000Z\r\nEND:VALARM\r\n\
                    END:VTODO\r\nEND:VCALENDAR\r\n";
        let task = task_from_ical(
            text,
            WorkspaceItemId::parse("dav.test.task").expect("id"),
            SyncMetadata::local(),
        )
        .expect("task");
        assert_eq!(task.title, "Rechnung");
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.priority, TaskPriority::High);
        assert_eq!(task.reminder_at_ms, Some(1_785_826_800_000));
        assert!(task.due_at_ms.is_some());

        let mut completed = task.clone();
        completed.status = TaskStatus::Waiting;
        let serialized = ical_from_task(&completed, "todo-1");
        assert!(serialized.contains("STATUS:NEEDS-ACTION"));
        assert!(serialized.contains("X-MAICENTA-STATUS:waiting"));
        let again = task_from_ical(
            &serialized,
            WorkspaceItemId::parse("dav.test.task").expect("id"),
            SyncMetadata::local(),
        )
        .expect("again");
        assert_eq!(again.status, TaskStatus::Waiting);
        assert_eq!(again.reminder_at_ms, task.reminder_at_ms);
        assert_eq!(again.due_at_ms, task.due_at_ms);
    }

    #[test]
    fn parses_durations_and_triggers() {
        assert_eq!(duration_ms("PT1H30M"), Some(5_400_000));
        assert_eq!(duration_ms("P2D"), Some(172_800_000));
        assert_eq!(duration_ms("-PT5M"), Some(-300_000));
        assert_eq!(trigger_minutes_before("-PT15M"), Some(15));
        assert_eq!(trigger_minutes_before("-P1D"), Some(1440));
        assert_eq!(trigger_minutes_before("-PT1H"), Some(60));
        assert_eq!(trigger_minutes_before("PT0S"), Some(0));
        assert_eq!(trigger_minutes_before("PT10M"), Some(0));
        assert_eq!(trigger_minutes_before("nonsense"), None);
    }
}
