#!/bin/sh
# Biweekly "reset": removes user activity history, keeps the protocol's structure.
#
#   removed: swaps, liquidity events, raw event log, votes and bribes of finished epochs
#   kept:    tokens, pools (reserves, LP supply), gauges, epochs, and live positions
#            (ve_locks, gauge_stakes, votes/bribes of the current epoch) because the
#            processor updates those with deltas and the chain still holds them
#
# The processor keeps running and keeps its checkpoint: nothing is replayed. Run from
# deploy/oracle (see cron.example). RETENTION_DAYS=0 (default) clears all history;
# RETENTION_DAYS=14 run daily gives a rolling two-week window instead.
set -eu
cd "$(dirname "$0")"

days=${RETENTION_DAYS:-0}
case "$days" in ''|*[!0-9]*) echo "reset-data: RETENTION_DAYS must be a number" >&2; exit 1 ;; esac

echo "reset-data: removing user history older than $days day(s)"
docker compose exec -T postgres psql -v ON_ERROR_STOP=1 -v days="$days" -U dynexus dynexus < ../../scripts/reset-user-data.sql
# Return freed pages to the OS (swaps/events are the big tables).
docker compose exec -T postgres psql -U dynexus dynexus -c 'VACUUM (ANALYZE) swaps, liquidity_events, dynexus_events, votes, bribes' >/dev/null
echo "reset-data: done"
