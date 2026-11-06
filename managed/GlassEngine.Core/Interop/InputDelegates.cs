using System;
using System.Runtime.InteropServices;

namespace GlassEngine.Core.Interop;

// Espejo de `InputDataFfi` en Rust (crates/glass_scripting/src/bindings.rs):
// mismo orden y mismos tipos; el layout secuencial debe producir los mismos
// offsets (MoveX 0, MoveZ 4, Jump 8, Run 9, MouseX 12, MouseY 16,
// MouseLeftHeld 20, MouseLeftDown 21, MouseLeftUp 22; tamaño 24). En Rust
// hay un test de layout que fija el contrato.
[StructLayout(LayoutKind.Sequential)]
public struct InputData
{
    public float MoveX; // -1.0 (A/Left) .. +1.0 (D/Right)
    public float MoveZ; // -1.0 (S/Down) .. +1.0 (W/Up)
    public byte Jump;   // 1 if Space pressed
    public byte Run;    // 1 if Shift pressed
    public byte Camera; // 1 si V está pulsada (cambio de cámara Plat3D). Vive en
                        // el byte de relleno entre Run y MouseX: tamaño (24) y
                        // offsets idénticos a los de siempre en ambos lados.
    public float MouseX; // Ratón en píxeles del offscreen (espacio del Canvas)
    public float MouseY;
    public byte MouseLeftHeld; // 1 mientras el botón izquierdo está pulsado
    public byte MouseLeftDown; // 1 solo en el frame de la pulsación (borde)
    public byte MouseLeftUp;   // 1 solo en el frame de la liberación (borde)
// --- Input v2: ejes configurables ---
    // El host Rust escribe, a continuación de MouseLeftUp, el conteo y los
    // buffers de ejes (suavizado y crudo, hasta 32) y las máscaras de botones.
    //
    // Layout actualizado (offsets en bytes, igual alineación que Rust):
    //  0   MoveX
    //  4   MoveZ
    //  8   Jump
    //  9   Run
    //  10  Camera
    //  12  MouseX
    //  16  MouseY
    //  20  MouseLeftHeld
    //  21  MouseLeftDown
    //  22  MouseLeftUp
    //  (23: 1 byte de relleno para alinear AxisCount a 4 bytes)
    //  24  AxisCount
    //  28  Axes (32 floats)
    //  156 AxesRaw (32 floats)
    //  284 Buttons
    //  288 ButtonsDown
    //  292 ButtonsUp
    //  Tamaño total: 296 bytes
    public int AxisCount;

    [MarshalAs(UnmanagedType.ByValArray, SizeConst = 32)]
    public float[] Axes;

    [MarshalAs(UnmanagedType.ByValArray, SizeConst = 32)]
    public float[] AxesRaw;

    public uint Buttons;
    public uint ButtonsDown;
    public uint ButtonsUp;
}

/// <summary>
/// Pregunta al host cuántos ejes configurables hay disponibles.
/// </summary>
public delegate int InputAxisCountDelegate();

/// <summary>
/// Pregunta al host el nombre del eje en el índice dado. Escribe el nombre
/// (UTF-8, sin terminador) en buffer y devuelve los bytes escritos, o -1
/// si el índice no existe.
/// </summary>
public delegate int InputAxisNameDelegate(int index, byte[] buffer, int bufferLength);

/// <summary>
/// Callbacks del host para consultar metadatos de ejes configurables.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public struct InputAxesCallbacks
{
    public IntPtr Count;
    public IntPtr Name;
}

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int GetInputDelegate(out InputData input);
