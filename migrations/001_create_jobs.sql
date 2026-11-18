CREATE TABLE IF NOT EXISTS job_queue (
    id SERIAL PRIMARY KEY,
    commit_hash VARCHAR(40) NOT NULL UNIQUE,
    target_branch VARCHAR(255) NOT NULL,
    diff TEXT NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'PENDING',
    deepseek_plan JSONB,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_job_queue_status_created 
    ON job_queue (status, created_at ASC);
