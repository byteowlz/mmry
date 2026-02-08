//! Database operations for reasoning module
//!
//! All operations maintain full traceability:
//! - Inferences link to premises
//! - Answers link to evidence
//! - Events log all reasoning activity

use chrono::DateTime;
use chrono::Utc;
use sqlx::Row;
use sqlx::SqlitePool;
use uuid::Uuid;

use super::inference::Inference;
use super::inference::InferenceType;
use super::inference::ReasoningAnswer;
use super::service::Contradiction;

/// Helper to parse UUID with proper error handling
fn parse_uuid(s: &str) -> crate::Result<Uuid> {
    Uuid::parse_str(s).map_err(|e| crate::Error::InvalidInput(format!("Invalid UUID '{s}': {e}")))
}

/// Helper to parse datetime with proper error handling
fn parse_datetime_str(s: &str) -> crate::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| crate::Error::InvalidInput(format!("Invalid datetime '{s}': {e}")))
}

// ─────────────────────────────────────────────────────────────────────────────
// Inference Operations
// ─────────────────────────────────────────────────────────────────────────────

/// Insert a new inference with its premises
pub async fn insert_inference(pool: &SqlitePool, inference: &Inference) -> crate::Result<()> {
    let mut tx = pool.begin().await?;

    // Insert the inference
    sqlx::query(
        r#"
        INSERT INTO inferences (
            id, conclusion, inference_type, reasoning_trace, 
            certainty_statement, category, created_at, superseded, metadata
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(inference.id.to_string())
    .bind(&inference.conclusion)
    .bind(inference.inference_type.as_str())
    .bind(&inference.reasoning_trace)
    .bind(&inference.certainty_statement)
    .bind(&inference.category)
    .bind(inference.created_at.to_rfc3339())
    .bind(inference.superseded)
    .bind("{}")
    .execute(&mut *tx)
    .await?;

    // Insert premise links
    for (order, premise_id) in inference.premise_ids.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO inference_premises (id, inference_id, premise_type, premise_id, premise_order)
            VALUES (?, ?, 'fact', ?, ?)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(inference.id.to_string())
        .bind(premise_id.to_string())
        .bind(order as i32)
        .execute(&mut *tx)
        .await?;
    }

    // Log the event
    log_reasoning_event(
        &mut tx,
        "inference_derived",
        Some(inference.id),
        None,
        None,
        &format!(
            "Derived {} inference: {}",
            inference.inference_type.as_str(),
            truncate(&inference.conclusion, 100)
        ),
        serde_json::json!({
            "inference_type": inference.inference_type.as_str(),
            "premise_count": inference.premise_ids.len(),
        }),
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Get an inference by ID with its premises
pub async fn get_inference(pool: &SqlitePool, id: Uuid) -> crate::Result<Option<Inference>> {
    let row = sqlx::query(
        r#"
        SELECT id, conclusion, inference_type, reasoning_trace, certainty_statement,
               category, created_at, superseded, superseded_by
        FROM inferences
        WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    // Get premises
    let premise_rows = sqlx::query(
        r#"
        SELECT premise_id FROM inference_premises
        WHERE inference_id = ?
        ORDER BY premise_order
        "#,
    )
    .bind(id.to_string())
    .fetch_all(pool)
    .await?;

    let premise_ids: Vec<Uuid> = premise_rows
        .iter()
        .filter_map(|r| {
            r.try_get::<String, _>("premise_id")
                .ok()
                .and_then(|s| Uuid::parse_str(&s).ok())
        })
        .collect();

    Ok(Some(row_to_inference(&row, premise_ids)?))
}

/// List recent inferences
pub async fn list_inferences(
    pool: &SqlitePool,
    limit: i64,
    include_superseded: bool,
) -> crate::Result<Vec<Inference>> {
    let query = if include_superseded {
        r#"
        SELECT id, conclusion, inference_type, reasoning_trace, certainty_statement,
               category, created_at, superseded, superseded_by
        FROM inferences
        ORDER BY created_at DESC
        LIMIT ?
        "#
    } else {
        r#"
        SELECT id, conclusion, inference_type, reasoning_trace, certainty_statement,
               category, created_at, superseded, superseded_by
        FROM inferences
        WHERE superseded = FALSE
        ORDER BY created_at DESC
        LIMIT ?
        "#
    };

    let rows = sqlx::query(query).bind(limit).fetch_all(pool).await?;

    let mut inferences = Vec::new();
    for row in rows {
        let id: String = row.try_get("id")?;
        let id = parse_uuid(&id)?;

        // Get premises for each inference
        let premise_rows = sqlx::query(
            "SELECT premise_id FROM inference_premises WHERE inference_id = ? ORDER BY premise_order",
        )
        .bind(id.to_string())
        .fetch_all(pool)
        .await?;

        let premise_ids: Vec<Uuid> = premise_rows
            .iter()
            .filter_map(|r| {
                r.try_get::<String, _>("premise_id")
                    .ok()
                    .and_then(|s| Uuid::parse_str(&s).ok())
            })
            .collect();

        inferences.push(row_to_inference(&row, premise_ids)?);
    }

    Ok(inferences)
}

/// List inferences by type
pub async fn list_inferences_by_type(
    pool: &SqlitePool,
    inference_type: InferenceType,
    limit: i64,
) -> crate::Result<Vec<Inference>> {
    let rows = sqlx::query(
        r#"
        SELECT id, conclusion, inference_type, reasoning_trace, certainty_statement,
               category, created_at, superseded, superseded_by
        FROM inferences
        WHERE inference_type = ? AND superseded = FALSE
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(inference_type.as_str())
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut inferences = Vec::new();
    for row in rows {
        let id: String = row.try_get("id")?;
        let id = parse_uuid(&id)?;

        let premise_rows = sqlx::query(
            "SELECT premise_id FROM inference_premises WHERE inference_id = ? ORDER BY premise_order",
        )
        .bind(id.to_string())
        .fetch_all(pool)
        .await?;

        let premise_ids: Vec<Uuid> = premise_rows
            .iter()
            .filter_map(|r| {
                r.try_get::<String, _>("premise_id")
                    .ok()
                    .and_then(|s| Uuid::parse_str(&s).ok())
            })
            .collect();

        inferences.push(row_to_inference(&row, premise_ids)?);
    }

    Ok(inferences)
}

/// Mark an inference as superseded
pub async fn supersede_inference(
    pool: &SqlitePool,
    old_id: Uuid,
    new_id: Uuid,
) -> crate::Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        UPDATE inferences
        SET superseded = TRUE, superseded_by = ?, superseded_at = ?
        WHERE id = ?
        "#,
    )
    .bind(new_id.to_string())
    .bind(Utc::now().to_rfc3339())
    .bind(old_id.to_string())
    .execute(&mut *tx)
    .await?;

    log_reasoning_event(
        &mut tx,
        "inference_superseded",
        Some(old_id),
        None,
        None,
        &format!("Inference {} superseded by {}", old_id, new_id),
        serde_json::json!({
            "old_id": old_id.to_string(),
            "new_id": new_id.to_string(),
        }),
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Get the full reasoning chain for an inference (all premises recursively)
pub async fn get_reasoning_chain(
    pool: &SqlitePool,
    inference_id: Uuid,
) -> crate::Result<ReasoningChain> {
    let inference = get_inference(pool, inference_id)
        .await?
        .ok_or_else(|| crate::Error::NotFound(format!("Inference {inference_id} not found")))?;

    let mut chain = ReasoningChain {
        inference,
        fact_premises: Vec::new(),
        inference_premises: Vec::new(),
    };

    // Get all premises with their types
    let premise_rows = sqlx::query(
        r#"
        SELECT premise_type, premise_id FROM inference_premises
        WHERE inference_id = ?
        ORDER BY premise_order
        "#,
    )
    .bind(inference_id.to_string())
    .fetch_all(pool)
    .await?;

    for row in premise_rows {
        let premise_type: String = row.try_get("premise_type")?;
        let premise_id: String = row.try_get("premise_id")?;
        let premise_id = parse_uuid(&premise_id)?;

        match premise_type.as_str() {
            "fact" => {
                if let Ok(Some(fact)) =
                    crate::database::operations::get_fact(pool, premise_id).await
                {
                    chain.fact_premises.push(fact);
                }
            }
            "inference" => {
                // Recursively get inference chain
                if let Ok(sub_chain) = Box::pin(get_reasoning_chain(pool, premise_id)).await {
                    chain.inference_premises.push(sub_chain);
                }
            }
            _ => {}
        }
    }

    Ok(chain)
}

// ─────────────────────────────────────────────────────────────────────────────
// Contradiction Operations
// ─────────────────────────────────────────────────────────────────────────────

/// Insert a detected contradiction
pub async fn insert_contradiction(
    pool: &SqlitePool,
    contradiction: &Contradiction,
) -> crate::Result<Uuid> {
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        INSERT INTO contradictions (
            id, item_a_type, item_a_id, item_b_type, item_b_id,
            explanation, status, created_at
        ) VALUES (?, 'fact', ?, 'fact', ?, ?, 'detected', ?)
        "#,
    )
    .bind(id.to_string())
    .bind(contradiction.fact_a_id.to_string())
    .bind(contradiction.fact_b_id.to_string())
    .bind(&contradiction.explanation)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *tx)
    .await?;

    log_reasoning_event(
        &mut tx,
        "contradiction_detected",
        None,
        Some(id),
        None,
        &format!(
            "Contradiction detected: {}",
            truncate(&contradiction.explanation, 100)
        ),
        serde_json::json!({
            "fact_a": contradiction.fact_a_id.to_string(),
            "fact_b": contradiction.fact_b_id.to_string(),
        }),
    )
    .await?;

    tx.commit().await?;
    Ok(id)
}

/// List unresolved contradictions
pub async fn list_unresolved_contradictions(
    pool: &SqlitePool,
    limit: i64,
) -> crate::Result<Vec<ContradictionRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT id, item_a_type, item_a_id, item_b_type, item_b_id,
               explanation, status, resolution_type, resolution_reasoning, created_at
        FROM contradictions
        WHERE status = 'detected'
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.iter().map(row_to_contradiction).collect()
}

/// Resolve a contradiction
pub async fn resolve_contradiction(
    pool: &SqlitePool,
    id: Uuid,
    resolution_type: &str,
    reasoning: &str,
) -> crate::Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        UPDATE contradictions
        SET status = 'resolved', resolution_type = ?, resolution_reasoning = ?, resolved_at = ?
        WHERE id = ?
        "#,
    )
    .bind(resolution_type)
    .bind(reasoning)
    .bind(Utc::now().to_rfc3339())
    .bind(id.to_string())
    .execute(&mut *tx)
    .await?;

    log_reasoning_event(
        &mut tx,
        "contradiction_resolved",
        None,
        Some(id),
        None,
        &format!("Contradiction resolved via {resolution_type}"),
        serde_json::json!({
            "resolution_type": resolution_type,
            "reasoning": reasoning,
        }),
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Answer Operations
// ─────────────────────────────────────────────────────────────────────────────

/// Insert a reasoning answer with its evidence
pub async fn insert_answer(
    pool: &SqlitePool,
    answer: &ReasoningAnswer,
    cache_seconds: u64,
) -> crate::Result<Uuid> {
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await?;

    let expires_at = if cache_seconds > 0 {
        Some(Utc::now() + chrono::Duration::seconds(cache_seconds as i64))
    } else {
        None
    };

    let question_hash = hash_question(&answer.question);

    sqlx::query(
        r#"
        INSERT INTO reasoning_answers (
            id, question, question_hash, answer, reasoning_trace,
            certainty_statement, created_at, expires_at,
            facts_considered, inferences_considered
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(id.to_string())
    .bind(&answer.question)
    .bind(&question_hash)
    .bind(&answer.answer)
    .bind(&answer.reasoning_trace)
    .bind(&answer.certainty_statement)
    .bind(answer.generated_at.to_rfc3339())
    .bind(expires_at.map(|t| t.to_rfc3339()))
    .bind(answer.supporting_facts.len() as i32)
    .bind(answer.supporting_inferences.len() as i32)
    .execute(&mut *tx)
    .await?;

    // Insert fact evidence links
    for fact_id in &answer.supporting_facts {
        sqlx::query(
            r#"
            INSERT INTO answer_evidence (id, answer_id, evidence_type, evidence_id)
            VALUES (?, ?, 'fact', ?)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(id.to_string())
        .bind(fact_id.to_string())
        .execute(&mut *tx)
        .await?;
    }

    // Insert inference evidence links
    for inf_id in &answer.supporting_inferences {
        sqlx::query(
            r#"
            INSERT INTO answer_evidence (id, answer_id, evidence_type, evidence_id)
            VALUES (?, ?, 'inference', ?)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(id.to_string())
        .bind(inf_id.to_string())
        .execute(&mut *tx)
        .await?;
    }

    log_reasoning_event(
        &mut tx,
        "question_answered",
        None,
        None,
        Some(id),
        &format!("Answered: {}", truncate(&answer.question, 80)),
        serde_json::json!({
            "facts_used": answer.supporting_facts.len(),
            "inferences_used": answer.supporting_inferences.len(),
        }),
    )
    .await?;

    tx.commit().await?;
    Ok(id)
}

/// Get cached answer by question hash
pub async fn get_cached_answer(
    pool: &SqlitePool,
    question: &str,
) -> crate::Result<Option<ReasoningAnswer>> {
    let hash = hash_question(question);
    let now = Utc::now().to_rfc3339();

    let row = sqlx::query(
        r#"
        SELECT id, question, answer, reasoning_trace, certainty_statement, created_at
        FROM reasoning_answers
        WHERE question_hash = ? AND (expires_at IS NULL OR expires_at > ?)
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(&hash)
    .bind(&now)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let id: String = row.try_get("id")?;
    let id = parse_uuid(&id)?;

    // Get supporting evidence
    let evidence_rows =
        sqlx::query("SELECT evidence_type, evidence_id FROM answer_evidence WHERE answer_id = ?")
            .bind(id.to_string())
            .fetch_all(pool)
            .await?;

    let mut fact_ids = Vec::new();
    let mut inference_ids = Vec::new();

    for ev in evidence_rows {
        let ev_type: String = ev.try_get("evidence_type")?;
        let ev_id: String = ev.try_get("evidence_id")?;
        if let Ok(uuid) = Uuid::parse_str(&ev_id) {
            match ev_type.as_str() {
                "fact" => fact_ids.push(uuid),
                "inference" => inference_ids.push(uuid),
                _ => {}
            }
        }
    }

    Ok(Some(row_to_answer(&row, fact_ids, inference_ids)?))
}

/// Get an answer with full evidence details
pub async fn get_answer_with_evidence(
    pool: &SqlitePool,
    answer_id: Uuid,
) -> crate::Result<Option<AnswerWithEvidence>> {
    let row = sqlx::query(
        r#"
        SELECT id, question, answer, reasoning_trace, certainty_statement, created_at
        FROM reasoning_answers
        WHERE id = ?
        "#,
    )
    .bind(answer_id.to_string())
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    // Get all evidence
    let evidence_rows =
        sqlx::query("SELECT evidence_type, evidence_id FROM answer_evidence WHERE answer_id = ?")
            .bind(answer_id.to_string())
            .fetch_all(pool)
            .await?;

    let mut facts = Vec::new();
    let mut inferences = Vec::new();

    for ev in evidence_rows {
        let ev_type: String = ev.try_get("evidence_type")?;
        let ev_id: String = ev.try_get("evidence_id")?;
        let ev_uuid = parse_uuid(&ev_id)?;

        match ev_type.as_str() {
            "fact" => {
                if let Ok(Some(fact)) = crate::database::operations::get_fact(pool, ev_uuid).await {
                    facts.push(fact);
                }
            }
            "inference" => {
                if let Ok(Some(inf)) = get_inference(pool, ev_uuid).await {
                    inferences.push(inf);
                }
            }
            _ => {}
        }
    }

    let answer = row_to_answer(
        &row,
        facts.iter().map(|f| f.id).collect(),
        inferences.iter().map(|i| i.id).collect(),
    )?;

    Ok(Some(AnswerWithEvidence {
        answer,
        facts,
        inferences,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Event Operations
// ─────────────────────────────────────────────────────────────────────────────

/// Log a reasoning event
async fn log_reasoning_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event_type: &str,
    inference_id: Option<Uuid>,
    contradiction_id: Option<Uuid>,
    answer_id: Option<Uuid>,
    description: &str,
    details: serde_json::Value,
) -> crate::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO reasoning_events (
            id, event_type, inference_id, contradiction_id, answer_id,
            description, details, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(event_type)
    .bind(inference_id.map(|id| id.to_string()))
    .bind(contradiction_id.map(|id| id.to_string()))
    .bind(answer_id.map(|id| id.to_string()))
    .bind(description)
    .bind(details.to_string())
    .bind(Utc::now().to_rfc3339())
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// List recent reasoning events
pub async fn list_reasoning_events(
    pool: &SqlitePool,
    limit: i64,
) -> crate::Result<Vec<ReasoningEvent>> {
    let rows = sqlx::query(
        r#"
        SELECT id, event_type, inference_id, contradiction_id, answer_id,
               description, details, created_at
        FROM reasoning_events
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.iter().map(row_to_event).collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper Types and Functions
// ─────────────────────────────────────────────────────────────────────────────

/// Full reasoning chain for an inference
#[derive(Debug, Clone)]
pub struct ReasoningChain {
    pub inference: Inference,
    pub fact_premises: Vec<crate::agents::FactRecord>,
    pub inference_premises: Vec<ReasoningChain>,
}

/// Contradiction record from database
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContradictionRecord {
    pub id: Uuid,
    pub item_a_type: String,
    pub item_a_id: Uuid,
    pub item_b_type: String,
    pub item_b_id: Uuid,
    pub explanation: String,
    pub status: String,
    pub resolution_type: Option<String>,
    pub resolution_reasoning: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Answer with full evidence loaded
#[derive(Debug, Clone)]
pub struct AnswerWithEvidence {
    pub answer: ReasoningAnswer,
    pub facts: Vec<crate::agents::FactRecord>,
    pub inferences: Vec<Inference>,
}

/// A reasoning event from the audit log
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReasoningEvent {
    pub id: Uuid,
    pub event_type: String,
    pub inference_id: Option<Uuid>,
    pub contradiction_id: Option<Uuid>,
    pub answer_id: Option<Uuid>,
    pub description: String,
    pub details: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

fn row_to_inference(
    row: &sqlx::sqlite::SqliteRow,
    premise_ids: Vec<Uuid>,
) -> crate::Result<Inference> {
    let id: String = row.try_get("id")?;
    let conclusion: String = row.try_get("conclusion")?;
    let inference_type: String = row.try_get("inference_type")?;
    let reasoning_trace: String = row.try_get("reasoning_trace")?;
    let certainty_statement: Option<String> = row.try_get("certainty_statement").ok();
    let category: Option<String> = row.try_get("category").ok();
    let created_at: String = row.try_get("created_at")?;
    let superseded: bool = row.try_get("superseded").unwrap_or(false);
    let superseded_by: Option<String> = row.try_get("superseded_by").ok().flatten();

    Ok(Inference {
        id: parse_uuid(&id)?,
        conclusion,
        inference_type: inference_type.parse().unwrap_or(InferenceType::Observed),
        premise_ids,
        reasoning_trace,
        certainty_statement,
        created_at: parse_datetime_str(&created_at)?,
        category,
        superseded,
        superseded_by: superseded_by.and_then(|s| Uuid::parse_str(&s).ok()),
    })
}

fn row_to_answer(
    row: &sqlx::sqlite::SqliteRow,
    fact_ids: Vec<Uuid>,
    inference_ids: Vec<Uuid>,
) -> crate::Result<ReasoningAnswer> {
    let question: String = row.try_get("question")?;
    let answer: String = row.try_get("answer")?;
    let reasoning_trace: String = row.try_get("reasoning_trace")?;
    let certainty_statement: String = row.try_get("certainty_statement")?;
    let created_at: String = row.try_get("created_at")?;

    Ok(ReasoningAnswer {
        question,
        answer,
        supporting_facts: fact_ids,
        supporting_inferences: inference_ids,
        reasoning_trace,
        certainty_statement,
        generated_at: parse_datetime_str(&created_at)?,
    })
}

fn row_to_contradiction(row: &sqlx::sqlite::SqliteRow) -> crate::Result<ContradictionRecord> {
    let id: String = row.try_get("id")?;
    let item_a_type: String = row.try_get("item_a_type")?;
    let item_a_id: String = row.try_get("item_a_id")?;
    let item_b_type: String = row.try_get("item_b_type")?;
    let item_b_id: String = row.try_get("item_b_id")?;
    let explanation: String = row.try_get("explanation")?;
    let status: String = row.try_get("status")?;
    let resolution_type: Option<String> = row.try_get("resolution_type").ok().flatten();
    let resolution_reasoning: Option<String> = row.try_get("resolution_reasoning").ok().flatten();
    let created_at: String = row.try_get("created_at")?;

    Ok(ContradictionRecord {
        id: parse_uuid(&id)?,
        item_a_type,
        item_a_id: parse_uuid(&item_a_id)?,
        item_b_type,
        item_b_id: parse_uuid(&item_b_id)?,
        explanation,
        status,
        resolution_type,
        resolution_reasoning,
        created_at: parse_datetime_str(&created_at)?,
    })
}

fn row_to_event(row: &sqlx::sqlite::SqliteRow) -> crate::Result<ReasoningEvent> {
    let id: String = row.try_get("id")?;
    let event_type: String = row.try_get("event_type")?;
    let inference_id: Option<String> = row.try_get("inference_id").ok().flatten();
    let contradiction_id: Option<String> = row.try_get("contradiction_id").ok().flatten();
    let answer_id: Option<String> = row.try_get("answer_id").ok().flatten();
    let description: String = row.try_get("description")?;
    let details: String = row.try_get("details").unwrap_or_else(|_| "{}".to_string());
    let created_at: String = row.try_get("created_at")?;

    Ok(ReasoningEvent {
        id: parse_uuid(&id)?,
        event_type,
        inference_id: inference_id.and_then(|s| Uuid::parse_str(&s).ok()),
        contradiction_id: contradiction_id.and_then(|s| Uuid::parse_str(&s).ok()),
        answer_id: answer_id.and_then(|s| Uuid::parse_str(&s).ok()),
        description,
        details: serde_json::from_str(&details).unwrap_or(serde_json::json!({})),
        created_at: parse_datetime_str(&created_at)?,
    })
}

fn hash_question(question: &str) -> String {
    use std::hash::Hash;
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    question.to_lowercase().trim().hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
