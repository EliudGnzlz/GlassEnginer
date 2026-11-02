using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core;

public static class Input
{
    private static GetInputDelegate? _getInput;

    public static void Initialize(GetInputDelegate del)
    {
        _getInput = del;
    }

    public static float GetAxisHorizontal()
    {
        if (_getInput != null)
        {
            _getInput(out var data);
            return data.MoveX;
        }
        return 0f;
    }

    public static float GetAxisVertical()
    {
        if (_getInput != null)
        {
            _getInput(out var data);
            return data.MoveZ;
        }
        return 0f;
    }

    public static float GetAxis(string axisName)
    {
        if (string.Equals(axisName, "Horizontal", StringComparison.OrdinalIgnoreCase)) return GetAxisHorizontal();
        if (string.Equals(axisName, "Vertical", StringComparison.OrdinalIgnoreCase)) return GetAxisVertical();
        return 0f;
    }

    public static bool GetButton(string name)
    {
        if (_getInput == null) return false;
        _getInput(out var data);
        return name.ToLowerInvariant() switch
        {
            "jump" or "space" => data.Jump != 0,
            "run" or "shift" => data.Run != 0,
            _ => false,
        };
    }

    public static bool GetKey(string keyName)
    {
        if (_getInput == null) return false;
        _getInput(out var data);
        return keyName.ToLowerInvariant() switch
        {
            "w" or "up" => data.MoveZ > 0.1f,
            "s" or "down" => data.MoveZ < -0.1f,
            "d" or "right" => data.MoveX > 0.1f,
            "a" or "left" => data.MoveX < -0.1f,
            "space" => data.Jump != 0,
            "shift" => data.Run != 0,
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
}
