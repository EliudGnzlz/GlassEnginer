using System;
using System.Collections.Generic;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core;

public static class Input
{
    private static GetInputDelegate? _getInput;
    private static Dictionary<string, int>? _axisIndex;

    /// <summary>
    /// Inicializa el puente de entrada con el delegado provisto por el host.
    /// Invalida la caché de índices de ejes configurables.
    /// </summary>
    public static void Initialize(GetInputDelegate del)
    {
        _getInput = del;
        _axisIndex = null;
    }

    private static bool TryRead(out InputData input)
    {
        if (_getInput == null)
        {
            input = default;
            return false;
        }

        return _getInput(out input) == 0;
    }

    /// <summary>
    /// Construye (o reutiliza) la caché de nombre de eje a índice,
    /// consultando al host mediante RuntimeBridge.
    /// </summary>
    private static Dictionary<string, int> AxisIndex()
    {
        if (_axisIndex != null) return _axisIndex;

        var d = new Dictionary<string, int>(StringComparer.OrdinalIgnoreCase);
        int n = RuntimeBridge.AxisCount();
        for (int i = 0; i < n; i++)
        {
            var name = RuntimeBridge.AxisName(i);
            if (!string.IsNullOrEmpty(name))
                d[name] = i;
        }

        _axisIndex = d;
        return _axisIndex;
    }

    /// <summary>
    /// Devuelve el valor suavizado del eje configurable con el nombre dado.
    /// Si no existe un eje configurable con ese nombre, cae en el
    /// comportamiento heredado (Horizontal/Vertical) o 0.
    /// </summary>
    public static float GetAxis(string axisName)
    {
        bool have = TryRead(out var data);
        if (have && AxisIndex().TryGetValue(axisName, out int idx)
            && data.Axes != null && idx < data.Axes.Length
            && idx < data.AxisCount)
        {
            return data.Axes[idx];
        }

        // Heredado: los campos del snapshot (que el host rellena
        // desde los ejes Horizontal/Vertical). NO delegar en
        // GetAxisHorizontal: ese método llama a este.
        if (!have) return 0f;
        if (string.Equals(axisName, "Horizontal", StringComparison.OrdinalIgnoreCase))
            return data.MoveX;
        if (string.Equals(axisName, "Vertical", StringComparison.OrdinalIgnoreCase))
            return data.MoveZ;
        return 0f;
    }

    /// <summary>
    /// Devuelve el valor crudo (sin suavizar) del eje configurable con el
    /// nombre dado. Para Horizontal/Vertical heredados, no hay dato crudo
    /// disponible, así que se devuelve el mismo valor que GetAxis.
    /// </summary>
    public static float GetAxisRaw(string axisName)
    {
        bool have = TryRead(out var data);
        if (have && AxisIndex().TryGetValue(axisName, out int idx)
            && data.AxesRaw != null && idx < data.AxesRaw.Length
            && idx < data.AxisCount)
        {
            return data.AxesRaw[idx];
        }

        if (!have) return 0f;
        if (string.Equals(axisName, "Horizontal", StringComparison.OrdinalIgnoreCase))
            return data.MoveX;
        if (string.Equals(axisName, "Vertical", StringComparison.OrdinalIgnoreCase))
            return data.MoveZ;
        return 0f;
    }

    /// <summary>
    /// Devuelve el eje horizontal heredado (MoveX).
    /// </summary>
    public static float GetAxisHorizontal() => GetAxis("Horizontal");

    /// <summary>
    /// Devuelve el eje vertical heredado (MoveZ).
    /// </summary>
    public static float GetAxisVertical() => GetAxis("Vertical");

    /// <summary>
    /// Devuelve el estado del botón con el nombre dado. Si existe un eje
    /// configurable con ese nombre, consulta la máscara de bits Buttons;
    /// si no, cae en el switch heredado (jump/space/run/shift/camera).
    /// </summary>
    public static bool GetButton(string name)
    {
        if (AxisIndex().TryGetValue(name, out int idx) && TryRead(out var data))
        {
            return idx < 32 && (data.Buttons & (1u << idx)) != 0;
        }

        if (!TryRead(out var legacy))
            return false;

        return name.ToLowerInvariant() switch
        {
            "jump" or "space" => legacy.Jump != 0,
            "run" or "shift" => legacy.Run != 0,
            "camera" => legacy.Camera != 0,
            _ => false,
        };
    }

    /// <summary>
    /// Devuelve true en el frame en que el botón del eje configurable con
    /// el nombre dado pasó a presionarse. Sin eje configurable, false.
    /// </summary>
    public static bool GetButtonDown(string name)
    {
        if (AxisIndex().TryGetValue(name, out int idx) && TryRead(out var data))
        {
            return idx < 32 && (data.ButtonsDown & (1u << idx)) != 0;
        }

        return false;
    }

    /// <summary>
    /// Devuelve true en el frame en que el botón del eje configurable con
    /// el nombre dado pasó a soltarse. Sin eje configurable, false.
    /// </summary>
    public static bool GetButtonUp(string name)
    {
        if (AxisIndex().TryGetValue(name, out int idx) && TryRead(out var data))
        {
            return idx < 32 && (data.ButtonsUp & (1u << idx)) != 0;
        }

        return false;
    }

    public static bool GetKey(string keyName)
    {
        if (!TryRead(out var data))
            return false;

        return keyName.ToLowerInvariant() switch
        {
            "w" or "up" => data.MoveZ > 0.1f,
            "s" or "down" => data.MoveZ < -0.1f,
            "d" or "right" => data.MoveX > 0.1f,
            "a" or "left" => data.MoveX < -0.1f,
            "space" => data.Jump != 0,
            "shift" => data.Run != 0,
            "v" => data.Camera != 0,
            _ => false,
        };
    }

    public static bool GetKey(UnityEngine.KeyCode key)
    {
        return key switch
        {
            UnityEngine.KeyCode.W => GetKey("w"),
            UnityEngine.KeyCode.S => GetKey("s"),
            UnityEngine.KeyCode.A => GetKey("a"),
            UnityEngine.KeyCode.D => GetKey("d"),
            UnityEngine.KeyCode.Space => GetKey("space"),
            UnityEngine.KeyCode.LeftShift => GetKey("shift"),
            UnityEngine.KeyCode.UpArrow => GetKey("up"),
            UnityEngine.KeyCode.DownArrow => GetKey("down"),
            UnityEngine.KeyCode.LeftArrow => GetKey("left"),
            UnityEngine.KeyCode.RightArrow => GetKey("right"),
            _ => false
        };
    }

    public static UnityEngine.Vector2 mousePosition
    {
        get
        {
            TryRead(out var data);
            return new UnityEngine.Vector2(data.MouseX, data.MouseY);
        }
    }

    public static bool GetMouseButton(int button)
    {
        if (button != 0 || !TryRead(out var data))
            return false;

        return data.MouseLeftHeld != 0;
    }

    public static bool GetMouseButtonDown(int button)
    {
        if (button != 0 || !TryRead(out var data))
            return false;

        return data.MouseLeftDown != 0;
    }

    public static bool GetMouseButtonUp(int button)
    {
        if (button != 0 || !TryRead(out var data))
            return false;

        return data.MouseLeftUp != 0;
    }
}
