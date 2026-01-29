use crate::graph::Entity;
use crate::graph::MemoryEntityLink;
use crate::graph::RelationType;
use crate::graph::Relationship;
use crate::Result;
use sqlx::Row;
use sqlx::SqlitePool;
use uuid::Uuid;

/// Insert or update an entity, returning the entity ID
/// If an entity with the same name already exists, returns the existing ID
pub async fn upsert_entity(pool: &SqlitePool, entity: &Entity) -> Result<Uuid> {
    // Check if entity with same name exists
    let existing: Option<String> = sqlx::query_scalar(
        r#"
        SELECT id FROM entities WHERE name = ?
        "#,
    )
    .bind(&entity.name)
    .fetch_optional(pool)
    .await?;

    if let Some(id_str) = existing {
        // Update existing entity
        sqlx::query(
            r#"
            UPDATE entities 
            SET type = ?, metadata = ?
            WHERE id = ?
            "#,
        )
        .bind(&entity.entity_type)
        .bind(entity.metadata.to_string())
        .bind(&id_str)
        .execute(pool)
        .await?;

        Ok(parse_uuid("entity id", &id_str)?)
    } else {
        // Insert new entity
        sqlx::query(
            r#"
            INSERT INTO entities (id, name, type, metadata)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(entity.id.to_string())
        .bind(&entity.name)
        .bind(&entity.entity_type)
        .bind(entity.metadata.to_string())
        .execute(pool)
        .await?;

        Ok(entity.id)
    }
}

/// Get an entity by ID
pub async fn get_entity(pool: &SqlitePool, id: Uuid) -> Result<Option<Entity>> {
    let row = sqlx::query(
        r#"
        SELECT id, name, type, metadata FROM entities WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        Ok(Some(entity_from_row(&row)?))
    } else {
        Ok(None)
    }
}

/// Get an entity by name
pub async fn get_entity_by_name(pool: &SqlitePool, name: &str) -> Result<Option<Entity>> {
    let row = sqlx::query(
        r#"
        SELECT id, name, type, metadata FROM entities WHERE name = ?
        "#,
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        Ok(Some(entity_from_row(&row)?))
    } else {
        Ok(None)
    }
}

/// List all entities, optionally filtered by type
pub async fn list_entities(
    pool: &SqlitePool,
    entity_type: Option<&str>,
    limit: i64,
) -> Result<Vec<Entity>> {
    let rows = if let Some(et) = entity_type {
        sqlx::query(
            r#"
            SELECT id, name, type, metadata FROM entities WHERE type = ? LIMIT ?
            "#,
        )
        .bind(et)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            r#"
            SELECT id, name, type, metadata FROM entities LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    let mut entities = Vec::new();
    for row in rows {
        entities.push(entity_from_row(&row)?);
    }

    Ok(entities)
}

/// Delete an entity and all its relationships
pub async fn delete_entity(pool: &SqlitePool, id: Uuid) -> Result<bool> {
    let id_str = id.to_string();

    // Delete relationships involving this entity
    sqlx::query(
        r#"
        DELETE FROM relationships WHERE from_entity = ? OR to_entity = ?
        "#,
    )
    .bind(&id_str)
    .bind(&id_str)
    .execute(pool)
    .await?;

    // Delete memory-entity links
    sqlx::query(
        r#"
        DELETE FROM memory_entities WHERE entity_id = ?
        "#,
    )
    .bind(&id_str)
    .execute(pool)
    .await?;

    // Delete the entity
    let result = sqlx::query(
        r#"
        DELETE FROM entities WHERE id = ?
        "#,
    )
    .bind(&id_str)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Link a memory to an entity
pub async fn link_memory_entity(pool: &SqlitePool, link: &MemoryEntityLink) -> Result<()> {
    sqlx::query(
        r#"
        INSERT OR REPLACE INTO memory_entities (memory_id, entity_id)
        VALUES (?, ?)
        "#,
    )
    .bind(link.memory_id.to_string())
    .bind(link.entity_id.to_string())
    .execute(pool)
    .await?;

    Ok(())
}

/// Get all entities linked to a memory
pub async fn get_memory_entities(pool: &SqlitePool, memory_id: Uuid) -> Result<Vec<Entity>> {
    let rows = sqlx::query(
        r#"
        SELECT e.id, e.name, e.type, e.metadata
        FROM entities e
        JOIN memory_entities me ON e.id = me.entity_id
        WHERE me.memory_id = ?
        "#,
    )
    .bind(memory_id.to_string())
    .fetch_all(pool)
    .await?;

    let mut entities = Vec::new();
    for row in rows {
        entities.push(entity_from_row(&row)?);
    }

    Ok(entities)
}

/// Get all memories linked to an entity
pub async fn get_entity_memories(pool: &SqlitePool, entity_id: Uuid) -> Result<Vec<Uuid>> {
    let rows: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT memory_id FROM memory_entities WHERE entity_id = ?
        "#,
    )
    .bind(entity_id.to_string())
    .fetch_all(pool)
    .await?;

    let memory_ids: Vec<Uuid> = rows
        .into_iter()
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect();

    Ok(memory_ids)
}

/// Create or update a relationship between entities
pub async fn upsert_relationship(pool: &SqlitePool, relationship: &Relationship) -> Result<Uuid> {
    // Check if relationship already exists
    let existing: Option<(String, f32)> = sqlx::query_as(
        r#"
        SELECT id, strength FROM relationships 
        WHERE from_entity = ? AND to_entity = ? AND relation_type = ?
        "#,
    )
    .bind(relationship.from_entity_id.to_string())
    .bind(relationship.to_entity_id.to_string())
    .bind(relationship.relation_type.as_str())
    .fetch_optional(pool)
    .await?;

    if let Some((id_str, existing_strength)) = existing {
        // Update strength (increment)
        let new_strength = (existing_strength + relationship.strength).min(1.0);
        sqlx::query(
            r#"
            UPDATE relationships SET strength = ? WHERE id = ?
            "#,
        )
        .bind(new_strength)
        .bind(&id_str)
        .execute(pool)
        .await?;

        Ok(parse_uuid("relationship id", &id_str)?)
    } else {
        // Insert new relationship
        sqlx::query(
            r#"
            INSERT INTO relationships (id, from_entity, to_entity, relation_type, strength)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(relationship.id.to_string())
        .bind(relationship.from_entity_id.to_string())
        .bind(relationship.to_entity_id.to_string())
        .bind(relationship.relation_type.as_str())
        .bind(relationship.strength)
        .execute(pool)
        .await?;

        Ok(relationship.id)
    }
}

/// Get relationships for an entity
pub async fn get_entity_relationships(
    pool: &SqlitePool,
    entity_id: Uuid,
) -> Result<Vec<Relationship>> {
    let rows = sqlx::query(
        r#"
        SELECT id, from_entity, to_entity, relation_type, strength
        FROM relationships
        WHERE from_entity = ? OR to_entity = ?
        "#,
    )
    .bind(entity_id.to_string())
    .bind(entity_id.to_string())
    .fetch_all(pool)
    .await?;

    let mut relationships = Vec::new();
    for row in rows {
        relationships.push(relationship_from_row(&row)?);
    }

    Ok(relationships)
}

/// Get all relationships
pub async fn list_relationships(pool: &SqlitePool, limit: i64) -> Result<Vec<Relationship>> {
    let rows = sqlx::query(
        r#"
        SELECT id, from_entity, to_entity, relation_type, strength
        FROM relationships
        ORDER BY strength DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut relationships = Vec::new();
    for row in rows {
        relationships.push(relationship_from_row(&row)?);
    }

    Ok(relationships)
}

/// Get related entities (entities connected by relationships)
pub async fn get_related_entities(
    pool: &SqlitePool,
    entity_id: Uuid,
    min_strength: f32,
) -> Result<Vec<(Entity, RelationType, f32)>> {
    let rows = sqlx::query(
        r#"
        SELECT e.id, e.name, e.type, e.metadata, r.relation_type, r.strength
        FROM entities e
        JOIN relationships r ON (
            (r.from_entity = ? AND r.to_entity = e.id) OR
            (r.to_entity = ? AND r.from_entity = e.id)
        )
        WHERE r.strength >= ?
        ORDER BY r.strength DESC
        "#,
    )
    .bind(entity_id.to_string())
    .bind(entity_id.to_string())
    .bind(min_strength)
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();
    for row in rows {
        let entity = entity_from_row(&row)?;
        let relation_type_str: String = row.try_get("relation_type")?;
        let relation_type: RelationType =
            relation_type_str.parse().unwrap_or(RelationType::RelatedTo);
        let strength: f32 = row.try_get("strength")?;
        results.push((entity, relation_type, strength));
    }

    Ok(results)
}

/// Get entity statistics
pub async fn get_entity_stats(pool: &SqlitePool) -> Result<EntityStats> {
    let total_entities: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entities")
        .fetch_one(pool)
        .await?;

    let total_relationships: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM relationships")
        .fetch_one(pool)
        .await?;

    let total_links: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memory_entities")
        .fetch_one(pool)
        .await?;

    Ok(EntityStats {
        total_entities: total_entities as usize,
        total_relationships: total_relationships as usize,
        total_memory_links: total_links as usize,
    })
}

#[derive(Debug, Clone)]
pub struct EntityStats {
    pub total_entities: usize,
    pub total_relationships: usize,
    pub total_memory_links: usize,
}

fn parse_uuid(field: &str, value: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|e| crate::Error::InvalidInput(format!("Invalid {field} '{value}': {e}")))
}

fn entity_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Entity> {
    let id_str: String = row.try_get("id")?;
    let name: String = row.try_get("name")?;
    let type_str: Option<String> = row.try_get("type").ok();
    let metadata_str: Option<String> = row.try_get("metadata").ok();

    let entity_type = type_str.unwrap_or_else(|| "unknown".to_string());

    let metadata = metadata_str
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));

    Ok(Entity {
        id: parse_uuid("entity id", &id_str)?,
        name,
        entity_type,
        metadata,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    })
}

fn relationship_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Relationship> {
    let id_str: String = row.try_get("id")?;
    let from_str: String = row.try_get("from_entity")?;
    let to_str: String = row.try_get("to_entity")?;
    let relation_type_str: String = row.try_get("relation_type")?;
    let strength: f32 = row.try_get("strength")?;

    let relation_type: RelationType = relation_type_str.parse().unwrap_or(RelationType::RelatedTo);

    Ok(Relationship {
        id: parse_uuid("relationship id", &id_str)?,
        from_entity_id: parse_uuid("from_entity", &from_str)?,
        to_entity_id: parse_uuid("to_entity", &to_str)?,
        relation_type,
        strength,
        metadata: serde_json::Value::Object(serde_json::Map::new()),
        created_at: chrono::Utc::now(),
    })
}
