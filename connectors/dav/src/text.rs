//! Content lines shared by iCalendar (RFC 5545) and vCard (RFC 6350):
//! unfolding, `NAME;PARAM=VALUE:value` splitting, text escaping, and the
//! component tree both formats are built from.

use std::fmt::Write as _;

/// One property such as `DTSTART;TZID=Europe/Berlin:20260728T093000`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Property {
    /// Upper-case property name.
    pub name: String,
    /// Parameters in order of appearance; names are upper-case, values are
    /// unquoted and comma-separated lists are kept as one string.
    pub params: Vec<(String, String)>,
    /// Raw value as written on the line, still escaped.
    pub value: String,
}

impl Property {
    #[must_use]
    pub fn new(name: &str, value: impl Into<String>) -> Self {
        Self {
            name: name.to_owned(),
            params: Vec::new(),
            value: value.into(),
        }
    }

    #[must_use]
    pub fn with_param(mut self, name: &str, value: &str) -> Self {
        self.params.push((name.to_owned(), value.to_owned()));
        self
    }

    /// First value of a parameter, compared case-insensitively.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Whether a parameter contains the given token, e.g. `TYPE=WORK,VOICE`.
    #[must_use]
    pub fn param_contains(&self, name: &str, token: &str) -> bool {
        self.param(name).is_some_and(|value| {
            value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case(token))
        })
    }

    /// The value with text escapes resolved.
    #[must_use]
    pub fn text(&self) -> String {
        unescape_text(&self.value)
    }
}

/// A `BEGIN:`…`END:` block with its properties and nested components.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Component {
    pub name: String,
    pub properties: Vec<Property>,
    pub children: Vec<Component>,
}

impl Component {
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            properties: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn push(&mut self, property: Property) {
        self.properties.push(property);
    }

    /// First property with the given name.
    #[must_use]
    pub fn property(&self, name: &str) -> Option<&Property> {
        self.properties
            .iter()
            .find(|property| property.name.eq_ignore_ascii_case(name))
    }

    /// All properties with the given name, in order.
    pub fn properties_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Property> {
        self.properties
            .iter()
            .filter(move |property| property.name.eq_ignore_ascii_case(name))
    }

    /// Unescaped text of the first property with the given name.
    #[must_use]
    pub fn text(&self, name: &str) -> Option<String> {
        self.property(name).map(Property::text)
    }

    /// First child component with the given name.
    #[must_use]
    pub fn child(&self, name: &str) -> Option<&Component> {
        self.children
            .iter()
            .find(|child| child.name.eq_ignore_ascii_case(name))
    }

    /// Serializes the component with CRLF line ends and folded lines.
    #[must_use]
    pub fn serialize(&self) -> String {
        let mut output = String::new();
        self.write_into(&mut output);
        output
    }

    fn write_into(&self, output: &mut String) {
        fold_line(&format!("BEGIN:{}", self.name), output);
        for property in &self.properties {
            let mut line = property.name.clone();
            for (name, value) in &property.params {
                let needs_quotes = value.contains([':', ';', ',']);
                if needs_quotes {
                    let _ = write!(line, ";{name}=\"{value}\"");
                } else {
                    let _ = write!(line, ";{name}={value}");
                }
            }
            line.push(':');
            line.push_str(&property.value);
            fold_line(&line, output);
        }
        for child in &self.children {
            child.write_into(output);
        }
        fold_line(&format!("END:{}", self.name), output);
    }
}

/// Errors raised while reading iCalendar or vCard text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseError {
    /// A line lacked the `NAME:value` shape.
    MalformedLine(String),
    /// `END:` did not match the open component, or a component was never
    /// closed.
    UnbalancedComponent(String),
    /// No component was found at all.
    Empty,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedLine(line) => write!(formatter, "malformed content line: {line}"),
            Self::UnbalancedComponent(name) => {
                write!(formatter, "unbalanced component: {name}")
            }
            Self::Empty => formatter.write_str("no component found"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses a complete document and returns its top-level components.
///
/// # Errors
///
/// Returns a [`ParseError`] for malformed lines or unbalanced components.
pub fn parse_components(text: &str) -> Result<Vec<Component>, ParseError> {
    let mut stack: Vec<Component> = Vec::new();
    let mut roots = Vec::new();
    for line in unfold(text) {
        if line.trim().is_empty() {
            continue;
        }
        let property = parse_line(&line)?;
        if property.name == "BEGIN" {
            stack.push(Component::new(&property.value.trim().to_ascii_uppercase()));
        } else if property.name == "END" {
            let Some(component) = stack.pop() else {
                return Err(ParseError::UnbalancedComponent(property.value));
            };
            if !component.name.eq_ignore_ascii_case(property.value.trim()) {
                return Err(ParseError::UnbalancedComponent(property.value));
            }
            match stack.last_mut() {
                Some(parent) => parent.children.push(component),
                None => roots.push(component),
            }
        } else if let Some(current) = stack.last_mut() {
            current.push(property);
        }
        // Properties outside any component are ignored, as servers may
        // prepend stray lines.
    }
    if let Some(open) = stack.pop() {
        return Err(ParseError::UnbalancedComponent(open.name));
    }
    if roots.is_empty() {
        return Err(ParseError::Empty);
    }
    Ok(roots)
}

/// Joins folded continuation lines (a line starting with a space or tab
/// continues the previous one).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(continuation) = line.strip_prefix([' ', '\t']) {
            if let Some(previous) = lines.last_mut() {
                previous.push_str(continuation);
                continue;
            }
        }
        lines.push(line.to_owned());
    }
    lines
}

/// Splits one unfolded line into name, parameters, and value.
fn parse_line(line: &str) -> Result<Property, ParseError> {
    let mut name = String::new();
    let mut params = Vec::new();
    // Name runs up to ';' or ':'.
    let mut value_start = None;
    let mut current_param: Option<(String, String)> = None;
    let mut in_quotes = false;
    for (index, character) in line.char_indices() {
        match (&mut current_param, character) {
            (None, ':') if !in_quotes => {
                value_start = Some(index + 1);
                break;
            }
            (None, ';') => {
                current_param = Some((String::new(), String::new()));
            }
            (None, other) => name.push(other),
            (Some((param_name, param_value)), other) => {
                if in_quotes {
                    if other == '"' {
                        in_quotes = false;
                    } else {
                        param_value.push(other);
                    }
                } else if other == '"' {
                    in_quotes = true;
                } else if other == '='
                    && param_value.is_empty()
                    && !param_name.is_empty()
                    && !param_name.contains('=')
                {
                    // Switch from name to value: mark by storing a sentinel.
                    param_name.push('=');
                } else if other == ';' {
                    let (param_name, param_value) = current_param.take().expect("param");
                    params.push(finish_param(&param_name, param_value));
                    current_param = Some((String::new(), String::new()));
                } else if other == ':' {
                    let (param_name, param_value) = current_param.take().expect("param");
                    params.push(finish_param(&param_name, param_value));
                    value_start = Some(index + 1);
                    break;
                } else if param_name.ends_with('=') {
                    param_value.push(other);
                } else {
                    param_name.push(other);
                }
            }
        }
    }
    let Some(value_start) = value_start else {
        return Err(ParseError::MalformedLine(line.to_owned()));
    };
    let name = name.trim().to_ascii_uppercase();
    if name.is_empty() {
        return Err(ParseError::MalformedLine(line.to_owned()));
    }
    Ok(Property {
        name,
        params,
        value: line[value_start..].to_owned(),
    })
}

fn finish_param(name: &str, value: String) -> (String, String) {
    let name = name.trim_end_matches('=').trim().to_ascii_uppercase();
    (name, value)
}

/// Resolves `\n`, `\N`, `\,`, `\;`, and `\\`.
#[must_use]
pub fn unescape_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('n' | 'N') => output.push('\n'),
                Some(other) => output.push(other),
                None => output.push('\\'),
            }
        } else {
            output.push(character);
        }
    }
    output
}

/// Escapes backslashes, semicolons, commas, and line breaks for a text
/// value.
#[must_use]
pub fn escape_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            ';' => output.push_str("\\;"),
            ',' => output.push_str("\\,"),
            '\n' => output.push_str("\\n"),
            '\r' => {}
            other => output.push(other),
        }
    }
    output
}

/// Writes a line folded at 75 octets with CRLF line ends, never splitting
/// a multi-byte character.
fn fold_line(line: &str, output: &mut String) {
    const LIMIT: usize = 75;
    let mut width = 0;
    let mut first = true;
    for character in line.chars() {
        let size = character.len_utf8();
        let limit = if first { LIMIT } else { LIMIT - 1 };
        if width + size > limit {
            output.push_str("\r\n ");
            width = 0;
            first = false;
        }
        output.push(character);
        width += size;
    }
    output.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::{Component, Property, escape_text, parse_components, unescape_text};

    #[test]
    fn parses_parameters_quotes_and_folded_lines() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART;TZID=Europe/Berlin:20260728T093000\r\n\
                    ATTENDEE;CN=\"Weber, Jonas\";ROLE=REQ-PARTICIPANT:mailto:jonas@example.org\r\n\
                    SUMMARY:Sehr lange Beschreibung die ueber mehrere Zeilen gefaltet\r\n  wurde\\, mit Komma\r\n\
                    END:VEVENT\r\nEND:VCALENDAR\r\n";
        let roots = parse_components(text).expect("parse");
        let event = roots[0].child("VEVENT").expect("event");
        let start = event.property("DTSTART").expect("dtstart");
        assert_eq!(start.param("tzid"), Some("Europe/Berlin"));
        assert_eq!(start.value, "20260728T093000");
        let attendee = event.property("ATTENDEE").expect("attendee");
        assert_eq!(attendee.param("CN"), Some("Weber, Jonas"));
        assert_eq!(attendee.value, "mailto:jonas@example.org");
        assert_eq!(
            event.text("SUMMARY").as_deref(),
            Some("Sehr lange Beschreibung die ueber mehrere Zeilen gefaltet wurde, mit Komma")
        );
    }

    #[test]
    fn serializes_with_folding_and_escaping() {
        let mut component = Component::new("VEVENT");
        component.push(Property::new(
            "DESCRIPTION",
            escape_text("Zeile 1\nZeile 2; mit, Satzzeichen \\ und einem sehr langen Text der gefaltet werden muss"),
        ));
        component.push(Property::new("X-NOTE", "a").with_param("CN", "Weber, Jonas"));
        let serialized = component.serialize();
        for line in serialized.split("\r\n") {
            assert!(line.len() <= 75, "line too long: {line}");
        }
        assert!(serialized.contains("X-NOTE;CN=\"Weber, Jonas\":a"));
        let parsed = parse_components(&serialized).expect("round trip");
        assert_eq!(
            parsed[0].text("DESCRIPTION").as_deref(),
            Some(
                "Zeile 1\nZeile 2; mit, Satzzeichen \\ und einem sehr langen Text der gefaltet werden muss"
            )
        );
        assert_eq!(unescape_text("a\\Nb"), "a\nb");
    }

    #[test]
    fn rejects_unbalanced_components() {
        assert!(parse_components("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nEND:VCALENDAR\r\n").is_err());
        assert!(parse_components("").is_err());
    }
}
