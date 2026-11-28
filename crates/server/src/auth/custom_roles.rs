//! Rôles personnalisés : lecture et écriture de la table `custom_roles`.
//!
//! Un rôle personnalisé est un nom, une description et un jeu de permissions
//! du catalogue ([`super::permissions`]). Un compte en reçoit un à la place
//! d'un rôle livré : son rôle livré reste `viewer`, et ses permissions sont
//! celles du rôle, relues à chaque requête — changer le rôle vaut tout de suite
//! pour tous ses comptes. Un rôle encore attribué ne se supprime pas (409) :
//! il faut d'abord donner un autre rôle à ses comptes.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use crate::auth::permissions::{self, Permission};

/// Longueurs maximales d'un nom et d'une description.
pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_DESCRIPTION_CHARS: usize = 500;
/// Au-delà, ce n'est plus une délégation, c'est un annuaire.
pub const MAX_ROLES: i64 = 100;

/// Noms réservés : ceux des rôles livrés, pour qu'une liste de comptes ne
/// puisse pas faire passer un rôle personnalisé pour l'un d'eux.
const RESERVED: &[&str] = &["admin", "operator", "viewer", "wall"];

/// Le rôle personnalisé d'un compte, tel qu'il est lu avec lui.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assigned {
    pub id: i64,
    pub name: String,
    /// Toujours en forme ([`permissions::normalize`]) : `view` compris.
    pub permissions: Vec<Permission>,
}

/// Un rôle, tel que l'API le montre.
#[derive(Debug, Clone, Serialize)]
pub struct CustomRole {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub permissions: Vec<Permission>,
    /// Comptes qui l'ont : un rôle attribué ne se supprime pas.
    pub users: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Ce qu'un administrateur envoie pour créer ou modifier un rôle.
#[derive(Debug, Clone, Deserialize)]
pub struct RoleInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Noms du catalogue. Un nom inconnu est refusé (400), pas ignoré : celui
    /// qui l'envoie croit donner un droit qui n'existe pas.
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// Une saisie vérifiée, prête à écrire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub name: String,
    pub description: String,
    pub permissions: Vec<Permission>,
}

impl RoleInput {
    /// Vérifie la saisie. Le message d'erreur est montré tel quel.
    pub fn check(&self) -> Result<Checked, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Give the role a name.".into());
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(format!("The name must be {MAX_NAME_CHARS} characters or fewer."));
        }
        if name.chars().any(char::is_control) {
            return Err("The name cannot contain control characters.".into());
        }
        if RESERVED.iter().any(|reserved| reserved.eq_ignore_ascii_case(name)) {
            return Err(format!("\"{name}\" is the name of a built-in role: pick another one."));
        }
        let description = self.description.trim();
        if description.chars().count() > MAX_DESCRIPTION_CHARS {
            return Err(format!(
                "The description must be {MAX_DESCRIPTION_CHARS} characters or fewer."
            ));
        }
        // Pas de préallocation : la taille vient du client.
        let mut chosen = Vec::new();
        for raw in &self.permissions {
            match Permission::parse(raw.trim()) {
                Some(permission) => chosen.push(permission),
                None => return Err(format!("\"{raw}\" is not a permission of the catalog.")),
            }
        }
        Ok(Checked {
            name: name.to_string(),
            description: description.to_string(),
            permissions: permissions::normalize(&chosen),
        })
    }
}

/// Le jeu de permissions tel qu'il est stocké : tableau JSON de noms.
pub fn stored(permissions: &[Permission]) -> String {
    let names: Vec<&str> = permissions.iter().map(|permission| permission.as_str()).collect();
    serde_json::to_string(&names).unwrap_or_else(|_| "[\"view\"]".to_string())
}

fn read(row: &SqliteRow) -> Result<CustomRole> {
    let raw: String = row.try_get("permissions")?;
    Ok(CustomRole {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        permissions: permissions::parse_stored(&raw),
        users: row.try_get("users")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

const SELECT: &str = "SELECT r.id, r.name, r.description, r.permissions, r.created_at,
            r.updated_at,
            (SELECT COUNT(*) FROM users u WHERE u.custom_role_id = r.id) AS users
     FROM custom_roles r";

pub async fn list(pool: &SqlitePool) -> Result<Vec<CustomRole>> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!("{SELECT} ORDER BY r.name COLLATE NOCASE")))
        .fetch_all(pool)
        .await
        .context("liste des rôles personnalisés")?;
    rows.iter().map(read).collect()
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<CustomRole>> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!("{SELECT} WHERE r.id = ?")))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("lecture d'un rôle personnalisé")?;
    row.as_ref().map(read).transpose()
}

pub async fn by_name(pool: &SqlitePool, name: &str) -> Result<Option<CustomRole>> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!("{SELECT} WHERE r.name = ? COLLATE NOCASE")))
        .bind(name.trim())
        .fetch_optional(pool)
        .await
        .context("recherche d'un rôle personnalisé")?;
    row.as_ref().map(read).transpose()
}

/// Pourquoi une écriture n'a pas eu lieu.
#[derive(Debug, PartialEq, Eq)]
pub enum WriteError {
    /// Un autre rôle porte déjà ce nom.
    NameTaken,
    /// Le plafond [`MAX_ROLES`] est atteint.
    TooMany,
    NotFound,
    /// Le rôle est encore attribué à ce nombre de comptes.
    InUse(i64),
}

/// Crée un rôle et renvoie son identifiant.
pub async fn create(pool: &SqlitePool, role: &Checked) -> Result<Result<i64, WriteError>> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM custom_roles")
        .fetch_one(pool)
        .await
        .context("comptage des rôles personnalisés")?;
    if count >= MAX_ROLES {
        return Ok(Err(WriteError::TooMany));
    }
    // Le conflit de nom est laissé à la contrainte d'unicité : c'est la seule
    // façon de fermer la fenêtre entre « vérifier » et « écrire ».
    let result = sqlx::query(
        "INSERT INTO custom_roles (name, description, permissions) VALUES (?, ?, ?)
         ON CONFLICT(name) DO NOTHING",
    )
    .bind(&role.name)
    .bind(&role.description)
    .bind(stored(&role.permissions))
    .execute(pool)
    .await
    .context("création d'un rôle personnalisé")?;
    if result.rows_affected() == 0 {
        return Ok(Err(WriteError::NameTaken));
    }
    Ok(Ok(result.last_insert_rowid()))
}

/// Remplace le nom, la description et les permissions d'un rôle.
pub async fn update(pool: &SqlitePool, id: i64, role: &Checked) -> Result<Result<(), WriteError>> {
    let other: Option<i64> =
        sqlx::query_scalar("SELECT id FROM custom_roles WHERE name = ? COLLATE NOCASE AND id <> ?")
            .bind(&role.name)
            .bind(id)
            .fetch_optional(pool)
            .await
            .context("recherche d'un homonyme")?;
    if other.is_some() {
        return Ok(Err(WriteError::NameTaken));
    }
    let result = sqlx::query(
        "UPDATE custom_roles SET name = ?, description = ?, permissions = ?,
                updated_at = strftime('%Y-%m-%d %H:%M:%S', 'now')
         WHERE id = ?",
    )
    .bind(&role.name)
    .bind(&role.description)
    .bind(stored(&role.permissions))
    .bind(id)
    .execute(pool)
    .await
    .context("mise à jour d'un rôle personnalisé")?;
    Ok(if result.rows_affected() == 1 { Ok(()) } else { Err(WriteError::NotFound) })
}

/// Supprime un rôle que plus aucun compte n'a. La condition est dans la
/// requête : une attribution faite entre-temps fait échouer la suppression.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<Result<(), WriteError>> {
    let result = sqlx::query(
        "DELETE FROM custom_roles
         WHERE id = ? AND NOT EXISTS (SELECT 1 FROM users WHERE custom_role_id = ?)",
    )
    .bind(id)
    .bind(id)
    .execute(pool)
    .await
    .context("suppression d'un rôle personnalisé")?;
    if result.rows_affected() == 1 {
        return Ok(Ok(()));
    }
    Ok(Err(match get(pool, id).await? {
        Some(role) => WriteError::InUse(role.users),
        None => WriteError::NotFound,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, permissions: &[&str]) -> RoleInput {
        RoleInput {
            name: name.into(),
            description: String::new(),
            permissions: permissions.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn a_role_needs_a_free_name_and_known_permissions() {
        let checked = input("  Monitor editors ", &["monitors.manage"]).check().unwrap();
        assert_eq!(checked.name, "Monitor editors");
        assert_eq!(checked.permissions, vec![Permission::View, Permission::MonitorsManage]);
        assert!(input("", &[]).check().is_err());
        assert!(input("Admin", &[]).check().is_err(), "a built-in name");
        assert!(input("wall", &[]).check().is_err());
        assert!(input(&"x".repeat(65), &[]).check().is_err());
        assert!(input("Editors", &["users.manage"]).check().is_err(), "not in the catalog");
        assert!(input("Editors", &["backup.restore"]).check().is_err());
        assert_eq!(input("Readers", &[]).check().unwrap().permissions, vec![Permission::View]);
    }

    #[test]
    fn the_stored_form_reads_back() {
        let set = vec![Permission::View, Permission::AlertsOperate, Permission::ReportsView];
        assert_eq!(stored(&set), r#"["view","alerts.operate","reports.view"]"#);
        assert_eq!(permissions::parse_stored(&stored(&set)), set);
    }
}
