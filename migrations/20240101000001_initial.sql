-- Taula d'usuaris
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    google_id VARCHAR(255) UNIQUE NOT NULL,
    email VARCHAR(255) NOT NULL,
    name VARCHAR(255),
    picture_url TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW() NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW() NOT NULL
);

CREATE INDEX idx_users_google_id ON users(google_id);
CREATE INDEX idx_users_email ON users(email);

-- Taula de dispositius
CREATE TABLE devices (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES users(id) ON DELETE CASCADE NOT NULL,
    google_device_id VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    device_type VARCHAR(100),
    room VARCHAR(255),
    is_active BOOLEAN DEFAULT true NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW() NOT NULL,
    UNIQUE(user_id, google_device_id)
);

CREATE INDEX idx_devices_user_id ON devices(user_id);

-- Taula de regles
CREATE TABLE rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    device_id UUID REFERENCES devices(id) ON DELETE CASCADE NOT NULL,
    name VARCHAR(255) NOT NULL,
    max_hours INTEGER NOT NULL CHECK (max_hours >= 1 AND max_hours <= 24),
    time_window_start TIME,
    time_window_end TIME,
    min_continuous_hours INTEGER DEFAULT 1 NOT NULL CHECK (min_continuous_hours >= 1),
    days_of_week INTEGER DEFAULT 127 NOT NULL CHECK (days_of_week >= 0 AND days_of_week <= 127),
    is_enabled BOOLEAN DEFAULT true NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW() NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW() NOT NULL
);

CREATE INDEX idx_rules_device_id ON rules(device_id);
CREATE INDEX idx_rules_enabled ON rules(is_enabled) WHERE is_enabled = true;

-- Taula d'accions programades
CREATE TABLE scheduled_actions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_id UUID REFERENCES rules(id) ON DELETE CASCADE NOT NULL,
    scheduled_date DATE NOT NULL,
    start_time TIME NOT NULL,
    end_time TIME NOT NULL,
    price_per_kwh DECIMAL(10,5),
    status VARCHAR(50) DEFAULT 'pending' NOT NULL,
    executed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW() NOT NULL,
    UNIQUE(rule_id, scheduled_date, start_time)
);

CREATE INDEX idx_scheduled_actions_date ON scheduled_actions(scheduled_date);
CREATE INDEX idx_scheduled_actions_status ON scheduled_actions(status);
CREATE INDEX idx_scheduled_actions_rule_date ON scheduled_actions(rule_id, scheduled_date);

-- Funció per actualitzar updated_at automàticament
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Triggers per updated_at
CREATE TRIGGER update_users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_rules_updated_at
    BEFORE UPDATE ON rules
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();
