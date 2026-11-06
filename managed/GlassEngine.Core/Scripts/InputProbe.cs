using System;
using UnityEngine;

namespace GlassEngine.Core.Scripts;

/// <summary>
/// Sonda de Input v2: registra la rampa del eje Horizontal
/// (suavizado), el valor crudo y los bordes de Jump/Fire1.
/// </summary>
public class InputProbe : GlassBehaviour
{
    private int _frames;

    public override void Update(float deltaTime)
    {
        _frames++;
        if (_frames <= 45)
        {
            Debug.Log($"INPUT f={_frames} H={Input.GetAxis("Horizontal"):F2} " +
                      $"HR={Input.GetAxisRaw("Horizontal"):F2} " +
                      $"V={Input.GetAxis("Vertical"):F2} " +
                      $"saltando={Input.GetButton("Jump")}");
        }
        if (Input.GetButtonDown("Jump")) Debug.Log("INPUT EVENTO: JUMP DOWN");
        if (Input.GetButtonUp("Jump")) Debug.Log("INPUT EVENTO: JUMP UP");
        if (Input.GetButtonDown("Fire1")) Debug.Log("INPUT EVENTO: FIRE1 DOWN");
        if (_frames == 90)
        {
            Debug.Log($"INPUT f=90 H={Input.GetAxis("Horizontal"):F2} " +
                      $"(deberia haber vuelto a ~0 tras soltar)");
        }
    }
}
