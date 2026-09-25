//! Provider configuration, provider models and preferences.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    error::AppResult,
    models::provider::{AuthMethod, ProviderKind},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRow {
    pub id: String,
    pub kind: ProviderKind,
    pub name: String,
    pub base_url: Option<String>,
    /// Configured model of an OpenAI-compatible endpoint.
    pub model: Option<String>,
    pub auth_method: AuthMethod,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRow {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub enabled: bool,
    pub max_output_tokens: Option<u32>,
    pub sort_order: i64,
}

const PROVIDER_COLUMNS: &str =
    "id, kind, name, base_url, model, auth_method, created_at, updated_at";
const MODEL_COLUMNS: &str =
    "provider_id, model_id, display_name, enabled, max_output_tokens, sort_order";

fn provider_from_row(row: &Row) -> rusqlite::Result<ProviderRow> {
    let kind: String = row.get(1)?;
    let auth: String = row.get(5)?;
    Ok(ProviderRow {
        id: row.get(0)?,
        kind: ProviderKind::parse(&kind).unwrap_or(ProviderKind::OpenaiCompatible),
        name: row.get(2)?,
        base_url: row.get(3)?,
        model: row.get(4)?,
        auth_method: AuthMethod::parse(&auth).unwrap_or(AuthMethod::None),
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn model_from_row(row: &Row) -> rusqlite::Result<ModelRow> {
    Ok(ModelRow {
        provider_id: row.get(0)?,
        model_id: row.get(1)?,
        display_name: row.get(2)?,
        enabled: row.get(3)?,
        max_output_tokens: row.get(4)?,
        sort_order: row.get(5)?,
    })
}

pub fn list(conn: &Connection) -> AppResult<Vec<ProviderRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {PROVIDER_COLUMNS} FROM providers ORDER BY created_at, id"
    ))?;
    let rows = stmt.query_map([], provider_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Option<ProviderRow>> {
    Ok(conn
        .query_row(
            &format!("SELECT {PROVIDER_COLUMNS} FROM providers WHERE id = ?1"),
            [id],
            provider_from_row,
        )
        .optional()?)
}

pub fn upsert(conn: &Connection, provider: &ProviderRow) -> AppResult<()> {
    conn.execute(
        "INSERT INTO providers (id, kind, name, base_url, model, auth_method, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (id) DO UPDATE SET
             name = excluded.name,
             base_url = excluded.base_url,
             model = excluded.model,
             auth_method = excluded.auth_method,
             updated_at = excluded.updated_at",
        params![
            provider.id,
            provider.kind.as_str(),
            provider.name,
            provider.base_url,
            provider.model,
            provider.auth_method.as_str(),
            provider.created_at,
            provider.updated_at,
        ],
    )?;
    Ok(())
}

/// Removes the provider and (by cascade) its models.
pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM providers WHERE id = ?1", [id])?;
    Ok(())
}

/// A new, unused id for an OpenAI-compatible endpoint (`custom-1`, `custom-2`, …).
pub fn next_custom_id(conn: &Connection) -> AppResult<String> {
    let max: Option<i64> = conn.query_row(
        "SELECT MAX(CAST(SUBSTR(id, 8) AS INTEGER)) FROM providers WHERE id LIKE 'custom-%'",
        [],
        |row| row.get(0),
    )?;
    Ok(format!("custom-{}", max.unwrap_or(0) + 1))
}

pub fn list_models(conn: &Connection, provider_id: &str) -> AppResult<Vec<ModelRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MODEL_COLUMNS} FROM provider_models WHERE provider_id = ?1
         ORDER BY sort_order, model_id"
    ))?;
    let rows = stmt.query_map([provider_id], model_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get_model(
    conn: &Connection,
    provider_id: &str,
    model_id: &str,
) -> AppResult<Option<ModelRow>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {MODEL_COLUMNS} FROM provider_models WHERE provider_id = ?1 AND model_id = ?2"
            ),
            [provider_id, model_id],
            model_from_row,
        )
        .optional()?)
}

/// Inserts or updates a model's metadata; keeps an existing `enabled` flag.
pub fn upsert_model(conn: &Connection, model: &ModelRow) -> AppResult<()> {
    conn.execute(
        "INSERT INTO provider_models
             (provider_id, model_id, display_name, enabled, max_output_tokens, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (provider_id, model_id) DO UPDATE SET
             display_name = excluded.display_name,
             max_output_tokens = excluded.max_output_tokens,
             sort_order = excluded.sort_order",
        params![
            model.provider_id,
            model.model_id,
            model.display_name,
            model.enabled,
            model.max_output_tokens,
            model.sort_order,
        ],
    )?;
    Ok(())
}

/// Deletes models of a provider that are not in `keep`.
pub fn retain_models(conn: &Connection, provider_id: &str, keep: &[String]) -> AppResult<()> {
    let existing = list_models(conn, provider_id)?;
    for model in existing.iter().filter(|m| !keep.contains(&m.model_id)) {
        conn.execute(
            "DELETE FROM provider_models WHERE provider_id = ?1 AND model_id = ?2",
            [provider_id, &model.model_id],
        )?;
    }
    Ok(())
}

pub fn set_model_enabled(
    conn: &Connection,
    provider_id: &str,
    model_id: &str,
    enabled: bool,
) -> AppResult<bool> {
    let changed = conn.execute(
        "UPDATE provider_models SET enabled = ?3 WHERE provider_id = ?1 AND model_id = ?2",
        params![provider_id, model_id, enabled],
    )?;
    Ok(changed > 0)
}

pub fn get_setting(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

pub fn delete_setting(conn: &Connection, key: &str) -> AppResult<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn provider(id: &str, kind: ProviderKind) -> ProviderRow {
        ProviderRow {
            id: id.into(),
            kind,
            name: kind.display_name().into(),
            base_url: None,
            model: None,
            auth_method: AuthMethod::ApiKey,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn model(provider_id: &str, model_id: &str, enabled: bool) -> ModelRow {
        ModelRow {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            display_name: model_id.to_uppercase(),
            enabled,
            max_output_tokens: Some(1000),
            sort_order: 0,
        }
    }

    #[test]
    fn upserts_providers_and_models() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            upsert(c, &provider("openai", ProviderKind::Openai))?;
            let mut renamed = provider("openai", ProviderKind::Openai);
            renamed.name = "OpenAI (work)".into();
            upsert(c, &renamed)?;
            assert_eq!(list(c)?.len(), 1);
            assert_eq!(get(c, "openai")?.unwrap().name, "OpenAI (work)");

            upsert_model(c, &model("openai", "a", true))?;
            upsert_model(c, &model("openai", "b", false))?;
            // Refreshing metadata keeps the user's enabled choice.
            upsert_model(c, &model("openai", "a", false))?;
            assert!(get_model(c, "openai", "a")?.unwrap().enabled);

            retain_models(c, "openai", &["b".to_string()])?;
            let models = list_models(c, "openai")?;
            assert_eq!(models.len(), 1);
            assert_eq!(models[0].model_id, "b");

            delete(c, "openai")?;
            assert!(list_models(c, "openai")?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn generates_sequential_custom_ids() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            assert_eq!(next_custom_id(c)?, "custom-1");
            upsert(c, &provider("custom-1", ProviderKind::OpenaiCompatible))?;
            upsert(c, &provider("custom-7", ProviderKind::OpenaiCompatible))?;
            assert_eq!(next_custom_id(c)?, "custom-8");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn stores_settings() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            assert_eq!(get_setting(c, "k")?, None);
            set_setting(c, "k", "1")?;
            set_setting(c, "k", "2")?;
            assert_eq!(get_setting(c, "k")?.as_deref(), Some("2"));
            delete_setting(c, "k")?;
            assert_eq!(get_setting(c, "k")?, None);
            Ok(())
        })
        .unwrap();
    }
}
