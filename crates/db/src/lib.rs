//! Diesel schema, models and migrations shared by the processor and the API.

pub mod models;
pub mod schema;

use diesel_migrations::{EmbeddedMigrations, embed_migrations};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
