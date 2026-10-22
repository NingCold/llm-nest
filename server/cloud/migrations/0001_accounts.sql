CREATE TABLE accounts (id uuid PRIMARY KEY, name text NOT NULL, bio text NOT NULL DEFAULT '', kind text NOT NULL DEFAULT 'human', created_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE credentials (account_id uuid PRIMARY KEY REFERENCES accounts(id), email text NOT NULL UNIQUE, password_hash text NOT NULL);
CREATE TABLE invitations (id uuid PRIMARY KEY, code_hash text NOT NULL UNIQUE, expires_at timestamptz NOT NULL, revoked boolean NOT NULL DEFAULT false, consumed_at timestamptz, reservation_id uuid, reserved_until timestamptz);
CREATE TABLE registrations (id uuid PRIMARY KEY, invitation_id uuid NOT NULL REFERENCES invitations(id), email text NOT NULL, name text NOT NULL, password_hash text NOT NULL, resume_hash text NOT NULL, verify_hash text NOT NULL UNIQUE, expires_at timestamptz NOT NULL, created_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE sessions (id uuid PRIMARY KEY, account_id uuid NOT NULL REFERENCES accounts(id), token_hash text NOT NULL UNIQUE, created_at timestamptz NOT NULL DEFAULT now(), expires_at timestamptz NOT NULL, revoked_at timestamptz);
CREATE INDEX sessions_account ON sessions(account_id);
CREATE TABLE password_resets (token_hash text PRIMARY KEY, account_id uuid NOT NULL REFERENCES accounts(id), expires_at timestamptz NOT NULL);
CREATE TABLE auth_throttles (key text PRIMARY KEY, started_at timestamptz NOT NULL DEFAULT now(), attempts integer NOT NULL DEFAULT 1);
CREATE INDEX auth_throttles_expiry ON auth_throttles(started_at);
