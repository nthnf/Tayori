use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_settings(manager).await?;
        seed_settings(manager).await?;

        create_projects(manager).await?;
        create_sessions(manager).await?;
        create_documents(manager).await?;

        create_transcript_chunks(manager).await?;
        create_session_answers(manager).await?;

        create_document_chunks(manager).await?;

        create_indexes(manager).await?;

        create_fts_indexes(manager).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop the chunk embedding table first (due to foreign keys)
        manager
            .drop_table(
                Table::drop()
                    .table(DocumentChunks::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;

        // Drop existing tables
        manager
            .drop_table(
                Table::drop()
                    .table(SessionAnswers::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(TranscriptChunks::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Documents::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Sessions::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Projects::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Settings::Table).if_exists().to_owned())
            .await?;

        let db = manager.get_connection();

        let sql = "
            DROP TRIGGER IF EXISTS fts_insert_chunks;
            DROP TRIGGER IF EXISTS fts_delete_chunks;
            DROP TRIGGER IF EXISTS fts_insert_docs;
            DROP TRIGGER IF EXISTS fts_delete_docs;
            DROP TABLE IF EXISTS global_fts;
        ";

        // Drop the triggers and the virtual table
        db.execute_unprepared(sql).await?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// TABLE CREATIONS
// ---------------------------------------------------------------------------

async fn create_settings(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Settings::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(Settings::Id)
                        .string()
                        .not_null()
                        .primary_key()
                        .check((
                            "ck_settings_id_default",
                            Expr::col(Settings::Id).eq("default"),
                        )),
                )
                .col(
                    ColumnDef::new(Settings::LlmProvider)
                        .string()
                        .not_null()
                        .check((
                            "ck_settings_llm_provider",
                            Expr::col(Settings::LlmProvider).is_in(["openai"]),
                        )),
                )
                .col(ColumnDef::new(Settings::LlmModel).string().not_null())
                .col(
                    ColumnDef::new(Settings::TranscriptModel)
                        .string()
                        .not_null()
                        .default("medium")
                        .check((
                            "ck_settings_transcript_model",
                            Expr::col(Settings::TranscriptModel).is_in(["tiny", "small", "medium"]),
                        )),
                )
                .col(
                    ColumnDef::new(Settings::SummaryMinutes)
                        .integer()
                        .not_null()
                        .default(5)
                        .check((
                            "ck_settings_summary_minutes",
                            Expr::col(Settings::SummaryMinutes).between(1, 10),
                        )),
                )
                .col(
                    ColumnDef::new(Settings::UiTheme)
                        .string()
                        .not_null()
                        .default("light")
                        .check((
                            "ck_settings_ui_theme",
                            Expr::col(Settings::UiTheme).is_in(["dark", "light"]),
                        )),
                )
                .col(
                    ColumnDef::new(Settings::CreatedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(
                    ColumnDef::new(Settings::UpdatedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .to_owned(),
        )
        .await
}

async fn seed_settings(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let insert = Query::insert()
        .into_table(Settings::Table)
        .columns([
            Settings::Id,
            Settings::LlmProvider,
            Settings::LlmModel,
            Settings::TranscriptModel,
            Settings::SummaryMinutes,
            Settings::UiTheme,
        ])
        .values_panic([
            "default".into(),
            "openai".into(),
            "gpt-5.4-mini".into(),
            "medium".into(),
            5.into(),
            "light".into(),
        ])
        .on_conflict(OnConflict::column(Settings::Id).do_nothing().to_owned())
        .to_owned();

    manager.execute(insert).await
}

async fn create_projects(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Projects::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(Projects::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(ColumnDef::new(Projects::Name).string().not_null())
                .col(ColumnDef::new(Projects::Description).text().null())
                .col(
                    ColumnDef::new(Projects::CreatedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(
                    ColumnDef::new(Projects::UpdatedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .to_owned(),
        )
        .await
}

async fn create_sessions(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Sessions::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(Sessions::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(ColumnDef::new(Sessions::ProjectId).string().not_null())
                .col(ColumnDef::new(Sessions::Title).string().null())
                .col(ColumnDef::new(Sessions::Status).string().not_null().check((
                    "ck_sessions_status",
                    Expr::col(Sessions::Status).is_in([
                        "running",
                        "completed",
                        "cancelled",
                        "failed",
                    ]),
                )))
                .col(
                    ColumnDef::new(Sessions::StartedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(ColumnDef::new(Sessions::EndedAt).timestamp().null())
                .col(
                    ColumnDef::new(Sessions::CreatedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(
                    ColumnDef::new(Sessions::UpdatedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_sessions_project_id")
                        .from(Sessions::Table, Sessions::ProjectId)
                        .to(Projects::Table, Projects::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
}

async fn create_documents(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Documents::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(Documents::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(ColumnDef::new(Documents::ProjectId).string().not_null())
                .col(ColumnDef::new(Documents::SourceName).string().not_null())
                .col(ColumnDef::new(Documents::OriginalPath).text().null())
                .col(ColumnDef::new(Documents::ContentHash).string().null())
                .col(
                    ColumnDef::new(Documents::Status)
                        .string()
                        .not_null()
                        .check((
                            "ck_documents_status",
                            Expr::col(Documents::Status).is_in(["ingesting", "ready", "failed"]),
                        )),
                )
                .col(ColumnDef::new(Documents::ErrorMessage).text().null())
                .col(
                    ColumnDef::new(Documents::CreatedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(
                    ColumnDef::new(Documents::UpdatedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_documents_project_id")
                        .from(Documents::Table, Documents::ProjectId)
                        .to(Projects::Table, Projects::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
}

async fn create_document_chunks(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(DocumentChunks::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(DocumentChunks::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(DocumentChunks::DocumentId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(DocumentChunks::ChunkIndex)
                        .integer()
                        .not_null(),
                )
                .col(ColumnDef::new(DocumentChunks::Content).text().not_null())
                .col(ColumnDef::new(DocumentChunks::Vector).binary().not_null())
                .col(
                    ColumnDef::new(DocumentChunks::CreatedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_document_chunks_document_id")
                        .from(DocumentChunks::Table, DocumentChunks::DocumentId)
                        .to(Documents::Table, Documents::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
}

async fn create_transcript_chunks(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(TranscriptChunks::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(TranscriptChunks::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(TranscriptChunks::SessionId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(TranscriptChunks::ProjectId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(TranscriptChunks::ChunkIndex)
                        .integer()
                        .not_null(),
                )
                .col(ColumnDef::new(TranscriptChunks::Text).text().not_null())
                .col(
                    ColumnDef::new(TranscriptChunks::StartMs)
                        .big_integer()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(TranscriptChunks::EndMs)
                        .big_integer()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(TranscriptChunks::DurationMs)
                        .big_integer()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(TranscriptChunks::CreatedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_transcript_chunks_session_project")
                        .from_tbl(TranscriptChunks::Table)
                        .from_col(TranscriptChunks::SessionId)
                        .from_col(TranscriptChunks::ProjectId)
                        .to_tbl(Sessions::Table)
                        .to_col(Sessions::Id)
                        .to_col(Sessions::ProjectId)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
}

async fn create_session_answers(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(SessionAnswers::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(SessionAnswers::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(SessionAnswers::ProjectId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(SessionAnswers::SessionId)
                        .string()
                        .not_null(),
                )
                .col(ColumnDef::new(SessionAnswers::Context).text().null())
                .col(ColumnDef::new(SessionAnswers::Query).text().not_null())
                .col(ColumnDef::new(SessionAnswers::Answer).text().not_null())
                .col(
                    ColumnDef::new(SessionAnswers::CreatedAt)
                        .timestamp()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_session_answers_session_project")
                        .from_tbl(SessionAnswers::Table)
                        .from_col(SessionAnswers::SessionId)
                        .from_col(SessionAnswers::ProjectId)
                        .to_tbl(Sessions::Table)
                        .to_col(Sessions::Id)
                        .to_col(Sessions::ProjectId)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
}

async fn create_indexes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_index(
            Index::create()
                .name("idx_sessions_id_project_id")
                .table(Sessions::Table)
                .col(Sessions::Id)
                .col(Sessions::ProjectId)
                .unique()
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_sessions_project_id")
                .table(Sessions::Table)
                .col(Sessions::ProjectId)
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_sessions_started_at")
                .table(Sessions::Table)
                .col(Sessions::StartedAt)
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_documents_project_id")
                .table(Documents::Table)
                .col(Documents::ProjectId)
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_documents_status")
                .table(Documents::Table)
                .col(Documents::Status)
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_documents_project_hash")
                .table(Documents::Table)
                .col(Documents::ProjectId)
                .col(Documents::ContentHash)
                .unique()
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_transcript_chunks_project_id")
                .table(TranscriptChunks::Table)
                .col(TranscriptChunks::ProjectId)
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_transcript_chunks_session_id")
                .table(TranscriptChunks::Table)
                .col(TranscriptChunks::SessionId)
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_transcript_chunks_session_index")
                .table(TranscriptChunks::Table)
                .col(TranscriptChunks::SessionId)
                .col(TranscriptChunks::ChunkIndex)
                .unique()
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_session_answers_project_id")
                .table(SessionAnswers::Table)
                .col(SessionAnswers::ProjectId)
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_session_answers_session_id")
                .table(SessionAnswers::Table)
                .col(SessionAnswers::SessionId)
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_session_answers_created_at")
                .table(SessionAnswers::Table)
                .col(SessionAnswers::CreatedAt)
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_document_chunks_document_id")
                .table(DocumentChunks::Table)
                .col(DocumentChunks::DocumentId)
                .if_not_exists()
                .to_owned(),
        )
        .await?;

    Ok(())
}

async fn create_fts_indexes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // We use fts5 for high-speed keyword matching.
    // source_id: The UUID/CUID of the original record.
    // source_type: 'transcript', 'summary', or 'document'.
    let sql = "
        CREATE VIRTUAL TABLE IF NOT EXISTS global_fts USING fts5(
            source_id UNINDEXED,
            source_type UNINDEXED,
            content
        );

        -- Sync Transcript Chunks
        CREATE TRIGGER IF NOT EXISTS fts_insert_chunks AFTER INSERT ON transcript_chunks BEGIN
            INSERT INTO global_fts (source_id, source_type, content) VALUES (NEW.id, 'transcript', NEW.text);
        END;
        CREATE TRIGGER IF NOT EXISTS fts_delete_chunks AFTER DELETE ON transcript_chunks BEGIN
            DELETE FROM global_fts WHERE source_id = OLD.id;
        END;

        -- Sync Document Chunks
        CREATE TRIGGER IF NOT EXISTS fts_insert_docs AFTER INSERT ON document_chunks BEGIN
            INSERT INTO global_fts (source_id, source_type, content) VALUES (NEW.id, 'document', NEW.content);
        END;
        CREATE TRIGGER IF NOT EXISTS fts_delete_docs AFTER DELETE ON document_chunks BEGIN
            DELETE FROM global_fts WHERE source_id = OLD.id;
        END;
    ";

    db.execute_unprepared(sql).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// ENUMS
// ---------------------------------------------------------------------------

#[derive(DeriveIden)]
enum Settings {
    Table,
    Id,
    LlmProvider,
    LlmModel,
    TranscriptModel,
    SummaryMinutes,
    UiTheme,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
    Name,
    Description,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Sessions {
    Table,
    Id,
    ProjectId,
    Title,
    Status,
    StartedAt,
    EndedAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Documents {
    Table,
    Id,
    ProjectId,
    SourceName,
    OriginalPath,
    ContentHash,
    Status,
    ErrorMessage,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum DocumentChunks {
    Table,
    Id,
    DocumentId,
    ChunkIndex,
    Content,
    Vector,
    CreatedAt,
}

#[derive(DeriveIden)]
enum TranscriptChunks {
    Table,
    Id,
    ProjectId,
    SessionId,
    ChunkIndex,
    Text,
    StartMs,
    EndMs,
    DurationMs,
    CreatedAt,
}

#[derive(DeriveIden)]
enum SessionAnswers {
    Table,
    Id,
    ProjectId,
    SessionId,
    Context,
    Query,
    Answer,
    CreatedAt,
}
