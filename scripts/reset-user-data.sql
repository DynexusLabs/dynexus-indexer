-- Removes user activity history, keeps tokens, pools, gauges, epochs and live positions.
-- Usage: psql -v ON_ERROR_STOP=1 -v days=0 -f scripts/reset-user-data.sql
BEGIN;
SELECT now() - make_interval(days => :days) AS cutoff \gset

DELETE FROM swaps WHERE ts < :'cutoff';
DELETE FROM liquidity_events WHERE ts < :'cutoff';

-- The raw log is the processor's replay guard; keep the last day so a batch replayed
-- after a crash is still recognised and not applied twice.
DELETE FROM dynexus_events
 WHERE ts < LEAST(:'cutoff'::timestamptz, now() - interval '1 day');

-- Epoch history: only finished epochs (the highest epoch in `epochs` is kept).
DELETE FROM votes WHERE epoch < (SELECT COALESCE(MAX(epoch), 0) FROM epochs);
DELETE FROM bribes WHERE epoch < (SELECT COALESCE(MAX(epoch), 0) FROM epochs);
COMMIT;
