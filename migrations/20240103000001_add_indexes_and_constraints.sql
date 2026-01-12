-- Migració per afegir índexs i constraints per millorar rendiment i integritat

-- =============================================================================
-- ÍNDEXS PER QUERIES COMUNES
-- =============================================================================

-- Index per buscar schedules per data i status (usat en background_tasks)
CREATE INDEX IF NOT EXISTS idx_scheduled_actions_date_status
ON scheduled_actions(scheduled_date, status);

-- Index per buscar schedules per regla, data i status (usat en regenerate_schedules)
CREATE INDEX IF NOT EXISTS idx_scheduled_actions_rule_date_status
ON scheduled_actions(rule_id, scheduled_date, status);

-- Index per buscar regles actives per dispositiu (usat en generate_schedules_for_date)
CREATE INDEX IF NOT EXISTS idx_rules_device_enabled
ON rules(device_id, is_enabled) WHERE is_enabled = true;

-- Index parcial per schedules pendents (molt usat per trobar accions a executar)
CREATE INDEX IF NOT EXISTS idx_scheduled_actions_pending
ON scheduled_actions(scheduled_date, start_time) WHERE status = 'pending';

-- =============================================================================
-- CHECK CONSTRAINTS PER INTEGRITAT DE DADES
-- =============================================================================

-- Constraint per validar que max_hours és entre 1 i 24
ALTER TABLE rules
ADD CONSTRAINT chk_rules_max_hours
CHECK (max_hours >= 1 AND max_hours <= 24);

-- Constraint per validar que min_continuous_hours és >= 1 i <= max_hours
ALTER TABLE rules
ADD CONSTRAINT chk_rules_min_continuous_hours
CHECK (min_continuous_hours >= 1 AND min_continuous_hours <= max_hours);

-- Constraint per validar que days_of_week és un bitmask vàlid (1-127)
ALTER TABLE rules
ADD CONSTRAINT chk_rules_days_of_week
CHECK (days_of_week >= 1 AND days_of_week <= 127);

-- Constraint per validar els valors de status
-- Nota: Usem una CHECK constraint amb IN en lloc de ENUM per simplicitat
ALTER TABLE scheduled_actions
ADD CONSTRAINT chk_scheduled_actions_status
CHECK (status IN ('pending', 'executed', 'executed_on', 'executed_off', 'failed', 'cancelled', 'missed'));

-- =============================================================================
-- COMENTARIS PER DOCUMENTACIÓ
-- =============================================================================

COMMENT ON INDEX idx_scheduled_actions_date_status IS
'Optimitza queries de background tasks que busquen accions per data i status';

COMMENT ON INDEX idx_scheduled_actions_rule_date_status IS
'Optimitza regeneració de schedules per una regla específica';

COMMENT ON INDEX idx_rules_device_enabled IS
'Optimitza obtenció de regles actives per generació diària de schedules';

COMMENT ON INDEX idx_scheduled_actions_pending IS
'Optimitza cerca d''accions pendents per executar';
