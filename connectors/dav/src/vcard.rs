//! Mapping between MAICENTA contacts and vCard 3.0/4.0 (RFC 2426, RFC 6350).

use maicenta_domain::{
    Contact, ContactEmail, ContactLabel, ContactPhone, MailAddress, PostalAddress, SyncMetadata,
    WorkspaceItemId,
};

use crate::text::{Component, ParseError, Property, escape_text, parse_components};

/// Errors raised while converting vCard data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VcardError {
    Parse(ParseError),
    /// The document contains no `VCARD`.
    MissingCard,
}

impl From<ParseError> for VcardError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl std::fmt::Display for VcardError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "vCard parse error: {error}"),
            Self::MissingCard => formatter.write_str("no VCARD component found"),
        }
    }
}

impl std::error::Error for VcardError {}

fn label_from_types(property: &Property, phone: bool) -> ContactLabel {
    let types: Vec<String> = property
        .params
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("TYPE"))
        .flat_map(|(_, value)| value.split(','))
        .map(|token| token.trim().to_ascii_lowercase())
        .collect();
    if phone {
        if types
            .iter()
            .any(|token| token == "cell" || token == "mobile")
        {
            return ContactLabel::Mobile;
        }
        if types.iter().any(|token| token == "fax") {
            return ContactLabel::Fax;
        }
    }
    if types.iter().any(|token| token == "work") {
        ContactLabel::Work
    } else if types.iter().any(|token| token == "home") {
        ContactLabel::Home
    } else {
        ContactLabel::Other
    }
}

fn type_param(label: ContactLabel, phone: bool) -> Option<&'static str> {
    match (label, phone) {
        (ContactLabel::Work, _) => Some("WORK"),
        (ContactLabel::Home, _) => Some("HOME"),
        (ContactLabel::Mobile, true) => Some("CELL"),
        (ContactLabel::Fax, true) => Some("FAX"),
        _ => None,
    }
}

/// Splits a structured value such as `N` or `ADR` on unescaped semicolons.
fn structured_parts(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        match character {
            '\\' => match chars.next() {
                Some('n' | 'N') => current.push('\n'),
                Some(other) => current.push(other),
                None => current.push('\\'),
            },
            ';' => parts.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    parts.push(current);
    parts
}

fn optional_text(card: &Component, name: &str) -> Option<String> {
    card.text(name)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Reads an ISO (`1980-05-17`), compact (`19800517`), or vCard 4 partial
/// (`--0517`) birthday into `YYYY-MM-DD`; years-only or unreadable values
/// are dropped.
fn birthday_from_value(value: &str) -> Option<String> {
    let value = value.trim();
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    if value.starts_with("--") && digits.len() == 4 {
        // Birthday without a year: keep the vCard 4 placeholder year.
        return Some(format!("1604-{}-{}", &digits[0..2], &digits[2..4]));
    }
    if digits.len() >= 8 {
        let year = &digits[0..4];
        let month = &digits[4..6];
        let day = &digits[6..8];
        if (1..=12).contains(&month.parse::<u32>().ok()?)
            && (1..=31).contains(&day.parse::<u32>().ok()?)
        {
            return Some(format!("{year}-{month}-{day}"));
        }
    }
    None
}

/// Reads the first `VCARD` of a document into a contact.
///
/// # Errors
///
/// Returns a [`VcardError`] when the document holds no card.
pub fn contact_from_vcard(
    text: &str,
    id: WorkspaceItemId,
    sync: SyncMetadata,
) -> Result<Contact, VcardError> {
    let roots = parse_components(text)?;
    let card = roots
        .iter()
        .find(|component| component.name == "VCARD")
        .ok_or(VcardError::MissingCard)?;
    let name = optional_text(card, "FN")
        .or_else(|| {
            card.property("N").map(|property| {
                let parts = structured_parts(&property.value);
                let given = parts.get(1).map(String::as_str).unwrap_or_default();
                let family = parts.first().map(String::as_str).unwrap_or_default();
                format!("{given} {family}").trim().to_owned()
            })
        })
        .filter(|value| !value.is_empty())
        .or_else(|| optional_text(card, "ORG"))
        .unwrap_or_else(|| "(Ohne Namen)".into());
    let emails = card
        .properties_named("EMAIL")
        .filter_map(|property| {
            let address = property.text();
            let address = address.trim().trim_start_matches("mailto:");
            MailAddress::new(address, None)
                .ok()
                .map(|address| ContactEmail {
                    label: label_from_types(property, false),
                    address,
                })
        })
        .collect();
    let phones = card
        .properties_named("TEL")
        .filter_map(|property| {
            let number = property.text();
            let number = number.trim().trim_start_matches("tel:").trim();
            if number.is_empty() {
                return None;
            }
            Some(ContactPhone {
                label: label_from_types(property, true),
                number: number.to_owned(),
            })
        })
        .collect();
    let addresses = card
        .properties_named("ADR")
        .filter_map(|property| {
            let parts = structured_parts(&property.value);
            let part = |index: usize| {
                parts
                    .get(index)
                    .map_or("", String::as_str)
                    .trim()
                    .to_owned()
            };
            let street = [part(0), part(1), part(2)]
                .into_iter()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            let address = PostalAddress {
                label: label_from_types(property, false),
                street,
                postal_code: part(5),
                city: part(3),
                country: part(6),
            };
            let empty = address.street.is_empty()
                && address.postal_code.is_empty()
                && address.city.is_empty()
                && address.country.is_empty();
            (!empty).then_some(address)
        })
        .collect();
    let mut sync = sync;
    if sync.uid.is_none() {
        sync.uid = optional_text(card, "UID");
    }
    Ok(Contact {
        id,
        name,
        company: optional_text(card, "ORG").map(|org| {
            structured_parts(&org)
                .first()
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_owned()
        }),
        job_title: optional_text(card, "TITLE"),
        emails,
        phones,
        addresses,
        birthday: card
            .property("BDAY")
            .and_then(|property| birthday_from_value(&property.value)),
        website: optional_text(card, "URL"),
        notes: optional_text(card, "NOTE"),
        sync,
    })
}

/// Serializes a contact as a vCard 3.0 document.
#[must_use]
pub fn vcard_from_contact(contact: &Contact, uid: &str) -> String {
    let mut card = Component::new("VCARD");
    card.push(Property::new("VERSION", "3.0"));
    card.push(Property::new("PRODID", crate::ical::PRODUCT_ID));
    card.push(Property::new("UID", uid));
    card.push(Property::new("FN", escape_text(&contact.name)));
    let mut words = contact.name.split_whitespace();
    let first = words.next().unwrap_or_default();
    let rest = words.collect::<Vec<_>>().join(" ");
    let (given, family) = if rest.is_empty() {
        ("", first)
    } else {
        (first, rest.as_str())
    };
    card.push(Property::new(
        "N",
        format!("{};{};;;", escape_text(family), escape_text(given)),
    ));
    if let Some(company) = &contact.company {
        card.push(Property::new("ORG", escape_text(company)));
    }
    if let Some(title) = &contact.job_title {
        card.push(Property::new("TITLE", escape_text(title)));
    }
    for email in &contact.emails {
        let mut property = Property::new("EMAIL", email.address.address());
        if let Some(kind) = type_param(email.label, false) {
            property = property.with_param("TYPE", kind);
        }
        card.push(property);
    }
    for phone in &contact.phones {
        let mut property = Property::new("TEL", escape_text(&phone.number));
        if let Some(kind) = type_param(phone.label, true) {
            property = property.with_param("TYPE", kind);
        }
        card.push(property);
    }
    for address in &contact.addresses {
        let mut property = Property::new(
            "ADR",
            format!(
                ";;{};{};;{};{}",
                escape_text(&address.street),
                escape_text(&address.city),
                escape_text(&address.postal_code),
                escape_text(&address.country)
            ),
        );
        if let Some(kind) = type_param(address.label, false) {
            property = property.with_param("TYPE", kind);
        }
        card.push(property);
    }
    if let Some(birthday) = &contact.birthday {
        card.push(Property::new("BDAY", birthday.replace('-', "")));
    }
    if let Some(website) = &contact.website {
        card.push(Property::new("URL", escape_text(website)));
    }
    if let Some(notes) = &contact.notes {
        card.push(Property::new("NOTE", escape_text(notes)));
    }
    card.serialize()
}

#[cfg(test)]
mod tests {
    use maicenta_domain::{ContactLabel, SyncMetadata, WorkspaceItemId};

    use super::{contact_from_vcard, vcard_from_contact};

    fn id() -> WorkspaceItemId {
        WorkspaceItemId::parse("dav.test.contact").expect("id")
    }

    #[test]
    fn reads_a_vcard_with_every_supported_detail() {
        let text = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:c1\r\nFN:Anna Schneider\r\nN:Schneider;Anna;;;\r\n\
                    ORG:Schneider Consulting;Beratung\r\nTITLE:Projektleitung\r\n\
                    EMAIL;TYPE=INTERNET,WORK:anna@example.org\r\nEMAIL;TYPE=HOME:anna@home.example\r\n\
                    TEL;TYPE=CELL,VOICE:+49 170 1234567\r\nTEL;TYPE=WORK,FAX:+49 30 1\r\n\
                    ADR;TYPE=WORK:;;Musterstraße 1;Berlin;;10115;Deutschland\r\n\
                    BDAY:1980-05-17\r\nURL:https://example.org\r\nNOTE:Kennt das Projekt\\, seit 2024\r\n\
                    END:VCARD\r\n";
        let contact = contact_from_vcard(text, id(), SyncMetadata::local()).expect("contact");
        assert_eq!(contact.name, "Anna Schneider");
        assert_eq!(contact.company.as_deref(), Some("Schneider Consulting"));
        assert_eq!(contact.job_title.as_deref(), Some("Projektleitung"));
        assert_eq!(contact.emails.len(), 2);
        assert_eq!(contact.emails[0].label, ContactLabel::Work);
        assert_eq!(contact.emails[1].label, ContactLabel::Home);
        assert_eq!(contact.phones[0].label, ContactLabel::Mobile);
        assert_eq!(contact.phones[1].label, ContactLabel::Fax);
        assert_eq!(contact.addresses[0].street, "Musterstraße 1");
        assert_eq!(contact.addresses[0].postal_code, "10115");
        assert_eq!(contact.addresses[0].city, "Berlin");
        assert_eq!(contact.addresses[0].country, "Deutschland");
        assert_eq!(contact.birthday.as_deref(), Some("1980-05-17"));
        assert_eq!(contact.website.as_deref(), Some("https://example.org"));
        assert_eq!(
            contact.notes.as_deref(),
            Some("Kennt das Projekt, seit 2024")
        );
        assert_eq!(contact.sync.uid.as_deref(), Some("c1"));
    }

    #[test]
    fn round_trips_through_vcard_3() {
        let text = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jonas Weber\r\nEMAIL:jonas@example.org\r\n\
                    TEL;TYPE=cell:tel:+4917012345\r\nBDAY:19901231\r\nEND:VCARD\r\n";
        let contact = contact_from_vcard(text, id(), SyncMetadata::local()).expect("contact");
        assert_eq!(contact.phones[0].number, "+4917012345");
        assert_eq!(contact.birthday.as_deref(), Some("1990-12-31"));
        let serialized = vcard_from_contact(&contact, "j1");
        assert!(serialized.contains("N:Weber;Jonas;;;"));
        assert!(serialized.contains("TEL;TYPE=CELL:+4917012345"));
        assert!(serialized.contains("BDAY:19901231"));
        let again = contact_from_vcard(&serialized, id(), SyncMetadata::local()).expect("again");
        assert_eq!(again.name, "Jonas Weber");
        assert_eq!(again.emails[0].address.address(), "jonas@example.org");
        assert_eq!(again.phones[0].label, ContactLabel::Mobile);
        assert_eq!(again.sync.uid.as_deref(), Some("j1"));
    }

    #[test]
    fn falls_back_to_structured_name_and_organisation() {
        let text = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Weber;Jonas;;;\r\nEND:VCARD\r\n";
        let contact = contact_from_vcard(text, id(), SyncMetadata::local()).expect("contact");
        assert_eq!(contact.name, "Jonas Weber");
        let text = "BEGIN:VCARD\r\nVERSION:3.0\r\nORG:Beispiel GmbH\r\nEND:VCARD\r\n";
        let contact = contact_from_vcard(text, id(), SyncMetadata::local()).expect("contact");
        assert_eq!(contact.name, "Beispiel GmbH");
    }
}
