//! Deterministic normalization of job fields.
//!
//! Every function returns `None` when the input does not state a value:
//! salaries, dates, locations and seniority are never guessed. Estimated
//! salaries ("~€100k (est.)") are ignored rather than stored as facts.

use std::sync::OnceLock;

use jiff::{civil::Date, tz::TimeZone, Timestamp};
use regex::Regex;
use reqwest::Url;

use crate::models::analytics::{EmploymentType, SalaryPeriod, Seniority, WorkMode};

const DAY_MS: i64 = 86_400_000;

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

// ── Text ──────────────────────────────────────────────────────────────

/// Collapses whitespace and cuts to `max` characters.
pub fn clip(text: &str, max: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        collapsed
            .chars()
            .take(max)
            .collect::<String>()
            .trim_end()
            .to_string()
    }
}

/// Like [`clip`], but keeps line breaks (descriptions keep their sections).
pub fn clip_lines(text: &str, max: usize) -> String {
    let lines: Vec<String> = text
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect();
    let joined = lines.join("\n");
    if joined.chars().count() <= max {
        joined
    } else {
        joined
            .chars()
            .take(max)
            .collect::<String>()
            .trim_end()
            .to_string()
    }
}

/// Words that mean "not stated".
pub fn is_missing(text: &str) -> bool {
    let t = text
        .trim()
        .trim_matches(|c: char| c == '.' || c == '*' || c == '_')
        .to_lowercase();
    matches!(
        t.as_str(),
        "" | "-"
            | "--"
            | "—"
            | "–"
            | "?"
            | "n/a"
            | "na"
            | "n.a"
            | "none"
            | "null"
            | "unknown"
            | "not stated"
            | "not listed"
            | "not specified"
            | "not disclosed"
            | "not mentioned"
            | "not available"
            | "unspecified"
            | "undisclosed"
            | "tbd"
            | "tba"
            | "k.a"
            | "keine angabe"
            | "competitive"
            | "negotiable"
            | "doe"
            | "varies"
    )
}

/// A value, or `None` when the text says it is not stated.
pub fn stated(text: &str, max: usize) -> Option<String> {
    let clean = clip(text, max);
    (!is_missing(&clean)).then_some(clean)
}

// ── Company & title ───────────────────────────────────────────────────

const LEGAL_SUFFIXES: &[&str] = &[
    "gmbh",
    "mbh",
    "ag",
    "se",
    "kg",
    "og",
    "eu",
    "ug",
    "co",
    "inc",
    "incorporated",
    "corp",
    "corporation",
    "ltd",
    "limited",
    "llc",
    "llp",
    "plc",
    "sa",
    "sas",
    "sarl",
    "srl",
    "spa",
    "bv",
    "nv",
    "oy",
    "oyj",
    "ab",
    "as",
    "asa",
    "aps",
    "pty",
    "pte",
    "kk",
    "group",
    "holding",
];

/// Matching key of a company: lowercase, no punctuation, no legal form.
pub fn company_key(company: &str) -> String {
    let lower = company.to_lowercase();
    let mut words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words.first() == Some(&"the") && words.len() > 1 {
        words.remove(0);
    }
    while words.len() > 1 && words.last().is_some_and(|w| LEGAL_SUFFIXES.contains(w)) {
        words.pop();
    }
    words.join(" ")
}

/// Removes gender markers ("(m/w/d)", "(all genders)") from a title.
pub fn clean_title(title: &str) -> String {
    static MARKER: OnceLock<Regex> = OnceLock::new();
    static TRAILING: OnceLock<Regex> = OnceLock::new();
    let marker = re(
        &MARKER,
        r"(?i)\s*[\(\[]\s*(?:all\s+genders?|alle\s+geschlechter|gn\*?|[mwfdxh](?:\s*[/|,]\s*[mwfdxh]){1,3}|mwd|m|w|f|d|x)\s*[\)\]]",
    );
    let trailing = re(
        &TRAILING,
        r"(?i)[\s,\-–]+(?:[mwfdxh](?:\s*/\s*[mwfdxh]){2,3}|all\s+genders)\s*$",
    );
    let without = marker.replace_all(title, "");
    clip(&trailing.replace_all(&without, ""), 200)
}

const SENIORITY_WORDS: &[&str] = &[
    "senior",
    "sr",
    "junior",
    "jr",
    "lead",
    "principal",
    "staff",
    "intern",
    "internship",
    "trainee",
    "graduate",
    "entry",
    "level",
    "mid",
    "medior",
    "werkstudent",
    "praktikant",
    "praktikum",
    "i",
    "ii",
    "iii",
    "iv",
    "1",
    "2",
    "3",
];

/// The role without seniority or level words: "Senior ML Engineer (m/w/d)"
/// → "Machine Learning Engineer". Abbreviations are expanded so equivalent
/// titles group together.
pub fn role(title: &str) -> String {
    let cleaned = clean_title(title);
    let mut words: Vec<String> = Vec::new();
    let mut skip_next_student = false;
    for raw in cleaned.split_whitespace() {
        let word = raw.trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '(' | ')' | '|'));
        let lower = word.trim_end_matches('.').to_lowercase();
        if lower.is_empty() || matches!(lower.as_str(), "-" | "–" | "—") {
            continue;
        }
        if lower == "working" {
            skip_next_student = true;
            continue;
        }
        if skip_next_student && lower == "student" {
            skip_next_student = false;
            continue;
        }
        if skip_next_student {
            words.push("Working".into());
            skip_next_student = false;
        }
        if SENIORITY_WORDS.contains(&lower.as_str())
            || lower.split('-').all(|p| SENIORITY_WORDS.contains(&p))
        {
            continue;
        }
        words.push(match lower.as_str() {
            "ml" => "Machine Learning".into(),
            "swe" => "Software Engineer".into(),
            "sre" => "Site Reliability Engineer".into(),
            "eng" => "Engineer".into(),
            "mgr" => "Manager".into(),
            "dev" => "Developer".into(),
            _ => word.to_string(),
        });
    }
    let joined = words.join(" ");
    if joined.is_empty() {
        cleaned
    } else {
        joined
    }
}

/// Seniority stated by a title or a seniority column; `None` if not stated.
pub fn seniority(text: &str) -> Option<Seniority> {
    let lower = format!(
        " {} ",
        text.to_lowercase()
            .replace(['(', ')', ',', '/', '-', '.'], " ")
    );
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(&format!(" {w} ")));
    if has(&[
        "head of",
        "director",
        "vp",
        "vice president",
        "chief",
        "cto",
        "cdo",
        "cio",
        "executive",
        "c level",
    ]) {
        Some(Seniority::Executive)
    } else if has(&[
        "intern",
        "internship",
        "praktikum",
        "praktikant",
        "working student",
        "werkstudent",
        "trainee",
    ]) {
        Some(Seniority::Intern)
    } else if has(&[
        "lead",
        "staff",
        "principal",
        "tech lead",
        "team lead",
        "distinguished",
    ]) {
        Some(Seniority::Lead)
    } else if has(&["senior", "sr", "experienced"]) {
        Some(Seniority::Senior)
    } else if has(&[
        "junior",
        "jr",
        "entry",
        "entry level",
        "graduate",
        "new grad",
        "berufseinsteiger",
    ]) {
        Some(Seniority::Entry)
    } else if has(&["mid", "mid level", "medior", "intermediate"]) {
        Some(Seniority::Mid)
    } else {
        None
    }
}

/// Remote / hybrid / on-site as stated in a column, location or title.
pub fn work_mode(text: &str) -> Option<WorkMode> {
    let t = text.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| t.contains(w));
    let hybrid = any(&[
        "hybrid",
        "partially remote",
        "partly remote",
        "remote days",
        "home office",
        "homeoffice",
        "home-office",
        "remote possible",
        "remote optional",
        "flexible remote",
        "teilweise remote",
    ]);
    let not_remote = any(&["no remote", "not remote", "kein remote"]);
    let remote = !not_remote
        && any(&[
            "remote",
            "work from home",
            "wfh",
            "fully distributed",
            "telecommute",
            "anywhere",
            "100% home",
        ]);
    let onsite = not_remote
        || any(&[
            "on-site",
            "onsite",
            "on site",
            "in office",
            "in-office",
            "office-based",
            "office based",
            "vor ort",
            "präsenz",
        ]);
    if hybrid || (remote && onsite && !not_remote) {
        Some(WorkMode::Hybrid)
    } else if remote {
        Some(WorkMode::Remote)
    } else if onsite {
        Some(WorkMode::Onsite)
    } else {
        None
    }
}

pub fn employment_type(text: &str) -> Option<EmploymentType> {
    let t = text.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if any(&["intern", "praktikum", "working student", "werkstudent"]) {
        Some(EmploymentType::Internship)
    } else if any(&["freelance", "freiberuf", "self-employed"]) {
        Some(EmploymentType::Freelance)
    } else if any(&["contract", "contractor", "b2b"]) {
        Some(EmploymentType::Contract)
    } else if any(&[
        "temporary",
        "fixed-term",
        "fixed term",
        "befristet",
        "maternity cover",
    ]) {
        Some(EmploymentType::Temporary)
    } else if any(&["part-time", "part time", "teilzeit", "parttime"]) {
        Some(EmploymentType::PartTime)
    } else if any(&[
        "full-time",
        "full time",
        "fulltime",
        "vollzeit",
        "permanent",
        "unbefristet",
    ]) {
        Some(EmploymentType::FullTime)
    } else {
        None
    }
}

// ── Location ──────────────────────────────────────────────────────────

/// Country names and the other ways jobs write them. Codes only count when
/// written in capitals ("AT", "DE"); codes that are also US states (CA, IN)
/// or common words (IT, NO) are not used.
static COUNTRIES: &[(&str, &[&str], &[&str])] = &[
    (
        "Austria",
        &["austria", "österreich", "osterreich", "oesterreich"],
        &["AT", "AUT"],
    ),
    (
        "Germany",
        &["germany", "deutschland"],
        &["DE", "DEU", "GER"],
    ),
    (
        "Switzerland",
        &["switzerland", "schweiz", "suisse", "svizzera"],
        &["CH", "CHE"],
    ),
    (
        "Netherlands",
        &["netherlands", "the netherlands", "holland", "nederland"],
        &["NL"],
    ),
    ("Belgium", &["belgium", "belgien", "belgique"], &["BE"]),
    ("Luxembourg", &["luxembourg", "luxemburg"], &["LU"]),
    ("France", &["france", "frankreich"], &["FR"]),
    ("Spain", &["spain", "spanien", "españa", "espana"], &["ES"]),
    ("Portugal", &["portugal"], &["PT"]),
    ("Italy", &["italy", "italien", "italia"], &[]),
    ("Ireland", &["ireland", "irland"], &["IE"]),
    (
        "United Kingdom",
        &[
            "united kingdom",
            "england",
            "scotland",
            "wales",
            "great britain",
            "britain",
            "northern ireland",
        ],
        &["UK", "GB"],
    ),
    ("Poland", &["poland", "polen", "polska"], &["PL"]),
    (
        "Czechia",
        &["czechia", "czech republic", "tschechien", "česko"],
        &["CZ"],
    ),
    ("Slovakia", &["slovakia", "slowakei"], &["SK"]),
    ("Hungary", &["hungary", "ungarn"], &["HU"]),
    ("Romania", &["romania", "rumänien"], &["RO"]),
    ("Bulgaria", &["bulgaria", "bulgarien"], &["BG"]),
    ("Croatia", &["croatia", "kroatien"], &["HR"]),
    ("Slovenia", &["slovenia", "slowenien"], &["SI"]),
    ("Serbia", &["serbia", "serbien"], &["RS"]),
    ("Greece", &["greece", "griechenland"], &["GR"]),
    ("Sweden", &["sweden", "schweden", "sverige"], &["SE"]),
    ("Denmark", &["denmark", "dänemark", "danmark"], &["DK"]),
    ("Norway", &["norway", "norwegen", "norge"], &[]),
    ("Finland", &["finland", "finnland", "suomi"], &["FI"]),
    ("Estonia", &["estonia", "estland"], &["EE"]),
    ("Latvia", &["latvia", "lettland"], &["LV"]),
    ("Lithuania", &["lithuania", "litauen"], &["LT"]),
    ("Ukraine", &["ukraine"], &["UA"]),
    (
        "Turkey",
        &["turkey", "türkei", "türkiye", "turkiye"],
        &["TR"],
    ),
    ("Israel", &["israel"], &[]),
    ("Malta", &["malta"], &[]),
    ("Cyprus", &["cyprus", "zypern"], &["CY"]),
    (
        "United States",
        &[
            "united states",
            "united states of america",
            "usa",
            "america",
        ],
        &["US", "USA"],
    ),
    ("Canada", &["canada", "kanada"], &[]),
    ("Mexico", &["mexico", "mexiko"], &["MX"]),
    ("Brazil", &["brazil", "brasilien", "brasil"], &["BR"]),
    ("Argentina", &["argentina", "argentinien"], &[]),
    ("India", &["india", "indien"], &[]),
    ("Singapore", &["singapore", "singapur"], &["SG"]),
    ("Japan", &["japan"], &["JP"]),
    ("China", &["china"], &["CN"]),
    (
        "South Korea",
        &["south korea", "korea", "südkorea"],
        &["KR"],
    ),
    ("Australia", &["australia", "australien"], &["AU"]),
    ("New Zealand", &["new zealand", "neuseeland"], &["NZ"]),
    (
        "United Arab Emirates",
        &["united arab emirates", "uae", "vae"],
        &["AE"],
    ),
    ("South Africa", &["south africa", "südafrika"], &["ZA"]),
];

/// Cities (with the names they are written as) and their country.
static CITIES: &[(&str, &[&str], &str)] = &[
    ("Vienna", &["vienna", "wien"], "Austria"),
    ("Graz", &["graz"], "Austria"),
    ("Linz", &["linz"], "Austria"),
    ("Salzburg", &["salzburg"], "Austria"),
    ("Innsbruck", &["innsbruck"], "Austria"),
    ("Klagenfurt", &["klagenfurt"], "Austria"),
    (
        "St. Pölten",
        &["st. pölten", "st pölten", "sankt pölten", "st. poelten"],
        "Austria",
    ),
    ("Wels", &["wels"], "Austria"),
    ("Villach", &["villach"], "Austria"),
    ("Dornbirn", &["dornbirn"], "Austria"),
    ("Hagenberg", &["hagenberg"], "Austria"),
    ("Berlin", &["berlin"], "Germany"),
    ("Munich", &["munich", "münchen", "muenchen"], "Germany"),
    ("Hamburg", &["hamburg"], "Germany"),
    ("Frankfurt", &["frankfurt", "frankfurt am main"], "Germany"),
    ("Cologne", &["cologne", "köln", "koeln"], "Germany"),
    ("Stuttgart", &["stuttgart"], "Germany"),
    (
        "Düsseldorf",
        &["düsseldorf", "dusseldorf", "duesseldorf"],
        "Germany",
    ),
    ("Dortmund", &["dortmund"], "Germany"),
    ("Essen", &["essen"], "Germany"),
    ("Leipzig", &["leipzig"], "Germany"),
    ("Dresden", &["dresden"], "Germany"),
    ("Hanover", &["hanover", "hannover"], "Germany"),
    (
        "Nuremberg",
        &["nuremberg", "nürnberg", "nuernberg"],
        "Germany",
    ),
    ("Bremen", &["bremen"], "Germany"),
    ("Karlsruhe", &["karlsruhe"], "Germany"),
    ("Mannheim", &["mannheim"], "Germany"),
    ("Heidelberg", &["heidelberg"], "Germany"),
    ("Bonn", &["bonn"], "Germany"),
    ("Aachen", &["aachen"], "Germany"),
    ("Darmstadt", &["darmstadt"], "Germany"),
    ("Walldorf", &["walldorf"], "Germany"),
    ("Ingolstadt", &["ingolstadt"], "Germany"),
    ("Wolfsburg", &["wolfsburg"], "Germany"),
    ("Erlangen", &["erlangen"], "Germany"),
    ("Zurich", &["zurich", "zürich", "zuerich"], "Switzerland"),
    (
        "Geneva",
        &["geneva", "genève", "geneve", "genf"],
        "Switzerland",
    ),
    ("Basel", &["basel"], "Switzerland"),
    ("Bern", &["bern", "berne"], "Switzerland"),
    ("Lausanne", &["lausanne"], "Switzerland"),
    ("Zug", &["zug"], "Switzerland"),
    ("Lucerne", &["lucerne", "luzern"], "Switzerland"),
    ("Amsterdam", &["amsterdam"], "Netherlands"),
    ("Rotterdam", &["rotterdam"], "Netherlands"),
    ("The Hague", &["the hague", "den haag"], "Netherlands"),
    ("Utrecht", &["utrecht"], "Netherlands"),
    ("Eindhoven", &["eindhoven"], "Netherlands"),
    ("Delft", &["delft"], "Netherlands"),
    ("Brussels", &["brussels", "bruxelles", "brüssel"], "Belgium"),
    ("Antwerp", &["antwerp", "antwerpen"], "Belgium"),
    ("Ghent", &["ghent", "gent"], "Belgium"),
    ("Paris", &["paris"], "France"),
    ("Lyon", &["lyon"], "France"),
    ("Toulouse", &["toulouse"], "France"),
    ("Marseille", &["marseille"], "France"),
    ("Grenoble", &["grenoble"], "France"),
    ("Bordeaux", &["bordeaux"], "France"),
    ("Lille", &["lille"], "France"),
    ("Nantes", &["nantes"], "France"),
    ("Madrid", &["madrid"], "Spain"),
    ("Barcelona", &["barcelona"], "Spain"),
    ("Valencia", &["valencia"], "Spain"),
    ("Seville", &["seville", "sevilla"], "Spain"),
    ("Málaga", &["málaga", "malaga"], "Spain"),
    ("Lisbon", &["lisbon", "lisboa", "lissabon"], "Portugal"),
    ("Porto", &["porto"], "Portugal"),
    ("Milan", &["milan", "milano", "mailand"], "Italy"),
    ("Rome", &["rome", "roma", "rom"], "Italy"),
    ("Turin", &["turin", "torino"], "Italy"),
    ("Bologna", &["bologna"], "Italy"),
    ("Dublin", &["dublin"], "Ireland"),
    ("Cork", &["cork"], "Ireland"),
    ("Galway", &["galway"], "Ireland"),
    ("London", &["london"], "United Kingdom"),
    ("Manchester", &["manchester"], "United Kingdom"),
    ("Edinburgh", &["edinburgh"], "United Kingdom"),
    ("Oxford", &["oxford"], "United Kingdom"),
    ("Bristol", &["bristol"], "United Kingdom"),
    ("Glasgow", &["glasgow"], "United Kingdom"),
    ("Leeds", &["leeds"], "United Kingdom"),
    ("Belfast", &["belfast"], "United Kingdom"),
    ("Warsaw", &["warsaw", "warszawa", "warschau"], "Poland"),
    (
        "Kraków",
        &["kraków", "krakow", "cracow", "krakau"],
        "Poland",
    ),
    ("Wrocław", &["wrocław", "wroclaw", "breslau"], "Poland"),
    ("Gdańsk", &["gdańsk", "gdansk"], "Poland"),
    ("Poznań", &["poznań", "poznan"], "Poland"),
    ("Prague", &["prague", "praha", "prag"], "Czechia"),
    ("Brno", &["brno"], "Czechia"),
    ("Bratislava", &["bratislava", "pressburg"], "Slovakia"),
    ("Budapest", &["budapest"], "Hungary"),
    (
        "Bucharest",
        &["bucharest", "bucurești", "bukarest"],
        "Romania",
    ),
    ("Cluj-Napoca", &["cluj-napoca", "cluj"], "Romania"),
    ("Sofia", &["sofia"], "Bulgaria"),
    ("Zagreb", &["zagreb"], "Croatia"),
    ("Ljubljana", &["ljubljana"], "Slovenia"),
    ("Belgrade", &["belgrade", "beograd", "belgrad"], "Serbia"),
    ("Athens", &["athens", "athen"], "Greece"),
    ("Stockholm", &["stockholm"], "Sweden"),
    ("Gothenburg", &["gothenburg", "göteborg"], "Sweden"),
    ("Malmö", &["malmö", "malmo"], "Sweden"),
    (
        "Copenhagen",
        &["copenhagen", "københavn", "kopenhagen"],
        "Denmark",
    ),
    ("Aarhus", &["aarhus"], "Denmark"),
    ("Oslo", &["oslo"], "Norway"),
    ("Helsinki", &["helsinki"], "Finland"),
    ("Espoo", &["espoo"], "Finland"),
    ("Tallinn", &["tallinn"], "Estonia"),
    ("Riga", &["riga"], "Latvia"),
    ("Vilnius", &["vilnius"], "Lithuania"),
    ("Kyiv", &["kyiv", "kiev"], "Ukraine"),
    ("Lviv", &["lviv"], "Ukraine"),
    ("Istanbul", &["istanbul"], "Turkey"),
    ("Tel Aviv", &["tel aviv", "tel-aviv"], "Israel"),
    (
        "New York",
        &["new york", "new york city", "nyc"],
        "United States",
    ),
    (
        "San Francisco",
        &["san francisco", "sf bay area", "bay area"],
        "United States",
    ),
    ("Seattle", &["seattle"], "United States"),
    ("Boston", &["boston"], "United States"),
    ("Austin", &["austin"], "United States"),
    ("Los Angeles", &["los angeles"], "United States"),
    ("Chicago", &["chicago"], "United States"),
    ("Denver", &["denver"], "United States"),
    ("Atlanta", &["atlanta"], "United States"),
    ("Miami", &["miami"], "United States"),
    ("San Jose", &["san jose"], "United States"),
    ("Palo Alto", &["palo alto"], "United States"),
    ("Mountain View", &["mountain view"], "United States"),
    ("Redmond", &["redmond"], "United States"),
    ("Pittsburgh", &["pittsburgh"], "United States"),
    ("Toronto", &["toronto"], "Canada"),
    ("Vancouver", &["vancouver"], "Canada"),
    ("Montreal", &["montreal", "montréal"], "Canada"),
    ("Ottawa", &["ottawa"], "Canada"),
    ("Bangalore", &["bangalore", "bengaluru"], "India"),
    ("Hyderabad", &["hyderabad"], "India"),
    ("Pune", &["pune"], "India"),
    ("Mumbai", &["mumbai"], "India"),
    ("Tokyo", &["tokyo"], "Japan"),
    ("Seoul", &["seoul"], "South Korea"),
    ("Beijing", &["beijing"], "China"),
    ("Shanghai", &["shanghai"], "China"),
    ("Sydney", &["sydney"], "Australia"),
    ("Melbourne", &["melbourne"], "Australia"),
    ("Auckland", &["auckland"], "New Zealand"),
    ("Dubai", &["dubai"], "United Arab Emirates"),
    ("Abu Dhabi", &["abu dhabi"], "United Arab Emirates"),
];

/// What a location text states.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Place {
    /// Cities in order of mention.
    pub cities: Vec<&'static str>,
    /// Countries (stated or of a stated city), in order of mention.
    pub countries: Vec<&'static str>,
}

impl Place {
    pub fn city(&self) -> Option<&'static str> {
        self.cities.first().copied()
    }

    pub fn country(&self) -> Option<&'static str> {
        self.countries.first().copied()
    }
}

/// Reads cities and countries from a location such as "Vienna, AT
/// (Hybrid)" or "Berlin / Munich". Regions ("EU", "Remote (Europe)") name no
/// country.
pub fn place(location: &str) -> Place {
    let mut out = Place::default();
    let push_country = |out: &mut Place, c: &'static str| {
        if !out.countries.contains(&c) {
            out.countries.push(c);
        }
    };
    // Capitalized country codes are separate words.
    for word in location.split(|c: char| !c.is_alphanumeric()) {
        if word.len() >= 2 && word.chars().all(|c| c.is_ascii_uppercase()) {
            if let Some((name, _, _)) = COUNTRIES.iter().find(|(_, _, codes)| codes.contains(&word))
            {
                push_country(&mut out, name);
            }
        }
    }
    let lower = location.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '-'))
        .map(|w| w.trim_matches(|c| c == '.' || c == '-'))
        .filter(|w| !w.is_empty())
        .collect();
    let mut i = 0;
    let mut found: Vec<(usize, &'static str, Option<&'static str>)> = Vec::new();
    while i < words.len() {
        let mut matched = 0;
        for len in (1..=4.min(words.len() - i)).rev() {
            let phrase = words[i..i + len].join(" ");
            let alt = words[i..i + len].join("-");
            if let Some((city, _, country)) = CITIES.iter().find(|(_, names, _)| {
                names.contains(&phrase.as_str()) || names.contains(&alt.as_str())
            }) {
                found.push((i, country, Some(city)));
                matched = len;
                break;
            }
            if let Some((country, _, _)) = COUNTRIES
                .iter()
                .find(|(_, names, _)| names.contains(&phrase.as_str()))
            {
                found.push((i, country, None));
                matched = len;
                break;
            }
        }
        i += matched.max(1);
    }
    for (_, country, city) in found {
        if let Some(city) = city {
            if !out.cities.contains(&city) {
                out.cities.push(city);
            }
        }
        push_country(&mut out, country);
    }
    // Codes found first but mentioned after names: keep mention order simple
    // by moving the country of the first city to the front.
    if let Some(city) = out.cities.first() {
        if let Some((_, _, country)) = CITIES.iter().find(|(c, _, _)| c == city) {
            if let Some(pos) = out.countries.iter().position(|c| c == country) {
                let c = out.countries.remove(pos);
                out.countries.insert(0, c);
            }
        }
    }
    out
}

/// The canonical country for a user-typed name ("österreich" → "Austria").
pub fn country_name(text: &str) -> Option<&'static str> {
    let t = text.trim();
    let lower = t.to_lowercase();
    COUNTRIES
        .iter()
        .find(|(name, names, codes)| {
            name.to_lowercase() == lower || names.contains(&lower.as_str()) || codes.contains(&t)
        })
        .map(|(name, _, _)| *name)
}

/// The canonical city for a user-typed name ("Wien" → "Vienna").
pub fn city_name(text: &str) -> Option<&'static str> {
    let lower = text.trim().to_lowercase();
    CITIES
        .iter()
        .find(|(name, names, _)| name.to_lowercase() == lower || names.contains(&lower.as_str()))
        .map(|(name, _, _)| *name)
}

// ── Salary ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Salary {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub currency: Option<&'static str>,
    pub period: Option<SalaryPeriod>,
}

impl Salary {
    /// One comparable figure: the middle of the range, or the stated bound.
    pub fn midpoint(&self) -> Option<f64> {
        match (self.min, self.max) {
            (Some(a), Some(b)) => Some((a + b) / 2.0),
            (a, b) => a.or(b),
        }
    }

    /// Yearly value in the salary's own currency. Only yearly and monthly
    /// salaries are converted (× 12); other periods are not comparable.
    pub fn annual(&self) -> Option<f64> {
        let mid = self.midpoint()?;
        match self.period? {
            SalaryPeriod::Year => Some(mid),
            SalaryPeriod::Month => Some(mid * 12.0),
            _ => None,
        }
    }
}

/// Currencies whose yearly salaries are routinely below 10,000 or whose
/// amounts are too large to tell the period by size.
const NO_PERIOD_GUESS: &[&str] = &["JPY", "HUF", "INR", "KRW", "CNY", "CZK", "IDR"];

fn currency(lower: &str) -> Option<&'static str> {
    const CODES: &[(&str, &str)] = &[
        ("ca$", "CAD"),
        ("c$", "CAD"),
        ("a$", "AUD"),
        ("au$", "AUD"),
        ("nz$", "NZD"),
        ("s$", "SGD"),
        ("hk$", "HKD"),
        ("us$", "USD"),
        ("€", "EUR"),
        ("£", "GBP"),
        ("₹", "INR"),
        ("¥", "JPY"),
        ("zł", "PLN"),
        ("kč", "CZK"),
    ];
    for (sign, code) in CODES {
        if lower.contains(sign) {
            return Some(code);
        }
    }
    const WORDS: &[(&str, &str)] = &[
        ("eur", "EUR"),
        ("euro", "EUR"),
        ("usd", "USD"),
        ("gbp", "GBP"),
        ("chf", "CHF"),
        ("cad", "CAD"),
        ("aud", "AUD"),
        ("nzd", "NZD"),
        ("sgd", "SGD"),
        ("sek", "SEK"),
        ("nok", "NOK"),
        ("dkk", "DKK"),
        ("pln", "PLN"),
        ("czk", "CZK"),
        ("huf", "HUF"),
        ("ron", "RON"),
        ("inr", "INR"),
        ("jpy", "JPY"),
        ("cny", "CNY"),
        ("rmb", "CNY"),
        ("hkd", "HKD"),
        ("aed", "AED"),
        ("ils", "ILS"),
    ];
    for word in lower.split(|c: char| !c.is_alphabetic()) {
        let w = word.trim_end_matches('s');
        if let Some((_, code)) = WORDS.iter().find(|(name, _)| *name == w || *name == word) {
            return Some(code);
        }
    }
    if lower.contains('$') {
        return Some("USD");
    }
    None
}

fn period(lower: &str) -> Option<SalaryPeriod> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let r = re(
        &RE,
        r"(?x)
        (?P<year>\b(?:per\s+)?(?:year|yr|annum|annual(?:ly)?|jahr|jährlich|pa)\b|\bp\.\s?a\b|/\s?(?:y|yr|year)\b)
        |(?P<month>\b(?:per\s+)?(?:month|mo|monthly|monat|monatlich|mtl\.?)\b|/\s?(?:m|mo|month)\b)
        |(?P<week>\b(?:per\s+)?(?:week|wk|weekly|woche)\b|/\s?(?:w|wk|week)\b)
        |(?P<day>\b(?:per\s+)?(?:day|daily|tag|per\s+diem|tagessatz)\b|/\s?(?:d|day)\b)
        |(?P<hour>\b(?:per\s+)?(?:hour|hr|hourly|stunde|std\.?)\b|/\s?(?:h|hr|hour)\b)",
    );
    let caps = r.captures(lower)?;
    [
        ("year", SalaryPeriod::Year),
        ("month", SalaryPeriod::Month),
        ("week", SalaryPeriod::Week),
        ("day", SalaryPeriod::Day),
        ("hour", SalaryPeriod::Hour),
    ]
    .into_iter()
    .find(|(name, _)| caps.name(name).is_some())
    .map(|(_, p)| p)
}

/// A number token: "110k", "110,000", "110.000", "90'000", "45.50", "1.2m".
fn amount(digits: &str, suffix: &str) -> Option<f64> {
    let d = digits.replace([' ', '\u{a0}', '\u{202f}', '\''], "");
    static THOUSANDS: OnceLock<Regex> = OnceLock::new();
    let thousands = re(&THOUSANDS, r"^\d{1,3}(?:[.,]\d{3})+$");
    let value: f64 = if thousands.is_match(&d) {
        d.replace(['.', ','], "").parse().ok()?
    } else {
        d.replace(',', ".").parse().ok()?
    };
    let factor = match suffix.to_lowercase().as_str() {
        "k" => 1_000.0,
        "m" | "mio" => 1_000_000.0,
        _ => 1.0,
    };
    Some(value * factor)
}

/// Parses a stated salary. Returns `None` for estimates, missing values and
/// text without an amount.
pub fn salary(text: &str) -> Option<Salary> {
    let lower = text.to_lowercase();
    if is_missing(&lower) {
        return None;
    }
    static ESTIMATE: OnceLock<Regex> = OnceLock::new();
    let estimate = re(
        &ESTIMATE,
        r"(?i)(~|≈|\best\b\.?|estimat|approx|circa\b|ca\.\s|typical|average|avg\b|market rate|glassdoor|levels\.fyi|likely)",
    );
    if estimate.is_match(&lower) {
        return None;
    }
    static NUMBER: OnceLock<Regex> = OnceLock::new();
    let number = re(
        &NUMBER,
        r"(\d{1,3}(?:[.,'\u{a0}\u{202f} ]\d{3})+|\d+(?:[.,]\d{1,2})?)\s?(k|m\b|mio\b)?",
    );
    // (value, unit factor, start, end)
    let mut values: Vec<(f64, f64, usize, usize)> = Vec::new();
    for caps in number.captures_iter(&lower) {
        let whole = caps.get(0)?;
        let suffix = caps.get(2).map_or("", |m| m.as_str());
        // "k" must not start a word ("10 kr").
        if suffix == "k" && lower[whole.end()..].starts_with(|c: char| c.is_alphabetic()) {
            continue;
        }
        let Some(v) = amount(caps.get(1)?.as_str().trim(), suffix) else {
            continue;
        };
        let factor = amount("1", suffix).unwrap_or(1.0);
        values.push((v, factor, whole.start(), whole.end()));
    }
    // Years ("2025") and small counts are not salaries when other amounts exist.
    if values.len() > 1 {
        values.retain(|(v, factor, _, _)| *factor > 1.0 || !(1900.0..=2100.0).contains(v));
    }
    let (mut min, mut max) = match values.as_slice() {
        [] => return None,
        [(v, ..)] => (Some(*v), Some(*v)),
        [(a, a_factor, _, a_end), (b, b_factor, b_start, _), ..] => {
            let between = &lower[*a_end..*b_start];
            let range = between.contains(['-', '–', '—'])
                || between.contains("to")
                || between.contains("bis");
            if !range {
                (Some(*a), Some(*a))
            } else if *a_factor == 1.0 && *b_factor > 1.0 && *a < 1_000.0 {
                // "90–110k": the unit applies to both ends.
                (Some(a * b_factor), Some(*b))
            } else {
                (Some(*a), Some(*b))
            }
        }
    };
    let up_to = lower.contains("up to") || lower.contains("bis zu") || lower.contains("max");
    let from = lower.contains("from ")
        || lower.starts_with("ab ")
        || lower.contains(" ab ")
        || lower.contains("at least")
        || lower.contains("min.")
        || lower.contains("mind.")
        || lower.contains("mindestens")
        || lower.contains("minimum")
        || lower.trim_end().ends_with('+');
    if min == max {
        if up_to {
            min = None;
        } else if from {
            max = None;
        }
    }
    if let (Some(a), Some(b)) = (min, max) {
        if a > b {
            (min, max) = (Some(b), Some(a));
        }
    }
    let currency = currency(&lower);
    let largest = min.into_iter().chain(max).fold(0.0, f64::max);
    if largest <= 0.0 {
        return None;
    }
    let period = period(&lower).or_else(|| {
        (largest >= 10_000.0 && currency.is_some_and(|c| !NO_PERIOD_GUESS.contains(&c)))
            .then_some(SalaryPeriod::Year)
    });
    Some(Salary {
        min,
        max,
        currency,
        period,
    })
}

fn format_amount(value: f64) -> String {
    if value >= 10_000.0 && (value % 1_000.0).abs() < f64::EPSILON {
        format!("{}k", (value / 1_000.0) as i64)
    } else if value >= 10_000.0 {
        format!("{:.1}k", value / 1_000.0)
    } else if value.fract() == 0.0 {
        let s = (value as i64).to_string();
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    } else {
        format!("{value:.2}")
    }
}

/// A salary as ReMa understood it, e.g. "€90k–110k / year".
pub fn format_salary(s: &Salary) -> String {
    let (prefix, suffix) = match s.currency {
        Some("EUR") => ("€".to_string(), String::new()),
        Some("USD") => ("$".to_string(), String::new()),
        Some("GBP") => ("£".to_string(), String::new()),
        Some(code) => (format!("{code} "), String::new()),
        None => (String::new(), " (currency not stated)".to_string()),
    };
    let amount = match (s.min, s.max) {
        (Some(a), Some(b)) if (a - b).abs() < f64::EPSILON => {
            format!("{prefix}{}", format_amount(a))
        }
        (Some(a), Some(b)) => format!("{prefix}{}–{}", format_amount(a), format_amount(b)),
        (Some(a), None) => format!("from {prefix}{}", format_amount(a)),
        (None, Some(b)) => format!("up to {prefix}{}", format_amount(b)),
        (None, None) => String::new(),
    };
    let period = s
        .period
        .map(|p| format!(" / {}", p.as_str()))
        .unwrap_or_default();
    format!("{amount}{period}{suffix}")
}

// ── Dates ─────────────────────────────────────────────────────────────

fn day_ms(date: Date) -> Option<i64> {
    Some(
        date.to_zoned(TimeZone::UTC)
            .ok()?
            .timestamp()
            .as_millisecond(),
    )
}

/// The UTC calendar day of a timestamp.
pub fn date_of(ms: i64) -> Date {
    Timestamp::from_millisecond(ms)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned(TimeZone::UTC)
        .date()
}

/// Midnight (UTC) of the day of `ms`.
pub fn start_of_day(ms: i64) -> i64 {
    day_ms(date_of(ms)).unwrap_or(ms)
}

fn month(word: &str) -> Option<i8> {
    let w = word.trim_end_matches('.').to_lowercase();
    let n = match w.as_str() {
        "jan" | "january" | "januar" | "jänner" | "jaenner" => 1,
        "feb" | "february" | "februar" => 2,
        "mar" | "march" | "märz" | "maerz" | "mär" => 3,
        "apr" | "april" => 4,
        "may" | "mai" => 5,
        "jun" | "june" | "juni" => 6,
        "jul" | "july" | "juli" => 7,
        "aug" | "august" => 8,
        "sep" | "sept" | "september" => 9,
        "oct" | "october" | "okt" | "oktober" => 10,
        "nov" | "november" => 11,
        "dec" | "december" | "dez" | "dezember" => 12,
        _ => return None,
    };
    Some(n)
}

/// A posting date as the UTC midnight of that day. Relative dates ("3 days
/// ago") count back from when ReMa discovered the job. Ambiguous numeric
/// dates (03/04/2025) and dates after the discovery are rejected.
pub fn posted(text: &str, discovered: i64) -> Option<i64> {
    let lower = text.trim().to_lowercase();
    if is_missing(&lower) {
        return None;
    }
    let today = date_of(discovered);
    let back = |days: i64| day_ms(today.checked_sub(jiff::Span::new().days(days)).ok()?);

    static RELATIVE: OnceLock<Regex> = OnceLock::new();
    let relative = re(
        &RELATIVE,
        r"(?:vor\s+)?(\d{1,3})\s*\+?\s*(minutes?|mins?|hours?|hrs?|h|stunden?|d|days?|tagen?|w|wks?|weeks?|wochen?|mo|months?|monaten?)\b",
    );
    let is_relative = lower.contains("ago") || lower.contains("vor ") || lower.len() <= 5;
    if ["today", "just now", "just posted", "heute", "new", "neu"].contains(&lower.as_str()) {
        return back(0);
    }
    if ["yesterday", "gestern"].contains(&lower.as_str()) {
        return back(1);
    }
    if is_relative {
        if let Some(caps) = relative.captures(&lower) {
            let n: i64 = caps[1].parse().ok()?;
            let unit = &caps[2];
            let days = if unit.starts_with('m') && !unit.starts_with("mo")
                || unit.starts_with('h')
                || unit.starts_with("std")
                || unit.starts_with("stu")
            {
                0
            } else if unit.starts_with('d') || unit.starts_with('t') {
                n
            } else if unit.starts_with('w') {
                n * 7
            } else {
                n * 30
            };
            return back(days);
        }
    }

    static ISO: OnceLock<Regex> = OnceLock::new();
    static DOTTED: OnceLock<Regex> = OnceLock::new();
    static SLASHED: OnceLock<Regex> = OnceLock::new();
    static WORDY: OnceLock<Regex> = OnceLock::new();
    let date = if let Some(c) = re(&ISO, r"\b(\d{4})-(\d{1,2})-(\d{1,2})").captures(&lower) {
        Date::new(c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?).ok()
    } else if let Some(c) = re(&DOTTED, r"\b(\d{1,2})\.\s?(\d{1,2})\.\s?(\d{4})\b").captures(&lower) {
        Date::new(c[3].parse().ok()?, c[2].parse().ok()?, c[1].parse().ok()?).ok()
    } else if let Some(c) = re(&SLASHED, r"\b(\d{1,2})/(\d{1,2})/(\d{4})\b").captures(&lower) {
        let (a, b): (i8, i8) = (c[1].parse().ok()?, c[2].parse().ok()?);
        let year = c[3].parse().ok()?;
        match (a > 12, b > 12) {
            (true, false) => Date::new(year, b, a).ok(),
            (false, true) => Date::new(year, a, b).ok(),
            _ => None, // 03/04/2025: day and month cannot be told apart
        }
    } else if let Some(c) = re(
        &WORDY,
        r"(?:\b(\d{1,2})\.?\s+([a-zäé]{3,9})\.?(?:,?\s+(\d{4}))?|\b([a-zäé]{3,9})\.?\s+(\d{1,2})(?:st|nd|rd|th)?(?:,?\s+(\d{4}))?)",
    )
    .captures(&lower)
    {
        let (day, month_word, year) = if c.get(1).is_some() {
            (c.get(1)?.as_str(), c.get(2)?.as_str(), c.get(3))
        } else {
            (c.get(5)?.as_str(), c.get(4)?.as_str(), c.get(6))
        };
        let m = month(month_word)?;
        let d: i8 = day.parse().ok()?;
        match year {
            Some(y) => Date::new(y.as_str().parse().ok()?, m, d).ok(),
            None => {
                // No year: the most recent such day not after the discovery.
                let this_year = Date::new(today.year(), m, d).ok()?;
                if this_year > today {
                    Date::new(today.year() - 1, m, d).ok()
                } else {
                    Some(this_year)
                }
            }
        }
    } else {
        None
    }?;
    if date > today || date.year() < 2000 {
        return None;
    }
    day_ms(date)
}

/// Days between two timestamps' calendar days.
pub fn days_between(earlier: i64, later: i64) -> i64 {
    (start_of_day(later) - start_of_day(earlier)) / DAY_MS
}

// ── URLs ──────────────────────────────────────────────────────────────

const TRACKING_PARAMS: &[&str] = &[
    "ref",
    "refid",
    "referrer",
    "referer",
    "trk",
    "trkinfo",
    "trackingid",
    "tracking_id",
    "source",
    "src",
    "from",
    "gh_src",
    "lever-source",
    "lever-origin",
    "utm",
    "fbclid",
    "gclid",
    "msclkid",
    "mc_cid",
    "mc_eid",
    "_hsenc",
    "_hsmi",
    "spm",
    "clickid",
    "click_id",
    "sid",
    "campaign",
    "position",
    "pagenum",
    "originalsubdomain",
    "eid",
    "lipi",
    "tk",
    "vjk",
    "advn",
    "alid",
    "sessionid",
    "session",
];

/// A web address without tracking parameters, fragment, "www." and
/// trailing slash, for deduplication.
pub fn canonical_url(url: &str) -> Option<String> {
    let mut parsed = Url::parse(url.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return None;
    }
    parsed.set_fragment(None);
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    let mut pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| {
            let k = k.to_lowercase();
            !k.starts_with("utm_") && !TRACKING_PARAMS.contains(&k.as_str())
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    pairs.sort();
    parsed.set_query(None);
    if !pairs.is_empty() {
        parsed.query_pairs_mut().extend_pairs(pairs.iter());
    }
    let host = parsed.host_str()?.trim_start_matches("www.").to_lowercase();
    let path = parsed.path().trim_end_matches('/');
    let query = parsed.query().map(|q| format!("?{q}")).unwrap_or_default();
    Some(format!("{host}{path}{query}"))
}

/// A job board and the job's id there, from its address.
pub fn platform_id(url: &str) -> Option<(&'static str, String)> {
    let parsed = Url::parse(url.trim()).ok()?;
    let host = parsed.host_str()?.trim_start_matches("www.").to_lowercase();
    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let param = |name: &str| {
        parsed
            .query_pairs()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.into_owned())
            .filter(|v| !v.is_empty())
    };
    let trailing_digits = |s: &str| {
        let digits: String = s.chars().rev().take_while(char::is_ascii_digit).collect();
        (digits.len() >= 5).then(|| digits.chars().rev().collect::<String>())
    };
    if host.ends_with("linkedin.com") {
        if let Some(id) = param("currentJobId") {
            return Some(("linkedin", id));
        }
        let pos = segments.iter().position(|s| *s == "view")?;
        return trailing_digits(segments.get(pos + 1)?).map(|id| ("linkedin", id));
    }
    if host.contains("indeed.") {
        return param("jk")
            .or_else(|| param("vjk"))
            .map(|id| ("indeed", id));
    }
    if host.contains("greenhouse.io") {
        if let Some(id) = param("gh_jid") {
            return Some(("greenhouse", id));
        }
        let pos = segments.iter().position(|s| *s == "jobs")?;
        return segments
            .get(pos + 1)
            .map(|id| ("greenhouse", id.to_string()));
    }
    if let Some(id) = param("gh_jid") {
        return Some(("greenhouse", id));
    }
    if host == "jobs.lever.co" || host == "jobs.eu.lever.co" {
        return segments.get(1).map(|id| ("lever", id.to_string()));
    }
    if host == "jobs.ashbyhq.com" {
        return segments.get(1).map(|id| ("ashby", id.to_string()));
    }
    if host.ends_with("smartrecruiters.com") {
        return segments
            .get(1)
            .and_then(|s| s.split('-').next())
            .map(|id| ("smartrecruiters", id.to_string()));
    }
    if host.ends_with("myworkdayjobs.com") {
        let last = segments.last()?;
        return last
            .rsplit('_')
            .next()
            .filter(|id| id.len() >= 4)
            .map(|id| ("workday", format!("{host}:{id}")));
    }
    if host.contains("personio.") {
        let pos = segments.iter().position(|s| *s == "job")?;
        return segments
            .get(pos + 1)
            .map(|id| ("personio", format!("{host}:{id}")));
    }
    if host.contains("stepstone.") {
        let last = segments.last()?;
        let id: String = last.split("--").last()?.split('-').next()?.to_string();
        return (id.len() >= 5 && id.chars().all(|c| c.is_ascii_digit()))
            .then_some(("stepstone", id));
    }
    if host.contains("glassdoor.") {
        return param("jobListingId")
            .or_else(|| param("jl"))
            .map(|id| ("glassdoor", id));
    }
    if host == "karriere.at" {
        let pos = segments.iter().position(|s| *s == "jobs")?;
        return segments
            .get(pos + 1)
            .filter(|s| s.chars().all(|c| c.is_ascii_digit()))
            .map(|id| ("karriere.at", id.to_string()));
    }
    if host.ends_with("xing.com") {
        return segments
            .last()
            .and_then(|s| trailing_digits(s))
            .map(|id| ("xing", id));
    }
    None
}

/// Display name of where a job was found ("LinkedIn", "greenhouse.io").
pub fn source_name(url: &str) -> Option<String> {
    let parsed = Url::parse(url.trim()).ok()?;
    let host = parsed.host_str()?.trim_start_matches("www.").to_lowercase();
    const KNOWN: &[(&str, &str)] = &[
        ("linkedin.com", "LinkedIn"),
        ("indeed.", "Indeed"),
        ("greenhouse.io", "Greenhouse"),
        ("lever.co", "Lever"),
        ("ashbyhq.com", "Ashby"),
        ("smartrecruiters.com", "SmartRecruiters"),
        ("myworkdayjobs.com", "Workday"),
        ("personio.", "Personio"),
        ("stepstone.", "StepStone"),
        ("glassdoor.", "Glassdoor"),
        ("karriere.at", "karriere.at"),
        ("xing.com", "XING"),
        ("wellfound.com", "Wellfound"),
        ("remoteok.com", "Remote OK"),
        ("weworkremotely.com", "We Work Remotely"),
        ("arbeitsagentur.de", "Bundesagentur für Arbeit"),
        ("ams.at", "AMS"),
        ("monster.", "Monster"),
    ];
    Some(
        KNOWN
            .iter()
            .find(|(part, _)| host.contains(part))
            .map_or(host.clone(), |(_, name)| (*name).to_string()),
    )
}

/// Whether an address points at one specific job (an id in the path or
/// query) rather than a general careers page.
pub fn is_job_specific(url: &str) -> bool {
    if platform_id(url).is_some() {
        return true;
    }
    let Ok(parsed) = Url::parse(url.trim()) else {
        return false;
    };
    let id_like = |s: &str| {
        let digits = s.chars().filter(char::is_ascii_digit).count();
        let hex = s.len() >= 12 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        digits >= 4 || hex
    };
    let path_id = parsed.path_segments().is_some_and(|mut s| s.any(id_like));
    let query_id = parsed.query_pairs().any(|(k, v)| {
        let k = k.to_lowercase();
        matches!(
            k.as_str(),
            "id" | "jobid" | "job_id" | "job" | "vacancy" | "req" | "requisition" | "posting"
        ) && !v.is_empty()
    });
    // "/jobs/ai-platform-engineer", "/careers/engineering/ml-engineer": a
    // hyphenated slug below a jobs section, or three levels deep.
    let slug = parsed.path_segments().is_some_and(|s| {
        let parts: Vec<&str> = s.filter(|p| !p.is_empty()).collect();
        let last_is_slug = parts.last().is_some_and(|p| p.contains('-'));
        let under_jobs = parts[..parts.len().saturating_sub(1)].iter().any(|p| {
            matches!(
                p.to_lowercase().as_str(),
                "jobs"
                    | "job"
                    | "careers"
                    | "career"
                    | "positions"
                    | "position"
                    | "openings"
                    | "vacancies"
                    | "stellen"
                    | "stellenangebote"
                    | "karriere"
                    | "offers"
            )
        });
        last_is_slug && (under_jobs || parts.len() >= 3)
    });
    path_id || query_id || slug
}

/// Whether a URL is an http(s) address without credentials.
pub fn web_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url.trim()).ok()?;
    (matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some()
        && parsed.username().is_empty()
        && parsed.password().is_none())
    .then(|| parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISCOVERED: i64 = 1_758_758_400_000; // 2025-09-25T00:00:00Z

    fn day(y: i16, m: i8, d: i8) -> i64 {
        day_ms(Date::new(y, m, d).unwrap()).unwrap()
    }

    #[test]
    fn normalizes_companies_and_titles() {
        assert_eq!(company_key("Company A GmbH"), "company a");
        assert_eq!(company_key("The Siemens AG"), "siemens");
        assert_eq!(company_key("Globex, Inc."), "globex");
        assert_eq!(clean_title("AI Engineer (m/w/d)"), "AI Engineer");
        assert_eq!(
            clean_title("Data Scientist (all genders)"),
            "Data Scientist"
        );
        assert_eq!(clean_title("ML Engineer - m/f/d"), "ML Engineer");
        assert_eq!(
            role("Senior ML Engineer (m/w/d)"),
            "Machine Learning Engineer"
        );
        assert_eq!(role("Sr. AI Platform Engineer II"), "AI Platform Engineer");
        assert_eq!(role("Lead Data Engineer"), "Data Engineer");
        assert_eq!(role("Head of AI"), "Head of AI");
    }

    #[test]
    fn reads_only_stated_seniority_and_work_mode() {
        assert_eq!(seniority("Senior AI Engineer"), Some(Seniority::Senior));
        assert_eq!(seniority("Staff ML Engineer"), Some(Seniority::Lead));
        assert_eq!(seniority("Junior Data Analyst"), Some(Seniority::Entry));
        assert_eq!(seniority("Head of Data"), Some(Seniority::Executive));
        assert_eq!(seniority("Working Student AI"), Some(Seniority::Intern));
        assert_eq!(seniority("AI Engineer"), None);
        assert_eq!(work_mode("Austria (Remote)"), Some(WorkMode::Remote));
        assert_eq!(work_mode("Vienna Hybrid"), Some(WorkMode::Hybrid));
        assert_eq!(work_mode("On-site"), Some(WorkMode::Onsite));
        assert_eq!(
            work_mode("Remote or on-site in Graz"),
            Some(WorkMode::Hybrid)
        );
        assert_eq!(work_mode("Vienna"), None);
        assert_eq!(
            employment_type("Full-time, permanent"),
            Some(EmploymentType::FullTime)
        );
        assert_eq!(
            employment_type("Contract (6 months)"),
            Some(EmploymentType::Contract)
        );
    }

    #[test]
    fn reads_places() {
        let p = place("Vienna, Austria (Hybrid)");
        assert_eq!((p.city(), p.country()), (Some("Vienna"), Some("Austria")));
        let p = place("Wien, AT");
        assert_eq!((p.city(), p.country()), (Some("Vienna"), Some("Austria")));
        let p = place("Berlin / München");
        assert_eq!(p.cities, ["Berlin", "Munich"]);
        assert_eq!(p.countries, ["Germany"]);
        let p = place("Remote (EU)");
        assert!(p.cities.is_empty() && p.countries.is_empty());
        let p = place("Austria Remote");
        assert_eq!(p.country(), Some("Austria"));
        let p = place("Zürich or Vienna");
        assert_eq!(p.countries, ["Switzerland", "Austria"]);
        // Lowercase words are not country codes.
        assert!(place("remote at home").countries.is_empty());
        assert_eq!(country_name("österreich"), Some("Austria"));
        assert_eq!(city_name("wien"), Some("Vienna"));
    }

    #[test]
    fn parses_stated_salaries_only() {
        let s = salary("€90k–110k").unwrap();
        assert_eq!(
            (s.min, s.max, s.currency, s.period),
            (
                Some(90_000.0),
                Some(110_000.0),
                Some("EUR"),
                Some(SalaryPeriod::Year)
            )
        );
        assert_eq!(s.annual(), Some(100_000.0));
        let s = salary("EUR 70.000 - 85.000 brutto/Jahr").unwrap();
        assert_eq!(
            (s.min, s.max, s.period),
            (Some(70_000.0), Some(85_000.0), Some(SalaryPeriod::Year))
        );
        let s = salary("$150K+").unwrap();
        assert_eq!(
            (s.min, s.max, s.currency),
            (Some(150_000.0), None, Some("USD"))
        );
        let s = salary("£5,000 per month").unwrap();
        assert_eq!(
            (s.period, s.annual()),
            (Some(SalaryPeriod::Month), Some(60_000.0))
        );
        let s = salary("CHF 120'000").unwrap();
        assert_eq!((s.min, s.currency), (Some(120_000.0), Some("CHF")));
        let s = salary("up to €120k").unwrap();
        assert_eq!((s.min, s.max), (None, Some(120_000.0)));
        let s = salary("€45/hour").unwrap();
        assert_eq!((s.period, s.annual()), (Some(SalaryPeriod::Hour), None));
        // Missing, vague or estimated values are not salaries.
        assert_eq!(salary("Not stated"), None);
        assert_eq!(salary("Competitive"), None);
        assert_eq!(salary("~€100k (est.)"), None);
        assert_eq!(salary("€95k (estimated)"), None);
        // No currency: kept, but not comparable to other currencies.
        let s = salary("110k").unwrap();
        assert_eq!((s.currency, s.period), (None, None));
        // Small amounts without a period are not guessed.
        assert_eq!(salary("€5,000").unwrap().period, None);
        assert_eq!(
            format_salary(&salary("€90k–110k").unwrap()),
            "€90k–110k / year"
        );
        assert_eq!(
            format_salary(&salary("from $150k").unwrap()),
            "from $150k / year"
        );
    }

    #[test]
    fn parses_posting_dates_without_guessing() {
        assert_eq!(posted("2025-09-20", DISCOVERED), Some(day(2025, 9, 20)));
        assert_eq!(posted("3 days ago", DISCOVERED), Some(day(2025, 9, 22)));
        assert_eq!(posted("2w ago", DISCOVERED), Some(day(2025, 9, 11)));
        assert_eq!(posted("today", DISCOVERED), Some(day(2025, 9, 25)));
        assert_eq!(posted("5 hours ago", DISCOVERED), Some(day(2025, 9, 25)));
        assert_eq!(posted("Sep 20, 2025", DISCOVERED), Some(day(2025, 9, 20)));
        assert_eq!(
            posted("20. September 2025", DISCOVERED),
            Some(day(2025, 9, 20))
        );
        assert_eq!(posted("20.09.2025", DISCOVERED), Some(day(2025, 9, 20)));
        assert_eq!(posted("Dec 30", DISCOVERED), Some(day(2024, 12, 30)));
        assert_eq!(posted("25/09/2025", DISCOVERED), Some(day(2025, 9, 25)));
        // Ambiguous, future, or missing: unknown.
        assert_eq!(posted("03/04/2025", DISCOVERED), None);
        assert_eq!(posted("2025-10-20", DISCOVERED), None);
        assert_eq!(posted("n/a", DISCOVERED), None);
        assert_eq!(posted("Recently", DISCOVERED), None);
    }

    #[test]
    fn canonicalizes_urls_and_finds_platform_ids() {
        assert_eq!(
            canonical_url("https://www.Example.com/jobs/123/?utm_source=x&ref=abc#top"),
            Some("example.com/jobs/123".into())
        );
        assert_eq!(
            canonical_url("https://example.com/job?b=2&id=7&utm_medium=y"),
            Some("example.com/job?b=2&id=7".into())
        );
        assert_eq!(
            platform_id(
                "https://www.linkedin.com/jobs/view/ai-engineer-at-company-a-4012345678/?trk=x"
            ),
            Some(("linkedin", "4012345678".into()))
        );
        assert_eq!(
            platform_id(
                "https://www.linkedin.com/jobs/search/?currentJobId=4012345678&keywords=ai"
            ),
            Some(("linkedin", "4012345678".into()))
        );
        assert_eq!(
            platform_id("https://boards.greenhouse.io/companya/jobs/5512345?gh_src=abc"),
            Some(("greenhouse", "5512345".into()))
        );
        assert_eq!(
            platform_id("https://jobs.lever.co/companya/8d1c2f3a-1111-2222-3333-444455556666"),
            Some(("lever", "8d1c2f3a-1111-2222-3333-444455556666".into()))
        );
        assert_eq!(
            platform_id("https://at.indeed.com/viewjob?jk=abc123def"),
            Some(("indeed", "abc123def".into()))
        );
        assert_eq!(
            source_name("https://boards.greenhouse.io/x/jobs/1"),
            Some("Greenhouse".into())
        );
        assert!(is_job_specific(
            "https://company.com/careers/ai-engineer-12345"
        ));
        assert!(!is_job_specific("https://company.com/careers"));
        assert!(is_job_specific("https://company.com/jobs/ai-engineer"));
        assert!(!is_job_specific("https://company.com/jobs"));
        assert!(!is_job_specific("https://company.com/about-us"));
        assert_eq!(web_url("javascript:alert(1)"), None);
        assert_eq!(web_url("https://user:pw@example.com"), None);
    }
}
