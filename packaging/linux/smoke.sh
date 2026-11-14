#!/bin/sh
# Starts the app on a virtual display; passes when it is still running after 12 seconds.
printf '# Smoke test\n\nHello from Linux.\n' > /tmp/smoke.md
export INKDOWN_DATA_DIR=/tmp/inkdown-smoke
timeout 12 xvfb-run -a target/release/inkdown /tmp/smoke.md > /tmp/smoke.log 2>&1
CODE=$?
if [ "$CODE" = "124" ] && [ -d /tmp/inkdown-smoke/config ]; then echo "SMOKE: ok (ran for 12s, profile created)"; else echo "SMOKE: FAILED (exit $CODE)"; tail -5 /tmp/smoke.log; fi