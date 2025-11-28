use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::ner::EntityType;

/// An entity in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Unique identifier
    pub id: Uuid,
    /// Canonical name of the entity
    pub name: String,
    /// Entity type (PER, LOC, ORG, MISC)
    pub entity_type: EntityType,
    /// Additional metadata (e.g., aliases, descriptions)
    pub metadata: serde_json::Value,
    /// When the entity was first seen
    pub created_at: DateTime<Utc>,
    /// When the entity was last updated
    pub updated_at: DateTime<Utc>,
}

impl Entity {
    pub fn new(name: String, entity_type: EntityType) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            entity_type,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            created_at: now,
            updated_at: now,
        }
    }

    /// Create entity with custom metadata
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Relationship types between entities
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    /// Entities co-occur in the same memory
    CoOccurs,
    /// One entity is part of another (e.g., person works at organization)
    PartOf,
    /// Entities are related by location
    LocatedIn,
    /// Generic association
    RelatedTo,
    /// Custom/user-defined relationship
    Custom,
}

impl RelationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CoOccurs => "co_occurs",
            Self::PartOf => "part_of",
            Self::LocatedIn => "located_in",
            Self::RelatedTo => "related_to",
            Self::Custom => "custom",
        }
    }
}

impl std::fmt::Display for RelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for RelationType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "co_occurs" | "cooccurs" => Ok(Self::CoOccurs),
            "part_of" | "partof" => Ok(Self::PartOf),
            "located_in" | "locatedin" => Ok(Self::LocatedIn),
            "related_to" | "relatedto" => Ok(Self::RelatedTo),
            "custom" => Ok(Self::Custom),
            _ => Err(format!("Unknown relation type: {s}")),
        }
    }
}

/// A relationship between two entities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    /// Unique identifier
    pub id: Uuid,
    /// Source entity ID
    pub from_entity_id: Uuid,
    /// Target entity ID
    pub to_entity_id: Uuid,
    /// Type of relationship
    pub relation_type: RelationType,
    /// Strength of the relationship (0.0 - 1.0)
    pub strength: f32,
    /// Additional metadata
    pub metadata: serde_json::Value,
    /// When the relationship was created
    pub created_at: DateTime<Utc>,
}

impl Relationship {
    pub fn new(from_entity_id: Uuid, to_entity_id: Uuid, relation_type: RelationType) -> Self {
        Self {
            id: Uuid::new_v4(),
            from_entity_id,
            to_entity_id,
            relation_type,
            strength: 1.0,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            created_at: Utc::now(),
        }
    }

    pub fn with_strength(mut self, strength: f32) -> Self {
        self.strength = strength.clamp(0.0, 1.0);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// A link between a memory and an entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntityLink {
    /// Memory ID
    pub memory_id: Uuid,
    /// Entity ID
    pub entity_id: Uuid,
    /// Confidence of the entity extraction (0.0 - 1.0)
    pub confidence: f32,
    /// Character offset where entity was found in memory content
    pub start_offset: Option<usize>,
    /// End character offset
    pub end_offset: Option<usize>,
}

impl MemoryEntityLink {
    pub fn new(memory_id: Uuid, entity_id: Uuid, confidence: f32) -> Self {
        Self {
            memory_id,
            entity_id,
            confidence,
            start_offset: None,
            end_offset: None,
        }
    }

    pub fn with_offsets(mut self, start: usize, end: usize) -> Self {
        self.start_offset = Some(start);
        self.end_offset = Some(end);
        self
    }
}

/// Summary of entities found in a memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityExtractionResult {
    /// Memory ID the entities were extracted from
    pub memory_id: Uuid,
    /// Entities that were found/created
    pub entities: Vec<Entity>,
    /// Links between the memory and entities
    pub links: Vec<MemoryEntityLink>,
    /// Relationships discovered between entities (co-occurrence)
    pub relationships: Vec<Relationship>,
}
