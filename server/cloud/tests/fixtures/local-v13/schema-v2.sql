CREATE TABLE run_details(
 run_id TEXT PRIMARY KEY REFERENCES ai_runs(id),
 payload TEXT NOT NULL CHECK(json_valid(payload))
);
