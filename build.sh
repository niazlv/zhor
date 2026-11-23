#!/bin/sh
# Сборка «Жора».
#   ./build.sh                 ядро (Rust, arm64) → jniLibs → APK
#   ./build.sh install         то же + adb install
#   ./build.sh core            только ядро → android/app/src/main/jniLibs/arm64-v8a/libzhord.so
#   ./build.sh app [install]   только APK (с тем ядром, что уже лежит в jniLibs)
set -e
cd "$(dirname "$0")"
: "${ANDROID_NDK_HOME:=$HOME/Library/Android/sdk/ndk/27.1.12297006}"
export ANDROID_NDK_HOME
# Gradle/AGP нужен JDK 17–21; на macOS берём установленный, если JAVA_HOME не задан
if [ -z "$JAVA_HOME" ] && [ -x /usr/libexec/java_home ]; then
    JAVA_HOME=$(/usr/libexec/java_home -v 21 2>/dev/null || /usr/libexec/java_home -v 17 2>/dev/null || true)
    [ -n "$JAVA_HOME" ] && export JAVA_HOME
fi
LIB=android/app/src/main/jniLibs/arm64-v8a/libzhord.so
APK=android/app/build/outputs/apk/release/app-release.apk

build_core() {
    (cd core && cargo test --quiet --locked && cargo ndk -t arm64-v8a -P 29 build --release --locked)
    mkdir -p "$(dirname "$LIB")"
    cp core/target/aarch64-linux-android/release/zhord "$LIB"
    echo "ядро → $LIB ($(wc -c < "$LIB") байт)"
}
build_app() {
    [ -f "$LIB" ] || { echo "нет $LIB — сначала ./build.sh core"; exit 1; }
    (cd android && ./gradlew --quiet assembleRelease)
    ls -la "$APK"
}

case "$1" in
    core) build_core ;;
    app) build_app; if [ "$2" = "install" ]; then adb install -r "$APK"; fi ;;
    install) build_core; build_app; adb install -r "$APK" ;;
    *) build_core; build_app ;;
esac
