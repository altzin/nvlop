mod ast;
mod test_builder;

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, postgres::PgPoolOptions};
use std::env;
use std::time::Duration;
use tokio::net::TcpListener;

#[derive(Deserialize)]
struct WebhookPayload {
    commit_hash: String,
    target_branch: String,
    diff: String,
}

#[derive(FromRow, Debug)]
struct Job {
    id: i32,
    commit_hash: String,
    target_branch: String,
    diff: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct TestPlanItem {
    target_method: String,
    scenario: String,
    expected_behavior: String,
    mock_requirements: Vec<String>,
}

#[derive(Clone)]
struct AppState {
    db: PgPool,
    ollama_url: String,
}

// --- Intake Endpoint ---

async fn handle_webhook(
    State(state): State<AppState>,
    Json(payload): Json<WebhookPayload>,
) -> (StatusCode, &'static str) {
    let result = sqlx::query(
        r#"
        INSERT INTO job_queue (commit_hash, target_branch, diff, status)
        VALUES ($1, $2, $3, 'PENDING')
        ON CONFLICT (commit_hash) DO NOTHING
        "#,
    )
    .bind(&payload.commit_hash)
    .bind(&payload.target_branch)
    .bind(&payload.diff)
    .execute(&state.db)
    .await;

    match result {
        Ok(res) if res.rows_affected() == 0 => {
            (StatusCode::OK, "Commit already queued or processed")
        }
        Ok(_) => (StatusCode::ACCEPTED, "Job queued"),
        Err(e) => {
            eprintln!("DB insert error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to queue job")
        }
    }
}

// --- Background FIFO Worker ---

async fn start_background_worker(state: AppState) {
    loop {
        let mut tx = match state.db.begin().await {
            Ok(tx) => tx,
            Err(e) => {
                eprintln!("Failed to start transaction: {e}");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let job = sqlx::query_as::<_, Job>(
            r#"
            SELECT id, commit_hash, target_branch, diff
            FROM job_queue
            WHERE status = 'PENDING'
            ORDER BY created_at ASC
            FOR UPDATE SKIP LOCKED
            LIMIT 1
            "#,
        )
        .fetch_optional(&mut *tx)
        .await;

        match job {
            Ok(Some(job)) => {
                let _ = sqlx::query(
                    "UPDATE job_queue SET status = 'PROCESSING', updated_at = NOW() WHERE id = $1",
                )
                .bind(job.id)
                .execute(&mut *tx)
                .await;

                if let Err(e) = tx.commit().await {
                    eprintln!("Commit failed: {e}");
                    continue;
                }

                println!(
                    "Picked up commit {} (branch: {})",
                    job.commit_hash, job.target_branch
                );

                // Stubbed pipeline execution
                match run_pipeline(&job, &state.ollama_url).await {
                    Ok(_) => {
                        let _ = sqlx::query(
                            "UPDATE job_queue SET status = 'COMPLETED', updated_at = NOW() WHERE id = $1",
                        )
                        .bind(job.id)
                        .execute(&state.db)
                        .await;
                        println!("Finished commit {}", job.commit_hash);
                    }
                    Err(e) => {
                        eprintln!("Pipeline failed for {}: {e}", job.commit_hash);
                        let _ = sqlx::query(
                            "UPDATE job_queue SET status = 'FAILED', error_message = $1, updated_at = NOW() WHERE id = $2",
                        )
                        .bind(e.to_string())
                        .bind(job.id)
                        .execute(&state.db)
                        .await;
                    }
                }
            }
            Ok(None) => {
                let _ = tx.rollback().await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Err(e) => {
                eprintln!("Worker query error: {e}");
                let _ = tx.rollback().await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
async fn run_pipeline(
    job: &Job,
    _ollama_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Next step: wire up reqwest calls to DeepSeek and Qwen here
    println!("Diff length: {} bytes", job.diff.len());
    tokio::time::sleep(Duration::from_secs(2)).await;
    Ok(())
}

// --- Entrypoint ---

#[tokio::main]
async fn main() {
    // .ok() ignores a missing .env file so container-injected env vars still work
    dotenvy::dotenv().ok();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let ollama_url = env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into());
    let port = env::var("PORT").unwrap_or_else(|_| "3000".into());

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to Postgres");

    // Automatically runs any unapplied .sql files in ./migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run database migrations");

    let state = AppState {
        db: pool,
        ollama_url,
    };

    tokio::spawn(start_background_worker(state.clone()));

    let app = Router::new()
        .route("/webhook", post(handle_webhook))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).await.unwrap();
    println!("Listening on {addr}");
    axum::serve(listener, app).await.unwrap();
}
