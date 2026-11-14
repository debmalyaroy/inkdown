# Android build of Inkdown (the pure-Rust app in native/).
#
#   docker build -f packaging/android/Dockerfile -t inkdown-android .
#   docker run --rm -v "${PWD}/dist/android:/out" inkdown-android
#
# Produces a universal APK (arm64-v8a + x86_64) signed with a throwaway test key.
# For Google Play, sign with your own upload key instead (see packaging/android/README.md).
#
# NOTE: the sdkmanager step below accepts the Android SDK licence agreement.
FROM rust:1-bookworm

ENV ANDROID_HOME=/opt/android \
    ANDROID_NDK_ROOT=/opt/android/ndk/26.3.11579264 \
    PATH=$PATH:/opt/android/cmdline-tools/latest/bin:/opt/android/platform-tools

RUN apt-get update && apt-get install -y --no-install-recommends openjdk-17-jdk-headless unzip curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p $ANDROID_HOME/cmdline-tools \
    && curl -fsSL -o /tmp/tools.zip https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip \
    && unzip -q /tmp/tools.zip -d $ANDROID_HOME/cmdline-tools \
    && mv $ANDROID_HOME/cmdline-tools/cmdline-tools $ANDROID_HOME/cmdline-tools/latest \
    && rm /tmp/tools.zip

RUN yes | sdkmanager --licenses > /dev/null \
    && sdkmanager "platform-tools" "platforms;android-34" "build-tools;34.0.0" "ndk;26.3.11579264" > /dev/null

RUN rustup target add aarch64-linux-android x86_64-linux-android \
    && cargo install cargo-apk --locked

# a test signing key, generated inside the image; never use it for a store upload
RUN keytool -genkeypair -v -keystore /opt/test.keystore -alias inkdown -keyalg RSA -keysize 2048 -validity 10000 \
      -storepass inkdown-test -keypass inkdown-test -dname "CN=Inkdown Test, O=Inkdown, C=US" 2> /dev/null
ENV CARGO_APK_RELEASE_KEYSTORE=/opt/test.keystore \
    CARGO_APK_RELEASE_KEYSTORE_PASSWORD=inkdown-test

WORKDIR /src
# the native crate reuses a few files of the root crate by path
COPY src ./src
COPY assets/icon.png assets/icon.ico ./assets/
COPY packaging/android/res ./packaging/android/res
COPY native/Cargo.toml native/Cargo.lock native/build.rs ./native/
COPY native/src ./native/src
COPY native/fonts ./native/fonts
COPY native/assets ./native/assets

WORKDIR /src/native
# The build directory and the crate registry are kept in BuildKit caches, so a rebuild after a
# source change only recompiles Inkdown itself. JOBS caps parallel compiler processes (memory).
ARG JOBS=6
RUN --mount=type=cache,target=/src/native/target,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    CARGO_BUILD_JOBS=$JOBS cargo apk build --release --lib \
    && mkdir -p /apk && cp target/release/apk/*.apk /apk/

CMD ["sh", "-c", "cp /apk/*.apk /out/ && ls -la /out"]
