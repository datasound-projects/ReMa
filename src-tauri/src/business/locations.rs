//! Location semantics (B7). Within one filter, places mean OR; different
//! filters combine with AND. Overlapping country, region and city
//! selections are a union ("Austria" already includes Vienna). "DACH" is
//! Germany, Austria and Switzerland, shown as such. A radius needs
//! reliable coordinates, which ReMa does not have: it is stated as
//! unavailable, never approximated. The device's location is never used.

use std::sync::OnceLock;

use regex::Regex;

use super::{model::Locations, text::has_phrase};
use crate::analytics::normalize;

/// (canonical region, country, names people use).
const REGIONS: &[(&str, &str, &[&str])] = &[
    (
        "Lower Austria",
        "Austria",
        &["lower austria", "niederösterreich", "niederoesterreich"],
    ),
    (
        "Upper Austria",
        "Austria",
        &["upper austria", "oberösterreich", "oberoesterreich"],
    ),
    ("Styria", "Austria", &["styria", "steiermark"]),
    ("Tyrol", "Austria", &["tyrol", "tirol"]),
    (
        "Carinthia",
        "Austria",
        &["carinthia", "kärnten", "kaernten"],
    ),
    ("Vorarlberg", "Austria", &["vorarlberg"]),
    ("Burgenland", "Austria", &["burgenland"]),
    (
        "Salzburg (state)",
        "Austria",
        &["salzburg state", "land salzburg", "bundesland salzburg"],
    ),
    (
        "Vienna (state)",
        "Austria",
        &["vienna state", "bundesland wien"],
    ),
    (
        "Berlin (state)",
        "Germany",
        &["berlin state", "land berlin"],
    ),
    ("Hamburg (state)", "Germany", &["hamburg state"]),
    ("Bremen (state)", "Germany", &["bremen state"]),
    ("Bavaria", "Germany", &["bavaria", "bayern"]),
    (
        "Baden-Württemberg",
        "Germany",
        &[
            "baden-württemberg",
            "baden-wuerttemberg",
            "baden württemberg",
        ],
    ),
    ("Hesse", "Germany", &["hesse", "hessen"]),
    (
        "Lower Saxony",
        "Germany",
        &["lower saxony", "niedersachsen"],
    ),
    (
        "North Rhine-Westphalia",
        "Germany",
        &["north rhine-westphalia", "nordrhein-westfalen", "nrw"],
    ),
    (
        "Rhineland-Palatinate",
        "Germany",
        &["rhineland-palatinate", "rheinland-pfalz"],
    ),
    ("Saxony", "Germany", &["saxony", "sachsen"]),
    (
        "Saxony-Anhalt",
        "Germany",
        &["saxony-anhalt", "sachsen-anhalt"],
    ),
    (
        "Thuringia",
        "Germany",
        &["thuringia", "thüringen", "thueringen"],
    ),
    ("Schleswig-Holstein", "Germany", &["schleswig-holstein"]),
    (
        "Mecklenburg-Vorpommern",
        "Germany",
        &["mecklenburg-vorpommern"],
    ),
    ("Brandenburg", "Germany", &["brandenburg"]),
    ("Saarland", "Germany", &["saarland"]),
    (
        "Canton of Zurich",
        "Switzerland",
        &["canton of zurich", "kanton zürich", "kanton zurich"],
    ),
    (
        "Canton of Bern",
        "Switzerland",
        &["canton of bern", "kanton bern"],
    ),
    ("Vaud", "Switzerland", &["vaud", "waadt"]),
    ("Ticino", "Switzerland", &["ticino", "tessin"]),
    ("Aargau", "Switzerland", &["aargau"]),
    ("Basel-Stadt", "Switzerland", &["basel-stadt"]),
    (
        "Canton of Geneva",
        "Switzerland",
        &["canton of geneva", "kanton genf"],
    ),
    (
        "Canton of Lucerne",
        "Switzerland",
        &["canton of lucerne", "kanton luzern"],
    ),
    (
        "Canton of Zug",
        "Switzerland",
        &["canton of zug", "kanton zug"],
    ),
];

/// Cities ReMa places in a region (major ones only; others stay unknown).
const CITY_REGION: &[(&str, &str)] = &[
    ("Munich", "Bavaria"),
    ("Nuremberg", "Bavaria"),
    ("Augsburg", "Bavaria"),
    ("Regensburg", "Bavaria"),
    ("Stuttgart", "Baden-Württemberg"),
    ("Karlsruhe", "Baden-Württemberg"),
    ("Mannheim", "Baden-Württemberg"),
    ("Freiburg", "Baden-Württemberg"),
    ("Heidelberg", "Baden-Württemberg"),
    ("Frankfurt", "Hesse"),
    ("Wiesbaden", "Hesse"),
    ("Darmstadt", "Hesse"),
    ("Cologne", "North Rhine-Westphalia"),
    ("Düsseldorf", "North Rhine-Westphalia"),
    ("Dortmund", "North Rhine-Westphalia"),
    ("Essen", "North Rhine-Westphalia"),
    ("Bonn", "North Rhine-Westphalia"),
    ("Aachen", "North Rhine-Westphalia"),
    ("Münster", "North Rhine-Westphalia"),
    ("Hanover", "Lower Saxony"),
    ("Dresden", "Saxony"),
    ("Leipzig", "Saxony"),
    ("Graz", "Styria"),
    ("Linz", "Upper Austria"),
    ("Innsbruck", "Tyrol"),
    ("Klagenfurt", "Carinthia"),
    ("St. Pölten", "Lower Austria"),
    ("Bregenz", "Vorarlberg"),
    ("Dornbirn", "Vorarlberg"),
    ("Eisenstadt", "Burgenland"),
    ("Salzburg", "Salzburg (state)"),
    ("Zurich", "Canton of Zurich"),
    ("Winterthur", "Canton of Zurich"),
    ("Bern", "Canton of Bern"),
    ("Lausanne", "Vaud"),
    ("Lugano", "Ticino"),
    ("Vienna", "Vienna (state)"),
    ("Berlin", "Berlin (state)"),
    ("Hamburg", "Hamburg (state)"),
    ("Bremen", "Bremen (state)"),
    ("Basel", "Basel-Stadt"),
    ("Geneva", "Canton of Geneva"),
    ("Lucerne", "Canton of Lucerne"),
    ("Zug", "Canton of Zug"),
];

/// Names that are both a city and a region; read as the city.
const AMBIGUOUS: &[(&str, &str)] = &[
    (
        "Salzburg",
        "Salzburg is read as the city; say \"Salzburg state\" for the whole state.",
    ),
    (
        "Vienna",
        "Vienna is both a city and a state; they cover the same area.",
    ),
    (
        "Berlin",
        "Berlin is both a city and a state; they cover the same area.",
    ),
    (
        "Hamburg",
        "Hamburg is both a city and a state; they cover the same area.",
    ),
    ("Bremen", "Bremen is read as the city."),
];

/// Adjectives that name a country when they describe organizations
/// ("Austrian manufacturers"), not a language ("fluent German").
const DEMONYMS: &[(&str, &str)] = &[
    ("austrian", "Austria"),
    ("german", "Germany"),
    ("swiss", "Switzerland"),
    ("dutch", "Netherlands"),
    ("belgian", "Belgium"),
    ("french", "France"),
    ("italian", "Italy"),
    ("spanish", "Spain"),
    ("british", "United Kingdom"),
    ("irish", "Ireland"),
    ("polish", "Poland"),
    ("czech", "Czechia"),
    ("danish", "Denmark"),
    ("swedish", "Sweden"),
    ("norwegian", "Norway"),
    ("finnish", "Finland"),
    ("american", "United States"),
    ("canadian", "Canada"),
];

const ORGANIZATIONS: &[&str] = &[
    "companies",
    "company",
    "firms",
    "firm",
    "businesses",
    "business",
    "manufacturers",
    "manufacturer",
    "startups",
    "startup",
    "smes",
    "sme",
    "organizations",
    "organisations",
    "clients",
    "customers",
    "prospects",
    "agencies",
    "banks",
    "hospitals",
    "retailers",
    "enterprises",
    "mittelstand",
    "market",
    "employers",
    "insurers",
    "brands",
    "producers",
    "suppliers",
    "manufacturing",
    "industry",
];

/// Countries named by adjective before an organization noun (within four
/// words: "Austrian and German manufacturing companies").
fn demonym_countries(text: &str) -> Vec<&'static str> {
    let words: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    let mut out = Vec::new();
    for (i, word) in words.iter().enumerate() {
        let Some((_, country)) = DEMONYMS.iter().find(|(d, _)| d == word) else {
            continue;
        };
        let window = &words[i + 1..words.len().min(i + 6)];
        if window.iter().any(|w| ORGANIZATIONS.contains(&w.as_str())) && !out.contains(country) {
            out.push(*country);
        }
    }
    out
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

fn push(list: &mut Vec<String>, value: &str) {
    if !list.iter().any(|v| v == value) {
        list.push(value.to_string());
    }
}

/// The places a request names (and notes on how they were read).
pub fn parse(text: &str) -> (Locations, Vec<String>) {
    static DACH: OnceLock<Regex> = OnceLock::new();
    static RADIUS: OnceLock<Regex> = OnceLock::new();
    let mut out = Locations::default();
    let mut notes = Vec::new();
    if re(&DACH, r"(?i)\b(dach|d-a-ch)\b").is_match(text) {
        for country in ["Germany", "Austria", "Switzerland"] {
            push(&mut out.countries, country);
        }
    }
    for country in demonym_countries(text) {
        push(&mut out.countries, country);
    }
    let lower = text.to_lowercase();
    for (region, _, names) in REGIONS {
        if names.iter().any(|n| has_phrase(&lower, n)) {
            push(&mut out.regions, region);
        }
    }
    let found = normalize::place(text);
    for city in &found.cities {
        // "Salzburg state" names the region, not the city.
        if *city == "Salzburg" && out.regions.iter().any(|r| r == "Salzburg (state)") {
            continue;
        }
        push(&mut out.cities, city);
        if let Some((_, note)) = AMBIGUOUS.iter().find(|(c, _)| c == city) {
            notes.push((*note).to_string());
        }
    }
    // A country named on its own (not only as a city's country).
    for country in &found.countries {
        let named_alone = found.cities.iter().all(|city| {
            normalize::place(city).country() != Some(*country)
                || has_phrase(&lower, &country.to_lowercase())
        });
        if named_alone || found.cities.is_empty() {
            push(&mut out.countries, country);
        }
    }
    if let Some(caps) = re(
        &RADIUS,
        r"(?i)\b(?:within|in a radius of|umkreis(?: von)?)\s+(\d{1,4})\s?km\b",
    )
    .captures(text)
    {
        out.radius_km = caps[1].parse().ok();
    }
    if out.radius_km.is_some() {
        notes.push(
            "A radius needs reliable coordinates, which ReMa does not have; the search uses the \
             selected places themselves."
                .into(),
        );
    }
    (union(out), notes)
}

fn region_country(region: &str) -> Option<&'static str> {
    REGIONS
        .iter()
        .find(|(r, _, _)| *r == region)
        .map(|(_, c, _)| *c)
}

fn city_country(city: &str) -> Option<&'static str> {
    normalize::place(city).country()
}

fn city_region(city: &str) -> Option<&'static str> {
    CITY_REGION
        .iter()
        .find(|(c, _)| *c == city)
        .map(|(_, r)| *r)
}

/// A union without duplicate conditions: places inside a selected
/// country or region are covered by it.
pub fn union(mut l: Locations) -> Locations {
    let countries = l.countries.clone();
    l.regions
        .retain(|r| region_country(r).is_none_or(|c| !countries.iter().any(|x| x == c)));
    let regions = l.regions.clone();
    l.cities.retain(|city| {
        city_country(city).is_none_or(|c| !countries.iter().any(|x| x == c))
            && city_region(city).is_none_or(|r| !regions.iter().any(|x| x == r))
    });
    l
}

/// The criteria as ReMa applies them ("Germany, Austria or Switzerland").
pub fn label(l: &Locations) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.extend(l.countries.iter().cloned());
    parts.extend(l.regions.iter().cloned());
    parts.extend(l.cities.iter().cloned());
    let text = match parts.len() {
        0 => return "Anywhere (no place selected)".into(),
        1 => parts[0].clone(),
        n => format!("{} or {}", parts[..n - 1].join(", "), parts[n - 1]),
    };
    let dach = ["Germany", "Austria", "Switzerland"]
        .iter()
        .all(|c| l.countries.iter().any(|x| x == c));
    if dach {
        format!("{text} (DACH)")
    } else {
        text
    }
}

pub fn is_empty(l: &Locations) -> bool {
    l.countries.is_empty() && l.regions.is_empty() && l.cities.is_empty()
}

/// Whether a stated location is one of the selected places: `Some(true)`
/// for a match, `Some(false)` when it is known to be elsewhere, `None`
/// when the text does not say.
pub fn matches(l: &Locations, location: &str) -> Option<bool> {
    if is_empty(l) {
        return Some(true);
    }
    let found = normalize::place(location);
    let lower = location.to_lowercase();
    let regions_named: Vec<&str> = REGIONS
        .iter()
        .filter(|(_, _, names)| names.iter().any(|n| has_phrase(&lower, n)))
        .map(|(r, _, _)| *r)
        .collect();
    if found.cities.is_empty() && found.countries.is_empty() && regions_named.is_empty() {
        return None;
    }
    let country_hit = found
        .countries
        .iter()
        .any(|c| l.countries.iter().any(|x| x == c))
        || regions_named
            .iter()
            .filter_map(|r| region_country(r))
            .any(|c| l.countries.iter().any(|x| x == c));
    let region_hit = regions_named
        .iter()
        .any(|r| l.regions.iter().any(|x| x == r))
        || found
            .cities
            .iter()
            .filter_map(|c| city_region(c))
            .any(|r| l.regions.iter().any(|x| x == r));
    let city_hit = found.cities.iter().any(|c| l.cities.iter().any(|x| x == c));
    if country_hit || region_hit || city_hit {
        return Some(true);
    }
    // Known to be elsewhere only when every selected place could be ruled
    // out: a country-only text cannot rule out a city in that country.
    let only_country = found.cities.is_empty() && regions_named.is_empty();
    let could_contain = only_country
        && found.countries.iter().any(|c| {
            l.cities.iter().any(|city| city_country(city) == Some(*c))
                || l.regions.iter().any(|r| region_country(r) == Some(*c))
        });
    let region_unknown = !l.regions.is_empty()
        && !found.cities.is_empty()
        && found.cities.iter().all(|c| city_region(c).is_none())
        && found
            .countries
            .iter()
            .any(|c| l.regions.iter().any(|r| region_country(r) == Some(*c)));
    if could_contain || region_unknown {
        None
    } else {
        Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dach_expands_visibly_and_selections_are_a_union() {
        let (l, notes) = parse("Python/AI contracts in DACH");
        assert_eq!(l.countries, ["Germany", "Austria", "Switzerland"]);
        assert_eq!(label(&l), "Germany, Austria or Switzerland (DACH)");
        assert!(notes.is_empty());
        let (l, _) = parse("companies in Austria and Vienna and Bavaria");
        assert_eq!(l.countries, ["Austria"]);
        assert!(l.cities.is_empty(), "Vienna is inside Austria: {l:?}");
        assert_eq!(l.regions, ["Bavaria"]);
        let (l, _) = parse("Munich or Bavaria");
        assert_eq!(l.regions, ["Bavaria"]);
        assert!(l.cities.is_empty(), "{l:?}");
    }

    #[test]
    fn country_adjectives_before_organizations_name_countries() {
        let (l, _) = parse("Find Austrian and German manufacturing companies");
        assert_eq!(l.countries, ["Austria", "Germany"]);
        let (l, _) = parse("Find Python contracts that need fluent German");
        assert!(l.countries.is_empty(), "{l:?}");
    }

    #[test]
    fn places_match_by_or_and_unknown_stays_unknown() {
        let (l, _) = parse("Vienna or Graz");
        assert_eq!(l.cities, ["Vienna", "Graz"]);
        assert_eq!(matches(&l, "Graz, Austria"), Some(true));
        assert_eq!(matches(&l, "Linz, Austria"), Some(false));
        assert_eq!(
            matches(&l, "Austria"),
            None,
            "a country cannot rule out its cities"
        );
        assert_eq!(matches(&l, "Remote"), None);
        let (bavaria, _) = parse("Bavaria");
        assert_eq!(matches(&bavaria, "Nuremberg, Germany"), Some(true));
        assert_eq!(matches(&bavaria, "Hamburg, Germany"), Some(false));
        assert_eq!(matches(&bavaria, "Bayern"), Some(true));
        let (dach, _) = parse("DACH");
        assert_eq!(matches(&dach, "Zürich"), Some(true));
        assert_eq!(matches(&dach, "Paris, France"), Some(false));
    }

    #[test]
    fn a_radius_is_stated_as_unavailable_and_ambiguity_is_explained() {
        let (l, notes) = parse("within 50 km of Salzburg");
        assert_eq!(l.radius_km, Some(50));
        assert_eq!(l.cities, ["Salzburg"]);
        assert!(notes.iter().any(|n| n.contains("reliable coordinates")));
        assert!(notes
            .iter()
            .any(|n| n.contains("Salzburg is read as the city")));
        let (state, _) = parse("Salzburg state");
        assert_eq!(state.regions, ["Salzburg (state)"]);
        assert!(state.cities.is_empty());
    }
}
