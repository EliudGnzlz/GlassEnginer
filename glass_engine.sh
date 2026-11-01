#!/usr/bin/env bash
# Lanzador de GlassEngine
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

# En entornos Wayland con GPUs NVIDIA, forzar backend X11/XWayland garantiza máxima estabilidad en Winit y Vulkan
if [ -n "$DISPLAY" ] && [ -z "$WINIT_UNIX_BACKEND" ]; then
    export WINIT_UNIX_BACKEND=x11
fi

if [ -f "$DIR/target/release/glass_app" ]; then
    EXECUTABLE="$DIR/target/release/glass_app"
elif [ -f "$DIR/target/debug/glass_app" ]; then
    EXECUTABLE="$DIR/target/debug/glass_app"
else
    cargo build --release --bin glass_app
    EXECUTABLE="$DIR/target/release/glass_app"
fi

echo "=========================================="
echo "  Iniciando GlassEngine (Vulkan + egui)   "
echo "  Binario: $EXECUTABLE"
echo "=========================================="
exec "$EXECUTABLE" "$@"
