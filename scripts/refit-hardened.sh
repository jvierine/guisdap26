#!/usr/bin/env bash
set -euo pipefail
cd /home/j/src/guisdap26
export PATH="$HOME/.cargo/bin:$PATH"
for kind in full native; do
  for cadence in 60 15; do
    config="config/ion-fit-hardened-${kind}-${cadence}s.json"
    dataset="2022-11-02-ionfit-${kind}-${cadence}s-hardened-v3"
    previous="/srv/guisdap/data/2022-11-02-ionfit-${kind}-${cadence}s-v2/index.json"
    next="/srv/guisdap/data/${dataset}/index.json"
    echo "START ${kind} ${cadence}s with 128 workers $(date -u +%FT%TZ)"
    target/release/guisdap-worker fit-ion --config "$config" > "local/refit-${kind}-${cadence}s.log" 2>&1 &
    worker=$!
    while [[ ! -s "$next" ]]; do
      if ! kill -0 "$worker" 2>/dev/null; then wait "$worker"; exit 1; fi
      sleep 2
    done
    python3 scripts/publish-refit.py "$kind" "$cadence" "$dataset" running
    wait "$worker"
    target/release/examples/check_refit "$previous" "$next" > "local/refit-${kind}-${cadence}s-validation.json"
    python3 scripts/publish-refit.py "$kind" "$cadence" "$dataset" complete
    echo "VALIDATED ${kind} ${cadence}s $(date -u +%FT%TZ)"
    cat "local/refit-${kind}-${cadence}s-validation.json"
  done
done
