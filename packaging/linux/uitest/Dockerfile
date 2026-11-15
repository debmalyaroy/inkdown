# Runs the end-to-end UI checks of Inkdown on Linux, in a container.
#
# The container has no screen and no graphics card: the window opens on a virtual display
# (Xvfb) with a small window manager, and OpenGL is drawn in software (Mesa llvmpipe).
# The checks are the same script that tests the Windows build, native/tools/uitest.ps1.
#
#   docker build -t inkdown-linux-uitest packaging/linux/uitest
#   docker run --rm -v "$PWD:/src:ro" -v "/path/to/dir/with/inkdown:/app:ro" -v "$PWD/dist/linux-uitest:/out" inkdown-linux-uitest
#
# or simply: pwsh packaging/linux/uitest.ps1   (make linux-uitest)
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl xvfb xauth openbox x11-utils \
      libgl1 libgl1-mesa-dri libegl1 libgles2 libglx-mesa0 \
      libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libwayland-egl1 libwayland-cursor0 \
      libx11-6 libx11-xcb1 libxcb1 libxcursor1 libxi6 libxrandr2 \
      libfontconfig1 fonts-dejavu-core fonts-noto-color-emoji \
      python3 python3-pil libicu74 \
    && rm -rf /var/lib/apt/lists/*
# PowerShell, which the test script is written in
ARG PWSH_VERSION=7.4.6
RUN curl -fsSL "https://github.com/PowerShell/PowerShell/releases/download/v${PWSH_VERSION}/powershell-${PWSH_VERSION}-linux-x64.tar.gz" -o /tmp/pwsh.tar.gz \
    && mkdir -p /opt/pwsh && tar -xzf /tmp/pwsh.tar.gz -C /opt/pwsh && chmod +x /opt/pwsh/pwsh \
    && ln -s /opt/pwsh/pwsh /usr/local/bin/pwsh && rm /tmp/pwsh.tar.gz
# draw OpenGL in software, and keep the app on X11
ENV LIBGL_ALWAYS_SOFTWARE=1 GALLIUM_DRIVER=llvmpipe XDG_RUNTIME_DIR=/tmp/runtime HOME=/tmp/home \
    POWERSHELL_TELEMETRY_OPTOUT=1 DOTNET_CLI_TELEMETRY_OPTOUT=1 LANG=C.UTF-8
COPY run.sh /usr/local/bin/run-uitest
RUN chmod +x /usr/local/bin/run-uitest
ENTRYPOINT ["/usr/local/bin/run-uitest"]
