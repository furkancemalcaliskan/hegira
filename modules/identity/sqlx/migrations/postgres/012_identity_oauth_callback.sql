ALTER TABLE oauth_states
    ADD COLUMN flow TEXT NOT NULL DEFAULT 'login',
    ADD COLUMN username TEXT NULL;

ALTER TABLE oauth_states
    ADD CONSTRAINT ck_oauth_states_flow CHECK (flow IN ('login', 'link')),
    ADD CONSTRAINT ck_oauth_states_link_username
        CHECK (flow <> 'link' OR username IS NOT NULL);
