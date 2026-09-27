//! Permitted relationships (NC §9, §22, §23, §41, §57): the user's own
//! first-degree connections, only from LinkedIn's Connections API and only
//! when LinkedIn granted ReMa that permission. ReMa matches them to
//! companies itself; the data is shown for this session only, never sent
//! to a model, never stored. There is no second-degree lookup: LinkedIn's
//! API does not offer one, and none is simulated.

use std::time::Duration;

use serde_json::Value;

use super::{
    capabilities::{self, RelationshipAccess},
    model::{Company, Connection, Person, Relationship},
    policy::{self, DataClass, DataSource, Operation, Persistence, Purpose},
    resolve,
};
use crate::{
    connectors::tokens,
    error::{AppError, AppResult},
    models::connectors::ProviderId,
    state::AppState,
    time::now_ms,
};

/// Most pages of 50 connections read (bounded work).
const MAX_PAGES: u32 = 20;
const PAGE_SIZE: u32 = 50;

/// One connection as LinkedIn returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub headline: Option<String>,
    pub profile_url: Option<String>,
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Members in one page of `GET /v2/connections?q=viewer` decorated with
/// the connections' names and headlines.
pub fn members(page: &Value) -> Vec<Member> {
    page.get("elements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|element| {
            let person = element.get("to~")?;
            let first = text(person, "localizedFirstName").unwrap_or_default();
            let last = text(person, "localizedLastName").unwrap_or_default();
            let name = format!("{first} {last}").trim().to_string();
            (!name.is_empty()).then(|| Member {
                name,
                headline: text(person, "localizedHeadline"),
                profile_url: text(person, "vanityName")
                    .filter(|v| {
                        v.chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    })
                    .map(|v| format!("https://www.linkedin.com/in/{v}")),
            })
        })
        .collect()
}

/// The user's first-degree connections (session memory first).
pub async fn connections(state: &AppState) -> AppResult<Vec<Member>> {
    policy::require(
        DataSource::LinkedinApi,
        DataClass::FirstDegreeConnection,
        Purpose::ProfessionalResearch,
        Operation::Fetch,
    )?;
    let providers = capabilities::all(state).await?;
    if let RelationshipAccess::Unavailable(reason) = capabilities::relationship_access(&providers) {
        return Err(AppError::permission(reason));
    }
    if let Some(members) = state.network.cached_connections() {
        return Ok(members);
    }
    let token = tokens::get_valid_access_token(state, ProviderId::Linkedin).await?;
    let api = state
        .connectors
        .linkedin
        .api
        .trim_end_matches('/')
        .to_string();
    let mut all = Vec::new();
    for page in 0..MAX_PAGES {
        let url = format!(
            "{api}/v2/connections?q=viewer&start={}&count={PAGE_SIZE}&projection=(elements*(to~(localizedFirstName,localizedLastName,localizedHeadline,vanityName)),paging)",
            page * PAGE_SIZE
        );
        let response = state
            .connectors
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("X-Restli-Protocol-Version", "2.0.0")
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| AppError::network("LinkedIn could not be reached"))?;
        match response.status().as_u16() {
            200 => {}
            401 => {
                tokens::mark_reauth_required(
                    state,
                    ProviderId::Linkedin,
                    "LinkedIn rejected the access token",
                )
                .await?;
                return Err(AppError::authentication(
                    "LinkedIn access has expired. Reconnect LinkedIn to check your connections.",
                ));
            }
            403 => {
                return Err(AppError::permission(
                    "LinkedIn refused access to your connection list: ReMa's LinkedIn app does \
                     not have that permission.",
                ))
            }
            429 => {
                return Err(AppError::provider(
                    "LinkedIn is limiting requests right now; try again later.",
                ))
            }
            code => {
                return Err(AppError::provider(format!(
                    "LinkedIn could not return your connections ({code})."
                )))
            }
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| AppError::provider("LinkedIn sent an unreadable answer."))?;
        let found = members(&body);
        let total = body
            .pointer("/paging/total")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let fetched = found.len();
        all.extend(found);
        if fetched < PAGE_SIZE as usize || all.len() as u64 >= total {
            break;
        }
    }
    state.network.remember_connections(all.clone());
    Ok(all)
}

/// Whether a headline names the company ("… at Nordlicht AI").
pub fn works_at(headline: &str, company: &Company) -> bool {
    let words = resolve::company_key(headline);
    let padded = format!(" {words} ");
    std::iter::once(&company.name)
        .chain(company.aliases.iter())
        .map(|n| resolve::company_key(n))
        .filter(|key| key.len() >= 3)
        .any(|key| padded.contains(&format!(" {key} ")))
}

/// Connections at the companies (session only).
pub fn match_companies(members: &[Member], companies: &[Company]) -> Vec<Connection> {
    let now = now_ms();
    let mut out = Vec::new();
    for member in members {
        let Some(headline) = &member.headline else {
            continue;
        };
        if let Some(company) = companies.iter().find(|c| works_at(headline, c)) {
            out.push(Connection {
                name: member.name.clone(),
                headline: Some(headline.clone()),
                company_id: Some(company.id.clone()),
                company_name: Some(company.name.clone()),
                profile_url: member.profile_url.clone(),
                relationship: relationship(now),
                match_reason: format!(
                    "Their LinkedIn headline names {} (as they describe it; LinkedIn does not \
                     confirm the employer)",
                    company.name
                ),
            });
        }
    }
    out
}

fn relationship(now: i64) -> Relationship {
    Relationship {
        provider: ProviderId::Linkedin,
        degree: 1,
        label: "LinkedIn first-degree connection".into(),
        fetched_at: now,
        persistence: Persistence::Session,
    }
}

/// Marks researched people who are the user's connections: the same name
/// and a headline naming the same company (never a name alone).
pub fn mark_people(people: &mut [Person], connections: &[Connection]) {
    for person in people.iter_mut() {
        let key = resolve::name_key(&person.name);
        if let Some(c) = connections
            .iter()
            .find(|c| resolve::name_key(&c.name) == key && c.company_id == person.company_id)
        {
            person.relationship = Some(c.relationship.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::companies;

    #[test]
    fn reads_decorated_connections_and_matches_companies_by_headline() {
        let page = serde_json::json!({
            "elements": [
                { "to": "urn:li:person:a", "to~": {
                    "localizedFirstName": "Jane", "localizedLastName": "Example",
                    "localizedHeadline": "Senior ML Engineer at Nordlicht AI",
                    "vanityName": "jane-example" } },
                { "to": "urn:li:person:b", "to~": {
                    "localizedFirstName": "Max", "localizedLastName": "Muster",
                    "localizedHeadline": "Product at Nordlichter Bank" } },
                { "to": "urn:li:person:c" }
            ],
            "paging": { "count": 50, "start": 0, "total": 3 }
        });
        let found = members(&page);
        assert_eq!(found.len(), 2, "undecorated entries carry no name");
        assert_eq!(
            found[0].profile_url.as_deref(),
            Some("https://www.linkedin.com/in/jane-example")
        );
        let company = companies::named("Nordlicht AI", 1);
        let matched = match_companies(&found, std::slice::from_ref(&company));
        assert_eq!(matched.len(), 1, "a similar name is another company");
        assert_eq!(matched[0].name, "Jane Example");
        assert_eq!(matched[0].relationship.degree, 1);
        assert_eq!(matched[0].relationship.persistence, Persistence::Session);
    }

    #[tokio::test]
    async fn without_the_permission_nothing_is_fetched_and_the_reason_is_exact() {
        let (state, _) = crate::state::testing::state(std::sync::Arc::new(
            crate::llm::fake::FakeLanguageModel::replying(&[]),
        ));
        let error = connections(&state).await.unwrap_err();
        assert!(matches!(error, AppError::Permission(_)));
        assert!(
            error.to_string().contains("cannot tell whom you know"),
            "{error}"
        );
        assert!(!error.to_string().contains("No connections"));
    }
}
