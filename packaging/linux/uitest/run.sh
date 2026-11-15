#!/bin/sh
# Inside the container: start a virtual display, then run the UI checks against /app/inkdown.
# Arguments are passed on to uitest.ps1 (for example: -Only editor).
set -u
mkdir -p "$XDG_RUNTIME_DIR" "$HOME" /out /work
chmod 700 "$XDG_RUNTIME_DIR"

if [ ! -f /app/inkdown ]; then
    echo "No binary at /app/inkdown: mount the folder that holds the Linux build of Inkdown at /app." >&2
    exit 2
fi
# the binary is mounted read-only and may have lost its executable bit on the way
cp /app/inkdown /work/inkdown && chmod +x /work/inkdown

export DISPLAY=:99
Xvfb :99 -screen 0 1600x1000x24 -nolisten tcp > /tmp/xvfb.log 2>&1 &
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    xdpyinfo > /dev/null 2>&1 && break
    sleep 0.25
done
if ! xdpyinfo > /dev/null 2>&1; then
    echo "The virtual display did not start." >&2
    cat /tmp/xvfb.log >&2
    exit 2
fi
# a window manager, so that the window gets the focus like on a desktop
openbox > /tmp/openbox.log 2>&1 &
sleep 0.5

echo "Inkdown UI checks on Linux ($(grep -m1 PRETTY_NAME /etc/os-release | cut -d= -f2), software OpenGL)"
pwsh -NoProfile -File /src/native/tools/uitest.ps1 -Exe /work/inkdown -Out /out/run "$@"
CODE=$?
# keep the screenshots and state dumps of a failed run where the host can read them
chmod -R a+rwX /out 2>/dev/null
exit $CODE
