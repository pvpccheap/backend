-- Afegir UNIQUE constraint per evitar duplicats de scheduled_actions
-- Una regla no pot tenir més d'una acció per la mateixa data i hora

ALTER TABLE scheduled_actions
ADD CONSTRAINT scheduled_actions_rule_date_time_unique
UNIQUE (rule_id, scheduled_date, start_time);
